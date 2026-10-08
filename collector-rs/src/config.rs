//! Configuration validation and privileged, fixed-scope node management.
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use url::Url;

pub const MAX_REQUEST: usize = 12 * 1024;
pub const MAX_RESPONSE: usize = 16 * 1024;
pub const SOCKET_PATH: &str = "/run/homelab-monitor/config.sock";
pub const COLLECTOR_UNIT: &str = "homelab-monitor-collector.service";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NodeConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub poll_interval_s: f64,
    pub timeout_s: f64,
    pub ttl_s: f64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub api_key_file: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub token_file: String,
}
impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            url: String::new(),
            poll_interval_s: 5.0,
            timeout_s: 2.5,
            ttl_s: 15.0,
            api_key_file: String::new(),
            token_file: String::new(),
        }
    }
}
impl NodeConfig {
    pub fn validate(&mut self, remote: bool) -> Result<()> {
        if !Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,31}$")?.is_match(&self.id) {
            bail!("Invalid stable node ID");
        }
        if self.name.is_empty() {
            self.name = self.id.clone();
        }
        if self.name.trim().is_empty()
            || self.name.chars().count() > 64
            || self.name.chars().any(|c| c < ' ')
        {
            bail!("Invalid node display name");
        }
        self.name = self.name.trim().to_string();
        if self.url.len() > 240 || self.url.chars().any(char::is_whitespace) {
            bail!("Invalid source URL");
        }
        let parsed = Url::parse(&self.url).map_err(|_| anyhow::anyhow!("Invalid source URL"))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.port() == Some(0)
            || (!matches!(parsed.path(), "" | "/")
                && !(remote && parsed.path() == "/api/v1/snapshot"))
        {
            bail!("Source URL must be a fixed HTTP(S) origin without credentials");
        }
        self.url = parsed.origin().ascii_serialization();
        for (name, value, low, high) in [
            ("poll", self.poll_interval_s, 2., 300.),
            ("timeout", self.timeout_s, 0.5, 3.),
            ("TTL", self.ttl_s, 5., 900.),
        ] {
            if !value.is_finite() || value < low || value > high {
                bail!("Invalid source {name}");
            }
        }
        if self.ttl_s < self.poll_interval_s * 2. {
            bail!("Source TTL must allow two polling intervals");
        }
        let key = if remote {
            &self.token_file
        } else {
            &self.api_key_file
        };
        if !key.is_empty() && !Path::new(key).is_absolute() {
            bail!("Source secret file must be absolute");
        }
        if remote && !self.api_key_file.is_empty() || !remote && !self.token_file.is_empty() {
            bail!("Unsupported source secret field");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub host_ip: String,
    pub node: String,
    pub display_name: String,
    pub enable_proxmox: bool,
    pub printers: Vec<NodeConfig>,
    pub remote_collectors: Vec<NodeConfig>,
    pub expected_running_guests: Vec<u64>,
    pub qga_guest_ids: Vec<u64>,
    pub truenas_ssh_host: String,
    pub truenas_ssh_user: String,
    pub truenas_ssh_key: String,
    pub truenas_known_hosts: String,
    pub truenas_interval_s: f64,
    pub truenas_guest_id: u64,
    pub guest_interval_s: f64,
    pub enable_gpus: bool,
    pub gpu_interval_s: f64,
    pub enable_faults: bool,
    pub faults_interval_s: f64,
    pub interval_s: f64,
    pub network_interfaces: Vec<String>,
    pub disk_devices: Vec<String>,
    pub smart_devices: Vec<String>,
    pub enable_turbostat: bool,
    pub enable_smart: bool,
    pub enable_zfs: bool,
    pub sensor_interval_s: f64,
    pub proxmox_interval_s: f64,
    pub turbostat_interval_s: f64,
    pub smart_interval_s: f64,
    pub zfs_interval_s: f64,
    pub temperature_warning_c: f64,
    pub temperature_critical_c: f64,
    pub disk_temperature_warning_c: f64,
    pub disk_temperature_critical_c: f64,
    pub memory_warning_pct: f64,
    pub memory_critical_pct: f64,
    pub storage_warning_pct: f64,
    pub storage_critical_pct: f64,
    pub io_wait_warning_pct: f64,
    pub io_wait_critical_pct: f64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            host_ip: "127.0.0.1".into(),
            node: String::new(),
            display_name: String::new(),
            enable_proxmox: true,
            printers: vec![],
            remote_collectors: vec![],
            expected_running_guests: vec![],
            qga_guest_ids: vec![],
            truenas_ssh_host: String::new(),
            truenas_ssh_user: "truenas_admin".into(),
            truenas_ssh_key: "/etc/homelab-monitor/truenas_ed25519".into(),
            truenas_known_hosts: "/etc/homelab-monitor/truenas_known_hosts".into(),
            truenas_interval_s: 30.,
            truenas_guest_id: 101,
            guest_interval_s: 15.,
            enable_gpus: true,
            gpu_interval_s: 30.,
            enable_faults: true,
            faults_interval_s: 30.,
            interval_s: 3.,
            network_interfaces: vec![],
            disk_devices: vec![],
            smart_devices: vec![],
            enable_turbostat: true,
            enable_smart: true,
            enable_zfs: true,
            sensor_interval_s: 6.,
            proxmox_interval_s: 6.,
            turbostat_interval_s: 6.,
            smart_interval_s: 300.,
            zfs_interval_s: 30.,
            temperature_warning_c: 80.,
            temperature_critical_c: 90.,
            disk_temperature_warning_c: 50.,
            disk_temperature_critical_c: 60.,
            memory_warning_pct: 90.,
            memory_critical_pct: 97.,
            storage_warning_pct: 85.,
            storage_critical_pct: 95.,
            io_wait_warning_pct: 20.,
            io_wait_critical_pct: 40.,
        }
    }
}
impl Config {
    pub fn read(path: &Path) -> Result<Self> {
        let raw = read_bounded(path, 64 * 1024).context("Configuration unavailable")?;
        Self::from_value(strict_json(&raw).context("Invalid configuration JSON")?)
    }
    pub fn from_value(value: Value) -> Result<Self> {
        for (field, allowed_secret) in [
            ("printers", "api_key_file"),
            ("remote_collectors", "token_file"),
        ] {
            if let Some(nodes) = value.get(field).and_then(Value::as_array) {
                for node in nodes {
                    if node
                        .get(if allowed_secret == "token_file" {
                            "api_key_file"
                        } else {
                            "token_file"
                        })
                        .is_some()
                    {
                        bail!("Unsupported node field");
                    }
                }
            }
        }
        let mut config: Self = serde_json::from_value(value)
            .map_err(|_| anyhow::anyhow!("Invalid configuration fields"))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&mut self) -> Result<()> {
        if self.host_ip.parse::<std::net::IpAddr>().is_err() {
            bail!("host_ip must be an IP address");
        }
        if !self.node.is_empty()
            && !Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$")?.is_match(&self.node)
        {
            bail!("Invalid Proxmox node name");
        }
        if self.display_name.chars().count() > 64 || self.display_name.chars().any(|c| c < ' ') {
            bail!("Invalid display name");
        }
        if self.printers.len() + self.remote_collectors.len() + self.enable_proxmox as usize
            > crate::MAX_NODES
        {
            bail!("At most four enabled nodes are supported");
        }
        for (nodes, remote) in [
            (&mut self.printers, false),
            (&mut self.remote_collectors, true),
        ] {
            let mut ids = HashSet::new();
            for node in nodes {
                node.validate(remote)?;
                if !ids.insert(node.id.clone()) {
                    bail!("Duplicate node ID");
                }
            }
        }
        for period in [
            self.interval_s,
            self.sensor_interval_s,
            self.proxmox_interval_s,
            self.turbostat_interval_s,
            self.smart_interval_s,
            self.zfs_interval_s,
            self.truenas_interval_s,
            self.faults_interval_s,
            self.guest_interval_s,
            self.gpu_interval_s,
        ] {
            if !period.is_finite() || period < 1. || period > 86400. {
                bail!("Polling intervals must be finite, from one second to one day");
            }
        }
        if self.expected_running_guests.contains(&0)
            || self.qga_guest_ids.len() > 48
            || self
                .qga_guest_ids
                .iter()
                .any(|id| !(100..=999999999).contains(id))
            || !(100..=999999999).contains(&self.truenas_guest_id)
        {
            bail!("Invalid guest IDs");
        }
        let mut ids = HashSet::new();
        self.qga_guest_ids.retain(|id| ids.insert(*id));
        if !self.truenas_ssh_host.is_empty() && self.qga_guest_ids.contains(&self.truenas_guest_id)
        {
            bail!("TrueNAS memory must have a single source");
        }
        if !self.truenas_ssh_host.is_empty()
            && !Regex::new(r"^[A-Za-z0-9][A-Za-z0-9.:-]*$")?.is_match(&self.truenas_ssh_host)
            || !Regex::new(r"^[A-Za-z_][A-Za-z0-9_-]*$")?.is_match(&self.truenas_ssh_user)
        {
            bail!("Invalid NAS SSH identity");
        }
        if !Path::new(&self.truenas_ssh_key).is_absolute()
            || !Path::new(&self.truenas_known_hosts).is_absolute()
        {
            bail!("NAS SSH file paths must be absolute");
        }
        for (low, high) in [
            (self.temperature_warning_c, self.temperature_critical_c),
            (
                self.disk_temperature_warning_c,
                self.disk_temperature_critical_c,
            ),
            (self.memory_warning_pct, self.memory_critical_pct),
            (self.storage_warning_pct, self.storage_critical_pct),
            (self.io_wait_warning_pct, self.io_wait_critical_pct),
        ] {
            if !low.is_finite() || !high.is_finite() || low >= high {
                bail!("Warning threshold must be lower than critical");
            }
        }
        Ok(())
    }
    pub fn native_name(&self) -> String {
        if self.node.is_empty() {
            hostname()
        } else {
            self.node.clone()
        }
    }
}
pub fn hostname() -> String {
    let mut buf = [0u8; 256];
    unsafe {
        libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len());
    }
    String::from_utf8_lossy(&buf[..buf.iter().position(|b| *b == 0).unwrap_or(255)])
        .split('.')
        .next()
        .unwrap_or("local")
        .to_string()
}
pub fn proxmox_node_id(name: &str) -> String {
    let mut slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_.-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !slug
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        slug = format!("node_{slug}");
    }
    if slug.len() > 55 {
        slug = format!("{}-{}", &slug[..46], &sha256(name.as_bytes())[..8]);
    }
    format!("proxmox:{slug}")
}
pub fn sha256(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
pub fn same_origin(first: &str, second: &str) -> bool {
    match (Url::parse(first), Url::parse(second)) {
        (Ok(a), Ok(b)) => a.origin() == b.origin(),
        _ => false,
    }
}
pub fn read_bounded(path: &Path, bound: usize) -> Result<Vec<u8>> {
    let mut raw = Vec::new();
    File::open(path)?
        .take((bound + 1) as u64)
        .read_to_end(&mut raw)?;
    if raw.len() > bound {
        bail!("File exceeds capacity");
    }
    Ok(raw)
}

/// Strict JSON rejects duplicate fields recursively, as well as nonfinite JSON.
pub fn strict_json(raw: &[u8]) -> Result<Value> {
    struct Strict(Value);
    impl<'de> Deserialize<'de> for Strict {
        fn deserialize<D: serde::Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = Strict;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("strict JSON")
                }
                fn visit_bool<E: serde::de::Error>(
                    self,
                    v: bool,
                ) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                    serde_json::Number::from_f64(v)
                        .map(|n| Strict(Value::Number(n)))
                        .ok_or_else(|| E::custom("nonfinite JSON"))
                }
                fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_string<E: serde::de::Error>(
                    self,
                    v: String,
                ) -> std::result::Result<Strict, E> {
                    Ok(Strict(v.into()))
                }
                fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::Null))
                }
                fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Strict, E> {
                    Ok(Strict(Value::Null))
                }
                fn visit_seq<A: serde::de::SeqAccess<'de>>(
                    self,
                    mut a: A,
                ) -> std::result::Result<Strict, A::Error> {
                    let mut v = Vec::new();
                    while let Some(x) = a.next_element::<Strict>()? {
                        v.push(x.0);
                    }
                    Ok(Strict(v.into()))
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut a: A,
                ) -> std::result::Result<Strict, A::Error> {
                    let mut v = serde_json::Map::new();
                    while let Some(k) = a.next_key::<String>()? {
                        if v.contains_key(&k) {
                            return Err(serde::de::Error::custom("duplicate configuration field"));
                        }
                        v.insert(k, a.next_value::<Strict>()?.0);
                    }
                    Ok(Strict(Value::Object(v)))
                }
            }
            de.deserialize_any(Visitor)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_slice(raw);
    let value = Strict::deserialize(&mut deserializer)
        .map_err(|_| anyhow::anyhow!("Invalid or duplicate JSON fields"))?
        .0;
    deserializer
        .end()
        .map_err(|_| anyhow::anyhow!("Trailing JSON data"))?;
    Ok(value)
}
pub fn atomic_write(path: &Path, payload: &[u8], mode: u32) -> Result<()> {
    let parent = path.parent().context("Destination has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    temporary.write_all(payload)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|_| anyhow::anyhow!("Atomic replacement failed"))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ConfigError {
    pub status: u16,
    pub message: &'static str,
}
impl ConfigError {
    pub fn new(status: u16, message: &'static str) -> Self {
        Self { status, message }
    }
}
pub struct ConfigManager {
    pub path: PathBuf,
}
impl ConfigManager {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
    fn load(&self) -> std::result::Result<(Value, Config, String), ConfigError> {
        let result = (|| -> Result<_> {
            let raw = read_bounded(&self.path, 64 * 1024)?;
            let value = strict_json(&raw)?;
            let config = Config::from_value(value.clone())?;
            Ok((value, config, sha256(&raw)))
        })();
        result.map_err(|_| ConfigError::new(503, "Configuration unavailable"))
    }
    pub fn projection(config: &Config, version: &str) -> Value {
        let native = config.native_name();
        let identity = proxmox_node_id(&native);
        let name = if config.display_name.is_empty() {
            native
        } else {
            config.display_name.clone()
        };
        let ip = &config.host_ip;
        let authority = if ip.contains(':') {
            format!("[{ip}]")
        } else {
            ip.clone()
        };
        let mut nodes = Vec::new();
        if config.enable_proxmox {
            nodes.push(json!({"id":identity,"type":"local-proxmox","origin":"local","name":name,"url":format!("http://{authority}:8765/api/v1/snapshot"),"poll_interval_s":config.interval_s,"timeout_s":null,"ttl_s":15,"has_secret":false}));
        }
        for (items, kind, prefix) in [
            (&config.printers, "klipper", "klipper"),
            (&config.remote_collectors, "proxmox-feed", "remote"),
        ] {
            for item in items {
                nodes.push(json!({"id":format!("{prefix}:{}",item.id),"type":kind,"origin":"config","name":item.name,"url":item.url,"poll_interval_s":item.poll_interval_s,"timeout_s":item.timeout_s,"ttl_s":item.ttl_s,"has_secret":!(if prefix=="remote"{&item.token_file}else{&item.api_key_file}).is_empty()}));
            }
        }
        json!({"schema":1,"version":version,"max_nodes":4,"nodes":nodes,"local_node":{"id":identity,"name":name,"address":ip,"enabled":config.enable_proxmox}})
    }
    pub fn get(&self) -> std::result::Result<Value, ConfigError> {
        let (_, c, v) = self.load()?;
        Ok(Self::projection(&c, &v))
    }
    pub fn mutate(&self, request: &Value) -> std::result::Result<Value, ConfigError> {
        let bad = || ConfigError::new(400, "Invalid node configuration request");
        let object = request.as_object().ok_or_else(bad)?;
        if object
            .keys()
            .any(|k| !["action", "version", "node", "id"].contains(&k.as_str()))
        {
            return Err(bad());
        }
        let action = request["action"].as_str().ok_or_else(bad)?;
        if !["upsert", "delete"].contains(&action) {
            return Err(bad());
        }
        let version = request["version"].as_str().ok_or_else(bad)?;
        if version.len() != 64
            || !version
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(bad());
        }
        let parent = self.path.parent().ok_or_else(bad)?;
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(parent.join(".config.lock"))
            .map_err(|_| ConfigError::new(503, "Configuration unavailable"))?;
        lock.lock_exclusive()
            .map_err(|_| ConfigError::new(503, "Configuration unavailable"))?;
        let (mut doc, config, actual) = self.load()?;
        if actual != version {
            return Err(ConfigError::new(
                409,
                "Configuration changed; reload before saving",
            ));
        }
        let local_id = proxmox_node_id(&config.native_name());
        let old_secrets: HashSet<String> = config
            .printers
            .iter()
            .map(|n| n.api_key_file.clone())
            .chain(
                config
                    .remote_collectors
                    .iter()
                    .map(|n| n.token_file.clone()),
            )
            .filter(|s| !s.is_empty())
            .collect();
        let secret_dir = parent.join("node-secrets");
        let mut new_secret: Option<(PathBuf, Vec<u8>)> = None;
        if action == "delete" {
            if request.get("node").is_some() {
                return Err(bad());
            }
            let id = request["id"].as_str().ok_or_else(bad)?;
            if id == local_id {
                doc["enable_proxmox"] = json!(false);
            } else {
                let mut found = false;
                for (field, prefix) in [("printers", "klipper"), ("remote_collectors", "remote")] {
                    if let Some(items) = doc[field].as_array_mut() {
                        items.retain(|n| {
                            let keep = format!("{prefix}:{}", n["id"].as_str().unwrap_or("")) != id;
                            found |= !keep;
                            keep
                        });
                    }
                }
                if !found {
                    return Err(ConfigError::new(400, "Unknown node id"));
                }
            }
        } else {
            if request.get("id").is_some() {
                return Err(bad());
            }
            let node = request["node"].as_object().ok_or_else(bad)?;
            if node.keys().any(|k| {
                ![
                    "id",
                    "type",
                    "name",
                    "url",
                    "poll_interval_s",
                    "timeout_s",
                    "ttl_s",
                    "secret",
                    "clear_secret",
                ]
                .contains(&k.as_str())
            }) {
                return Err(bad());
            }
            let name = node.get("name").and_then(Value::as_str).ok_or_else(bad)?;
            if name.trim().is_empty() || name.chars().count() > 64 || name.chars().any(|c| c < ' ')
            {
                return Err(bad());
            }
            let kind = node.get("type").and_then(Value::as_str).ok_or_else(bad)?;
            let raw_id = match node.get("id") {
                None => "",
                Some(v) => v.as_str().ok_or_else(bad)?,
            };
            let candidate = match node.get("secret") {
                None => "",
                Some(v) => v.as_str().ok_or_else(bad)?,
            };
            let clear = match node.get("clear_secret") {
                None => false,
                Some(v) => v.as_bool().ok_or_else(bad)?,
            };
            if candidate.len() > 256
                || !candidate.is_ascii()
                || candidate.chars().any(char::is_whitespace)
                || !candidate.is_empty() && clear
            {
                return Err(bad());
            }
            if kind == "local-proxmox" {
                if !["", local_id.as_str()].contains(&raw_id) || !candidate.is_empty() || clear {
                    return Err(bad());
                }
                let current = Self::projection(&config, &actual);
                let authority = if config.host_ip.contains(':') {
                    format!("[{}]", config.host_ip)
                } else {
                    config.host_ip.clone()
                };
                let local_url = format!("http://{authority}:8765/api/v1/snapshot");
                if let Some(url) = node.get("url") {
                    if !url.is_null()
                        && ![
                            "",
                            local_url.as_str(),
                            current["local_node"]["address"].as_str().unwrap_or(""),
                        ]
                        .contains(&url.as_str().ok_or_else(bad)?)
                    {
                        return Err(ConfigError::new(400, "Local node address is fixed"));
                    }
                }
                doc["display_name"] = json!(name.trim());
                doc["enable_proxmox"] = json!(true);
            } else {
                let (field, prefix, key) = match kind {
                    "klipper" => ("printers", "klipper", "api_key_file"),
                    "proxmox-feed" => ("remote_collectors", "remote", "token_file"),
                    _ => return Err(bad()),
                };
                if kind == "proxmox-feed" && !candidate.is_empty() && candidate.len() < 32 {
                    return Err(bad());
                }
                let existing = if field == "printers" {
                    &config.printers
                } else {
                    &config.remote_collectors
                };
                let identity =
                    generate_identity(raw_id, prefix, name, existing).map_err(|_| bad())?;
                let old = existing.iter().find(|n| n.id == identity);
                let mut updated = old
                    .map(|n| serde_json::to_value(n).unwrap())
                    .unwrap_or_else(|| json!({}));
                updated["id"] = json!(identity);
                updated["name"] = json!(name.trim());
                for k in ["url", "poll_interval_s", "timeout_s", "ttl_s"] {
                    if let Some(v) = node.get(k) {
                        updated[k] = v.clone();
                    }
                }
                if !candidate.is_empty() {
                    let bytes = format!("{candidate}\n").into_bytes();
                    let filename = format!("{prefix}-{identity}-{}.token", &sha256(&bytes)[..12]);
                    let path = secret_dir.join(filename);
                    updated[key] = json!(path);
                    new_secret = Some((path, bytes));
                } else if updated.get(key).is_none()
                    || clear
                    || old.is_some_and(|n| {
                        !same_origin(&n.url, updated["url"].as_str().unwrap_or(""))
                    })
                {
                    updated[key] = json!("");
                }
                let mut values = existing
                    .iter()
                    .filter(|n| n.id != identity)
                    .map(|n| serde_json::to_value(n).unwrap())
                    .collect::<Vec<_>>();
                values.push(updated);
                doc[field] = values.into();
            }
        }
        let validated = Config::from_value(doc.clone()).map_err(|_| {
            ConfigError::new(
                400,
                "Invalid node configuration, URL, polling, or four-node capacity",
            )
        })?;
        // Normalize only edited inventories; preserve unrelated private options.
        for (field, items) in [
            ("printers", &validated.printers),
            ("remote_collectors", &validated.remote_collectors),
        ] {
            if doc.get(field).is_some() {
                doc[field] = serde_json::to_value(items).unwrap();
            }
        }
        let mut raw = serde_json::to_vec_pretty(&doc).map_err(|_| bad())?;
        raw.push(b'\n');
        let mut created: Option<(PathBuf, bool)> = None;
        if let Some((path, secret)) = new_secret {
            fs::create_dir_all(&secret_dir)
                .map_err(|_| ConfigError::new(503, "Managed secret storage unavailable"))?;
            if fs::symlink_metadata(&secret_dir)
                .map_err(|_| ConfigError::new(503, "Managed secret storage unavailable"))?
                .file_type()
                .is_symlink()
            {
                return Err(ConfigError::new(503, "Managed secret storage unavailable"));
            }
            fs::set_permissions(&secret_dir, fs::Permissions::from_mode(0o700))
                .map_err(|_| ConfigError::new(503, "Managed secret storage unavailable"))?;
            let existed = path.exists();
            atomic_write(&path, &secret, 0o600)
                .map_err(|_| ConfigError::new(503, "Managed secret could not be saved"))?;
            created = Some((path, existed));
        }
        if atomic_write(&self.path, &raw, 0o600).is_err() {
            if let Some((path, false)) = created {
                let _ = fs::remove_file(path);
            }
            return Err(ConfigError::new(503, "Configuration could not be saved"));
        }
        let new_secrets: HashSet<_> = validated
            .printers
            .iter()
            .map(|n| n.api_key_file.clone())
            .chain(
                validated
                    .remote_collectors
                    .iter()
                    .map(|n| n.token_file.clone()),
            )
            .collect();
        let managed_filename =
            Regex::new(r"^(?:klipper|remote)-[A-Za-z0-9_.-]+-[0-9a-f]{12}\.token$").unwrap();
        for unused in old_secrets.difference(&new_secrets) {
            let path = Path::new(unused);
            if path.parent() == Some(secret_dir.as_path())
                && path
                    .file_name()
                    .is_some_and(|f| managed_filename.is_match(&f.to_string_lossy()))
            {
                let _ = fs::remove_file(path);
            }
        }
        Ok(json!({"config":Self::projection(&validated,&sha256(&raw)),"applying":true}))
    }
}
fn generate_identity(
    raw: &str,
    prefix: &str,
    name: &str,
    existing: &[NodeConfig],
) -> Result<String> {
    let slug = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_.-]{0,31}$")?;
    if !raw.is_empty() {
        let id = if let Some((p, id)) = raw.split_once(':') {
            if p != prefix {
                bail!("Wrong ID prefix");
            }
            id
        } else {
            raw
        };
        if !slug.is_match(id) {
            bail!("Invalid ID");
        }
        return Ok(id.into());
    }
    let raw = Regex::new(r"[^A-Za-z0-9_.-]+")?
        .replace_all(name, "-")
        .trim_matches(['_', '.', '-'])
        .chars()
        .take(32)
        .collect::<String>();
    let base = if raw.is_empty() {
        "node".to_string()
    } else {
        raw
    };
    let mut identity = base.clone();
    let mut index = 2;
    while existing.iter().any(|n| n.id == identity) {
        let suffix = format!("-{index}");
        identity = format!("{}{}", &base[..base.len().min(32 - suffix.len())], suffix);
        index += 1;
    }
    Ok(identity)
}
