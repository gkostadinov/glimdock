//! Scoped agent enrollment and durable, bounded latest-value ingestion.
//! The privileged configuration service owns publisher hashes and inbox files;
//! the HTTP reader forwards only these fixed operations over its private socket.
use crate::{
    config::{self, Config, ConfigError, ConfigManager, NodeConfig},
    runtime, MAX_PAYLOAD,
};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

pub const PAIRING_LIFETIME: f64 = 600.;
const MAX_INBOX: usize = MAX_PAYLOAD + 4096;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub platform: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub enabled: bool,
    pub interval_s: f64,
    pub ttl_s: f64,
    pub agent_id: String,
    pub token_hash: String,
    pub pairing_hash: String,
    pub pairing_expires_at: f64,
    pub revoked: bool,
}
impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            platform: String::new(),
            kind: "server".into(),
            enabled: true,
            interval_s: 3.,
            ttl_s: 15.,
            agent_id: String::new(),
            token_hash: String::new(),
            pairing_hash: String::new(),
            pairing_expires_at: 0.,
            revoked: false,
        }
    }
}
impl AgentConfig {
    pub fn validate(&self) -> Result<()> {
        if !crate::server::valid_node_id(&self.id)
            || !(self.id.starts_with("agent:") || self.id.starts_with("remote:"))
            || !valid_name(&self.name)
            || !config::valid_platform(&self.platform)
            || self.kind != "server"
            || !self.interval_s.is_finite()
            || !(1. ..=300.).contains(&self.interval_s)
            || !self.ttl_s.is_finite()
            || !(5. ..=900.).contains(&self.ttl_s)
            || self.ttl_s < self.interval_s * 2.
            || !self.pairing_expires_at.is_finite()
            || self.pairing_expires_at < 0.
        {
            bail!("Invalid push agent configuration");
        }
        if (!self.agent_id.is_empty() && !valid_identity(&self.agent_id))
            || (!self.token_hash.is_empty() && !valid_hash(&self.token_hash))
            || (!self.pairing_hash.is_empty() && !valid_hash(&self.pairing_hash))
            || (self.agent_id.is_empty() != self.token_hash.is_empty())
            || (self.revoked
                && (!self.token_hash.is_empty() || !self.pairing_hash.is_empty() || self.enabled))
        {
            bail!("Invalid agent credential binding");
        }
        Ok(())
    }
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 64 && !name.chars().any(|c| c < ' ')
}
pub fn valid_identity(s: &str) -> bool {
    (32..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn valid_hash(s: &str) -> bool {
    s.len() == 64 && valid_identity(s)
}
fn hash_matches(hash: &str, token: &str) -> bool {
    valid_hash(token)
        && bool::from(
            config::sha256(token.as_bytes())
                .as_bytes()
                .ct_eq(hash.as_bytes()),
        )
}
fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn projection(agent: &AgentConfig) -> Value {
    json!({"id":agent.id,"type":agent.kind,"platform":agent.platform,"origin":"agent","name":agent.name,"enabled":agent.enabled,"registered":!agent.agent_id.is_empty(),"agent_id":if agent.agent_id.is_empty(){Value::Null}else{json!(agent.agent_id)},"revoked":agent.revoked,"pairing_pending":!agent.pairing_hash.is_empty() && agent.agent_id.is_empty() && agent.pairing_expires_at>crate::epoch(),"has_secret":!agent.token_hash.is_empty(),"url":"","poll_interval_s":agent.interval_s,"timeout_s":Value::Null,"ttl_s":agent.ttl_s})
}
fn err(status: u16, message: &'static str) -> ConfigError {
    ConfigError::new(status, message)
}
fn bad() -> ConfigError {
    err(400, "Invalid agent request")
}
fn private_lock(path: &Path) -> std::result::Result<File, ConfigError> {
    let parent = path.parent().ok_or_else(bad)?;
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(parent.join(".config.lock"))
        .map_err(|_| err(503, "Agent state unavailable"))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(lock),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(err(503, "Agent state is busy"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return Err(err(503, "Agent state unavailable")),
        }
    }
}
fn load(manager: &ConfigManager) -> std::result::Result<(Value, Config, String), ConfigError> {
    let raw = config::read_bounded(&manager.path, 64 * 1024)
        .map_err(|_| err(503, "Configuration unavailable"))?;
    let doc = config::strict_json(&raw).map_err(|_| err(503, "Configuration unavailable"))?;
    let cfg = Config::from_value(doc.clone()).map_err(|_| err(503, "Configuration unavailable"))?;
    Ok((doc, cfg, config::sha256(&raw)))
}
fn save(
    manager: &ConfigManager,
    mut doc: Value,
    agents: Vec<AgentConfig>,
) -> std::result::Result<Value, ConfigError> {
    doc["push_agents"] = serde_json::to_value(agents).map_err(|_| bad())?;
    let cfg = Config::from_value(doc.clone())
        .map_err(|_| err(400, "Invalid agent settings or four-node capacity"))?;
    let mut bytes = serde_json::to_vec_pretty(&doc).map_err(|_| bad())?;
    bytes.push(b'\n');
    config::atomic_write(&manager.path, &bytes, 0o600)
        .map_err(|_| err(503, "Agent configuration could not be saved"))?;
    Ok(ConfigManager::projection(&cfg, &config::sha256(&bytes)))
}
fn inbox_path(config_path: &Path, id: &str) -> PathBuf {
    config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("agent-inbox")
        .join(format!("{}.json", config::sha256(id.as_bytes())))
}
fn remove_inbox(path: &Path, id: &str) {
    let _ = fs::remove_file(inbox_path(path, id));
}
fn prepare_inbox(path: &Path) -> Result<()> {
    let parent = path.parent().context("Inbox directory unavailable")?;
    fs::create_dir_all(parent)?;
    let meta = fs::symlink_metadata(parent)?;
    if !meta.is_dir() || meta.file_type().is_symlink() || meta.uid() != unsafe { libc::geteuid() } {
        bail!("Unsafe inbox directory");
    }
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
pub fn is_management_request(manager: &ConfigManager, request: &Value) -> bool {
    if matches!(
        request["action"].as_str(),
        Some("pair-agent" | "revoke-agent")
    ) || request["node"]["origin"] == "agent"
        || request["node"]["type"] == "server-push"
    {
        return true;
    }
    let id = request["id"]
        .as_str()
        .or_else(|| request["node"]["id"].as_str())
        .unwrap_or("");
    !id.is_empty()
        && load(manager).is_ok_and(|(_, cfg, _)| cfg.push_agents.iter().any(|n| n.id == id))
}
/// Management enrollment grants are never returned by GET or stored as plaintext.
pub fn manage(manager: &ConfigManager, request: &Value) -> std::result::Result<Value, ConfigError> {
    let obj = request.as_object().ok_or_else(bad)?;
    if obj
        .keys()
        .any(|k| !["action", "version", "node", "id"].contains(&k.as_str()))
    {
        return Err(bad());
    }
    let action = request["action"].as_str().ok_or_else(bad)?;
    if !["pair-agent", "revoke-agent", "upsert", "delete"].contains(&action) {
        return Err(bad());
    }
    let _lock = private_lock(&manager.path)?;
    let (mut doc, cfg, actual) = load(manager)?;
    if request["version"].as_str() != Some(&actual) {
        return Err(err(409, "Configuration changed; reload before saving"));
    }
    let mut agents = cfg.push_agents.clone();
    if ["delete", "revoke-agent"].contains(&action) {
        if request.get("node").is_some() {
            return Err(bad());
        }
        let id = request["id"].as_str().ok_or_else(bad)?;
        let index = agents
            .iter()
            .position(|n| n.id == id)
            .ok_or_else(|| err(400, "Unknown agent node"))?;
        if action == "delete" {
            agents.remove(index);
        } else {
            let a = &mut agents[index];
            a.token_hash.clear();
            a.agent_id.clear();
            a.pairing_hash.clear();
            a.pairing_expires_at = 0.;
            a.enabled = false;
            a.revoked = true;
        }
        let projection = save(manager, doc, agents)?;
        remove_inbox(&manager.path, id);
        return Ok(json!({"config":projection,"applying":true}));
    }
    if request.get("id").is_some() {
        return Err(bad());
    }
    let node = request["node"].as_object().ok_or_else(bad)?;
    if node.keys().any(|k| {
        ![
            "id",
            "name",
            "platform",
            "kind",
            "type",
            "origin",
            "enabled",
            "interval_s",
            "poll_interval_s",
            "ttl_s",
            "url",
            "timeout_s",
        ]
        .contains(&k.as_str())
    }) {
        return Err(bad());
    }
    if node.get("url").is_some_and(|v| v != "" && !v.is_null())
        || node.get("timeout_s").is_some_and(|v| !v.is_null())
        || node.get("origin").is_some_and(|v| v != "agent")
    {
        return Err(bad());
    }
    let name = request["node"]["name"]
        .as_str()
        .filter(|n| valid_name(n))
        .ok_or_else(bad)?
        .trim()
        .to_string();
    let raw_id = request["node"]["id"].as_str().unwrap_or("");
    let mut id = raw_id.to_string();
    if id.is_empty() {
        if action != "pair-agent" {
            return Err(bad());
        }
        let stem = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "_.-".contains(c) {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .trim_matches(['.', '_', '-'])
            .chars()
            .take(24)
            .collect::<String>();
        let stem = if stem.is_empty() { "node" } else { &stem };
        id = format!("agent:{stem}");
        let mut suffix = 2;
        while agents.iter().any(|a| a.id == id) {
            id = format!("agent:{stem}-{suffix}");
            suffix += 1;
        }
    }
    if !crate::server::valid_node_id(&id)
        || !(id.starts_with("agent:") || id.starts_with("remote:"))
    {
        return Err(bad());
    }
    let existing = agents.iter().position(|a| a.id == id);
    let remote = cfg
        .remote_collectors
        .iter()
        .find(|n| format!("remote:{}", n.id) == id);
    if existing.is_none() && (action != "pair-agent" || (!raw_id.is_empty() && remote.is_none())) {
        return Err(err(400, "Unknown agent node"));
    }
    if remote.is_some_and(|r| r.node_type != "server") {
        return Err(err(400, "Only generic server feeds can migrate to push"));
    }
    let mut a = existing
        .map(|i| agents[i].clone())
        .unwrap_or_else(|| AgentConfig {
            id: id.clone(),
            platform: remote.map(|r| r.platform.clone()).unwrap_or_default(),
            ..Default::default()
        });
    a.name = name;
    if action == "upsert"
        && !a.agent_id.is_empty()
        && node
            .get("platform")
            .is_some_and(|p| p.as_str() != Some(a.platform.as_str()))
    {
        return Err(err(409, "Re-pair the agent to change its platform"));
    }
    if let Some(v) = node.get("platform") {
        a.platform = v
            .as_str()
            .filter(|p| config::valid_platform(p))
            .ok_or_else(bad)?
            .to_string();
    }
    if node.get("kind").is_some_and(|v| v != "server")
        || node
            .get("type")
            .is_some_and(|v| v != "server" && v != "server-push")
    {
        return Err(bad());
    }
    if let Some(v) = node
        .get("interval_s")
        .or_else(|| node.get("poll_interval_s"))
    {
        a.interval_s = v.as_f64().ok_or_else(bad)?;
    }
    if let Some(v) = node.get("ttl_s") {
        a.ttl_s = v.as_f64().ok_or_else(bad)?;
    }
    if let Some(v) = node.get("enabled") {
        a.enabled = v.as_bool().ok_or_else(bad)?;
    }
    let pairing = if action == "pair-agent" {
        let token = random_token().map_err(|_| err(503, "Secure pairing key unavailable"))?;
        a.pairing_hash = config::sha256(token.as_bytes());
        a.pairing_expires_at = crate::epoch() + PAIRING_LIFETIME;
        a.agent_id.clear();
        a.token_hash.clear();
        a.revoked = false;
        a.enabled = true;
        Some(
            json!({"token":token,"expires_at":a.pairing_expires_at,"node_id":a.id,"interval_s":a.interval_s,"ttl_s":a.ttl_s}),
        )
    } else {
        if a.revoked && a.enabled {
            return Err(err(400, "Re-pair a revoked agent before enabling"));
        }
        None
    };
    a.validate().map_err(|_| bad())?;
    if let Some(i) = existing {
        agents[i] = a;
    } else {
        agents.push(a);
    }
    if remote.is_some() {
        doc["remote_collectors"] = json!(cfg
            .remote_collectors
            .iter()
            .filter(|r| format!("remote:{}", r.id) != id)
            .collect::<Vec<_>>());
    }
    let projection = save(manager, doc, agents)?;
    if pairing.is_some() {
        remove_inbox(&manager.path, &id);
    }
    let mut response = json!({"config":projection,"applying":true});
    if let Some(pairing) = pairing {
        response["pairing"] = pairing;
    }
    Ok(response)
}
/// Enrollment is idempotent only for the exact pre-generated publisher binding.
pub fn enroll(
    manager: &ConfigManager,
    token: &str,
    request: &Value,
    now: f64,
) -> std::result::Result<Value, ConfigError> {
    let obj = request.as_object().ok_or_else(bad)?;
    if obj.keys().any(|k| {
        ![
            "agent_id",
            "agent_token",
            "name",
            "platform",
            "kind",
            "boot_id",
            "interval_s",
        ]
        .contains(&k.as_str())
    }) {
        return Err(bad());
    }
    let agent_id = request["agent_id"]
        .as_str()
        .filter(|s| valid_identity(s))
        .ok_or_else(bad)?;
    let agent_token = request["agent_token"]
        .as_str()
        .filter(|s| valid_hash(s))
        .ok_or_else(bad)?;
    let platform = request["platform"]
        .as_str()
        .filter(|p| config::valid_platform(p) && !p.is_empty())
        .ok_or_else(bad)?;
    if request["kind"] != "server"
        || !request["name"].as_str().is_some_and(valid_name)
        || !request["boot_id"].as_str().is_some_and(valid_identity)
    {
        return Err(bad());
    }
    let _lock = private_lock(&manager.path)?;
    let (doc, cfg, _) = load(manager)?;
    let mut agents = cfg.push_agents;
    let index = agents
        .iter()
        .position(|a| hash_matches(&a.pairing_hash, token))
        .ok_or_else(|| err(401, "Invalid or expired pairing key"))?;
    if agents
        .iter()
        .enumerate()
        .any(|(i, a)| i != index && a.agent_id == agent_id)
    {
        return Err(err(409, "Agent identity is already registered"));
    }
    let requested_hash = config::sha256(agent_token.as_bytes());
    if agents
        .iter()
        .enumerate()
        .any(|(i, a)| i != index && a.token_hash == requested_hash)
    {
        return Err(err(409, "Publisher key is already registered"));
    }
    let a = &mut agents[index];
    if !a.enabled || a.revoked {
        return Err(err(403, "Agent is paused or revoked"));
    }
    if !a.platform.is_empty() && a.platform != platform {
        return Err(err(409, "Agent platform does not match pairing"));
    }
    let token_hash = config::sha256(agent_token.as_bytes());
    if token_hash == a.pairing_hash {
        return Err(err(400, "Publisher key must differ from pairing key"));
    }
    let duplicate = !a.agent_id.is_empty();
    // Expiration closes the grant to new publishers. An exact durable binding
    // may still recover a lost response using that original grant after an
    // outage; revocation/re-pairing removes the original grant hash entirely.
    if a.pairing_expires_at <= now
        && (!duplicate || a.agent_id != agent_id || a.token_hash != token_hash)
    {
        return Err(err(401, "Invalid or expired pairing key"));
    }
    if duplicate && (a.agent_id != agent_id || a.token_hash != token_hash) {
        return Err(err(409, "Pairing key has already been used"));
    }
    if !duplicate {
        a.agent_id = agent_id.into();
        a.token_hash = token_hash;
        a.platform = platform.into();
        if let Some(interval) = request.get("interval_s") {
            let interval = interval
                .as_f64()
                .filter(|v| v.is_finite() && (1. ..=300.).contains(v))
                .ok_or_else(bad)?;
            a.interval_s = interval;
            a.ttl_s = a.ttl_s.max(interval * 3.).min(900.);
        }
    }
    let result = json!({"schema":1,"node_id":a.id,"agent_id":a.agent_id,"interval_s":a.interval_s,"ttl_s":a.ttl_s,"duplicate":duplicate});
    if !duplicate {
        save(manager, doc, agents)?;
    }
    Ok(result)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inbox {
    agent_id: String,
    boot_id: String,
    sequence: u64,
    sampled_at: f64,
    last_seen: f64,
    payload_hash: String,
    retired_boots: Vec<String>,
    snapshot: Value,
}
fn read_inbox(path: &Path) -> Result<Option<Inbox>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(m) if !m.is_file() || m.permissions().mode() & 0o077 != 0 => bail!("Unsafe agent inbox"),
        _ => {}
    }
    let mut raw = Vec::new();
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?
        .take((MAX_INBOX + 1) as u64)
        .read_to_end(&mut raw)?;
    if raw.len() > MAX_INBOX {
        bail!("Agent inbox exceeds capacity");
    }
    let inbox: Inbox = serde_json::from_value(config::strict_json(&raw)?)?;
    if inbox.retired_boots.len() > 16
        || !valid_identity(&inbox.agent_id)
        || !valid_identity(&inbox.boot_id)
        || !valid_hash(&inbox.payload_hash)
        || !inbox.sampled_at.is_finite()
        || !inbox.last_seen.is_finite()
    {
        bail!("Invalid agent inbox");
    }
    Ok(Some(inbox))
}
/// A retry may update last_seen; it never changes the measurement timestamp.
pub fn ingest(
    manager: &ConfigManager,
    token: &str,
    request: &Value,
    now: f64,
) -> std::result::Result<Value, ConfigError> {
    let obj = request.as_object().ok_or_else(bad)?;
    if obj.keys().any(|k| {
        ![
            "agent_id",
            "node_id",
            "boot_id",
            "sequence",
            "sampled_at",
            "snapshot",
        ]
        .contains(&k.as_str())
    }) {
        return Err(bad());
    }
    let id = request["node_id"].as_str().ok_or_else(bad)?;
    let agent_id = request["agent_id"]
        .as_str()
        .filter(|s| valid_identity(s))
        .ok_or_else(bad)?;
    let boot = request["boot_id"]
        .as_str()
        .filter(|s| valid_identity(s))
        .ok_or_else(bad)?;
    let sequence = request["sequence"]
        .as_u64()
        .filter(|n| *n > 0)
        .ok_or_else(bad)?;
    let stamp = request["sampled_at"]
        .as_f64()
        .filter(|n| n.is_finite() && *n > 0.)
        .ok_or_else(bad)?;
    let _lock = private_lock(&manager.path)?;
    let (_, cfg, _) = load(manager)?;
    let a = cfg
        .push_agents
        .iter()
        .find(|a| a.id == id && a.agent_id == agent_id && hash_matches(&a.token_hash, token))
        .ok_or_else(|| err(401, "Invalid agent credential"))?;
    if !a.enabled || a.revoked {
        return Err(err(403, "Agent is paused or revoked"));
    }
    if stamp > now + 30. || now - stamp > a.ttl_s {
        return Err(err(409, "Agent sample is expired or future dated"));
    }
    let sample = &request["snapshot"];
    if !sample["host"]["name"].as_str().is_some_and(valid_name)
        || !sample["host"]["ip"]
            .as_str()
            .is_some_and(|s| s.len() <= 255 && !s.chars().any(char::is_control))
    {
        return Err(err(400, "Invalid agent host identity"));
    }
    if sample["node"]["platform"].as_str() != Some(a.platform.as_str())
        || sample["generated_at"].as_f64() != Some(stamp)
        || sample["sequence"].as_u64() != Some(sequence)
    {
        return Err(bad());
    }
    let mut reader = crate::remote_feeds::RemoteCollectorReader::new(NodeConfig {
        id: "push".into(),
        node_type: a.kind.clone(),
        platform: a.platform.clone(),
        url: "http://127.0.0.1".into(),
        ttl_s: a.ttl_s,
        ..Default::default()
    })
    .map_err(|_| bad())?;
    let validated = reader
        .validate(sample.clone(), now)
        .map_err(|_| err(400, "Invalid agent telemetry snapshot"))?;
    let safe = runtime::bounded_snapshot(validated["snapshot"].clone());
    if serde_json::to_vec(&safe).map_or(true, |v| v.len() > MAX_PAYLOAD) {
        return Err(err(413, "Agent telemetry exceeds capacity"));
    }
    let payload_hash = config::sha256(&serde_json::to_vec(&safe).map_err(|_| bad())?);
    let path = inbox_path(&manager.path, id);
    let old = read_inbox(&path).map_err(|_| err(503, "Agent inbox unavailable"))?;
    let mut retired = Vec::new();
    let mut duplicate = false;
    if let Some(old) = old {
        if old.agent_id != agent_id {
            return Err(err(409, "Agent binding changed; re-pair"));
        }
        retired = old.retired_boots;
        if old.boot_id == boot {
            if sequence < old.sequence || stamp < old.sampled_at {
                return Err(err(409, "Agent sample is out of order"));
            }
            if sequence == old.sequence {
                if old.payload_hash != payload_hash || stamp != old.sampled_at {
                    return Err(err(409, "Agent sequence already used"));
                }
                duplicate = true;
            } else if stamp <= old.sampled_at {
                return Err(err(409, "Agent sample time did not advance"));
            }
        } else {
            if retired.iter().any(|s| s == boot) || stamp <= old.sampled_at {
                return Err(err(409, "Agent session is retired or stale"));
            }
            retired.push(old.boot_id);
            if retired.len() > 16 {
                retired.remove(0);
            }
        }
    }
    let inbox = Inbox {
        agent_id: agent_id.into(),
        boot_id: boot.into(),
        sequence,
        sampled_at: stamp,
        last_seen: now,
        payload_hash,
        retired_boots: retired,
        snapshot: safe,
    };
    let bytes = serde_json::to_vec(&inbox).map_err(|_| bad())?;
    if bytes.len() > MAX_INBOX {
        return Err(err(413, "Agent telemetry exceeds capacity"));
    }
    prepare_inbox(&path)
        .and_then(|_| config::atomic_write(&path, &bytes, 0o600))
        .map_err(|_| err(503, "Agent telemetry could not be saved"))?;
    Ok(
        json!({"schema":1,"accepted":true,"duplicate":duplicate,"node_id":id,"sequence":sequence,"received_at":now,"interval_s":a.interval_s,"ttl_s":a.ttl_s}),
    )
}
/// Ingestion is transport freshness; measurement freshness remains sampled_at.
pub fn node_snapshot(
    a: &AgentConfig,
    config_path: Option<&Path>,
    sequence: u64,
    now: f64,
) -> Value {
    let inbox = config_path
        .and_then(|p| read_inbox(&inbox_path(p, &a.id)).ok().flatten())
        .filter(|i| i.agent_id == a.agent_id && !a.agent_id.is_empty());
    let stamp = inbox.as_ref().map(|i| i.sampled_at);
    let last_seen = inbox.as_ref().map(|i| i.last_seen);
    let age = stamp.map(|s| (now - s).max(0.));
    let fresh = age.is_some_and(|age| age <= a.ttl_s) && stamp.is_some_and(|s| s <= now + 30.);
    let mut snapshot = if fresh {
        inbox.as_ref().unwrap().snapshot.clone()
    } else {
        runtime::empty_snapshot(&a.name, "", sequence, now)
    };
    snapshot["host"]["name"] = json!(a.name);
    snapshot["sequence"] = json!(sequence);
    snapshot["generated_at"] = json!(now);
    if let Some(sources) = snapshot["sources"].as_object_mut() {
        for source in sources.values_mut() {
            if let Some(t) = source["updated_at"].as_f64() {
                source["age_s"] = json!((now - t).max(0.));
            }
        }
    }
    for (field, stamp_field, age_field) in [
        ("sensors", "updated_at", "age_s"),
        ("gpus", "updated_at", "age_s"),
        ("guests", "mem_updated_at", "mem_age_s"),
    ] {
        if let Some(items) = snapshot[field].as_array_mut() {
            for item in items {
                if let Some(t) = item[stamp_field].as_f64() {
                    item[age_field] = json!((now - t).max(0.));
                }
            }
        }
    }
    let reason = if a.agent_id.is_empty() {
        "Agent has not enrolled"
    } else {
        "Agent telemetry expired or unavailable"
    };
    snapshot["sources"]["agent_push"] = json!({"enabled":true,"ok":fresh,"updated_at":stamp,"age_s":age,"error":if fresh{Value::Null}else{json!(reason)}});
    snapshot["agent"] = json!({"last_seen":last_seen,"sampled_at":stamp,"age_s":age,"ttl_s":a.ttl_s,"session":inbox.as_ref().map(|i|&i.boot_id),"sequence":inbox.as_ref().map(|i|i.sequence)});
    if !fresh {
        snapshot["alerts"] =
            json!([{"id":format!("agent/{}/offline",a.id),"severity":"warning","message":reason}]);
    }
    let address = snapshot["host"]["ip"].as_str().unwrap_or("").to_string();
    json!({"id":a.id,"type":a.kind,"platform":a.platform,"origin":"agent","name":a.name,"address":address,"last_seen":last_seen,"sample_at":stamp,"snapshot":runtime::bounded_snapshot(snapshot)})
}
