//! Outbound enrollment and latest-value delivery. Sampling never waits on the network.
use crate::{config, protocol, Collector, MAX_PAYLOAD};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use url::Url;

const STATE_BOUND: usize = 4096;
const RESPONSE_BOUND: usize = 4096;
pub const MAX_PUSH_BODY: usize = 50 * 1024;

#[derive(Clone)]
pub struct Options {
    pub collector_url: String,
    pub state_dir: PathBuf,
    pub enrollment_key_file: Option<PathBuf>,
    pub re_enroll: bool,
    pub allow_insecure_http: bool,
    pub collector_ca_cert: Option<PathBuf>,
}

/// Enrollment credentials are never formatted through Debug or error messages.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub schema: u8,
    pub collector_url: String,
    pub agent_id: String,
    pub agent_token: String,
    pub node_id: Option<String>,
    pub interval_s: Option<f64>,
    pub ttl_s: Option<f64>,
}
impl Identity {
    fn validate(&self, collector_url: &str) -> Result<()> {
        ensure!(
            self.schema == 1 && self.collector_url == collector_url,
            "Agent state belongs to another collector; use a separate state directory"
        );
        ensure!(
            is_hex_identity(&self.agent_id) && is_hex_identity(&self.agent_token),
            "Invalid saved agent identity"
        );
        match (&self.node_id, self.interval_s, self.ttl_s) {
            (None, None, None) => Ok(()),
            (Some(node), Some(interval), Some(ttl))
                if valid_node_id(node) && valid_cadence(interval, ttl) =>
            {
                Ok(())
            }
            _ => bail!("Invalid saved enrollment"),
        }
    }
    pub fn enrolled(&self) -> bool {
        self.node_id.is_some()
    }
}
pub fn is_hex_identity(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
fn valid_node_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b":_.-".contains(&c))
}
fn display_name(value: &Value) -> String {
    let name = protocol::text(value.as_str().unwrap_or("Device"), 64);
    if name.trim().is_empty() {
        "Device".into()
    } else {
        name
    }
}
fn valid_cadence(interval: f64, ttl: f64) -> bool {
    interval.is_finite()
        && (1.0..=300.0).contains(&interval)
        && ttl.is_finite()
        && (5.0..=900.0).contains(&ttl)
        && ttl >= interval * 2.0
}
pub fn random_identity() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| anyhow::anyhow!("Secure randomness unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn collector_base(raw: &str, allow_insecure_http: bool) -> Result<Url> {
    let mut url = Url::parse(raw).map_err(|_| anyhow::anyhow!("Invalid collector URL"))?;
    ensure!(
        url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && matches!(url.path(), "" | "/"),
        "Use a collector base URL without credentials, path, query or fragment"
    );
    match url.scheme() {
        "https" => {}
        "http" if allow_insecure_http => {}
        "http" => {
            bail!("HTTP requires --allow-insecure-http on a trusted network; HTTPS is recommended")
        }
        _ => bail!("Collector URL must use HTTPS or explicit trusted-network HTTP"),
    }
    url.set_path("/");
    Ok(url)
}

pub struct StateStore {
    directory: PathBuf,
    pub identity: Identity,
    _lock: File,
}
impl StateStore {
    pub fn open(directory: &Path, collector_url: &str) -> Result<Self> {
        ensure!(
            directory.is_absolute(),
            "--state-dir must be an absolute private directory"
        );
        ensure_private_directory(directory)?;
        let lock_path = directory.join("agent.lock");
        if let Ok(metadata) = fs::symlink_metadata(&lock_path) {
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Agent lock must be a regular file"
            );
        }
        let mut lock_options = OpenOptions::new();
        lock_options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            lock_options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            lock_options.share_mode(0);
        }
        let lock = lock_options
            .open(&lock_path)
            .context("Agent state is already in use or cannot be locked")?;
        #[cfg(unix)]
        {
            ensure!(
                unsafe {
                    libc::flock(
                        std::os::fd::AsRawFd::as_raw_fd(&lock),
                        libc::LOCK_EX | libc::LOCK_NB,
                    )
                } == 0,
                "Agent state is already in use"
            );
        }
        let path = directory.join("agent.json");
        let identity = if path.exists() || fs::symlink_metadata(&path).is_ok() {
            let raw = read_private_file(&path)?;
            serde_json::from_value(config::strict_json(&raw)?).map_err(|_| {
                anyhow::anyhow!("Invalid saved agent state; it has not been replaced")
            })?
        } else {
            Identity {
                schema: 1,
                collector_url: collector_url.into(),
                agent_id: random_identity()?,
                agent_token: random_identity()?,
                node_id: None,
                interval_s: None,
                ttl_s: None,
            }
        };
        let store = Self {
            directory: directory.into(),
            identity,
            _lock: lock,
        };
        store.identity.validate(collector_url)?;
        // Save before the first enrollment request: a lost response retries exactly
        // the same identity and credential instead of consuming a second pairing.
        store.save()?;
        Ok(store)
    }
    pub fn save(&self) -> Result<()> {
        self.identity.validate(&self.identity.collector_url)?;
        let raw = serde_json::to_vec_pretty(&self.identity)?;
        ensure!(raw.len() <= STATE_BOUND, "Agent state exceeds capacity");
        let mut pending = tempfile::Builder::new()
            .prefix(".agent-")
            .tempfile_in(&self.directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            pending
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        pending.write_all(&raw)?;
        pending.as_file().sync_all()?;
        pending
            .persist(self.directory.join("agent.json"))
            .map_err(|_| anyhow::anyhow!("Cannot persist agent enrollment"))?;
        #[cfg(unix)]
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
    pub fn enroll(&mut self, enrollment: Enrollment) -> Result<()> {
        ensure!(
            enrollment.schema == 1
                && enrollment.agent_id == self.identity.agent_id
                && valid_node_id(&enrollment.node_id)
                && valid_cadence(enrollment.interval_s, enrollment.ttl_s),
            "Invalid collector enrollment response"
        );
        self.identity.node_id = Some(enrollment.node_id);
        self.identity.interval_s = Some(enrollment.interval_s);
        self.identity.ttl_s = Some(enrollment.ttl_s);
        self.save()
    }
    pub fn update_cadence(&mut self, interval: f64, ttl: f64) -> Result<()> {
        ensure!(valid_cadence(interval, ttl), "Invalid collector cadence");
        if self.identity.interval_s != Some(interval) || self.identity.ttl_s != Some(ttl) {
            self.identity.interval_s = Some(interval);
            self.identity.ttl_s = Some(ttl);
            self.save()?;
        }
        Ok(())
    }
    pub fn prepare_reenrollment(&mut self) -> Result<()> {
        // A pending enrollment already has new durable credentials. Retrying it
        // after a lost reply must not rotate them and consume the grant again.
        if self.identity.enrolled() {
            self.identity.agent_token = random_identity()?;
            self.identity.node_id = None;
            self.identity.interval_s = None;
            self.identity.ttl_s = None;
            self.save()?;
        }
        Ok(())
    }
}
fn ensure_private_directory(path: &Path) -> Result<()> {
    // Reject symlinked ancestors, so credentials cannot be redirected by replacing
    // a link in an otherwise innocuous state path.
    let mut ancestor = PathBuf::new();
    for component in path.components() {
        ancestor.push(component.as_os_str());
        if let Ok(metadata) = fs::symlink_metadata(&ancestor) {
            ensure!(
                !metadata.file_type().is_symlink(),
                "Agent state directory cannot contain symlinks"
            );
        }
    }
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Agent state must be a private directory"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() },
            "Agent state directory must belong to the current user"
        );
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    windows_private_directory(path)?;
    Ok(())
}
fn read_private_file(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Agent state must be a regular private file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.uid() == unsafe { libc::geteuid() } && metadata.mode() & 0o077 == 0,
            "Agent state file must belong to the current user with mode 0600"
        );
    }
    #[cfg(windows)]
    windows_private_directory(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut raw = Vec::new();
    options
        .open(path)?
        .take((STATE_BOUND + 1) as u64)
        .read_to_end(&mut raw)?;
    ensure!(raw.len() <= STATE_BOUND, "Agent state exceeds capacity");
    Ok(raw)
}

#[derive(Deserialize)]
pub struct Enrollment {
    pub schema: u8,
    pub node_id: String,
    pub agent_id: String,
    pub interval_s: f64,
    pub ttl_s: f64,
}
#[derive(Debug)]
pub struct Acknowledgment {
    pub interval_s: Option<f64>,
    pub ttl_s: Option<f64>,
}
#[derive(Clone)]
pub struct Sample {
    pub sequence: u64,
    pub sampled_at: f64,
    pub snapshot: Value,
    sampled_mono: Instant,
}
impl Sample {
    pub fn new(sequence: u64, sampled_at: f64, mut snapshot: Value) -> Result<Self> {
        ensure!(
            sequence > 0
                && sampled_at.is_finite()
                && sampled_at > 0.0
                && snapshot["schema"] == 1
                && serde_json::to_vec(&snapshot)?.len() <= MAX_PAYLOAD,
            "Invalid outbound telemetry"
        );
        snapshot["sequence"] = json!(sequence);
        snapshot["generated_at"] = json!(sampled_at);
        Ok(Self {
            sequence,
            sampled_at,
            snapshot,
            sampled_mono: Instant::now(),
        })
    }
    pub fn fresh(&self, now: f64, ttl: f64) -> bool {
        self.sampled_mono.elapsed().as_secs_f64() <= ttl
            && now.is_finite()
            && now >= self.sampled_at
            && now - self.sampled_at <= ttl
    }
    pub fn envelope(&self, identity: &Identity, boot_id: &str) -> Result<Value> {
        ensure!(
            identity.enrolled() && is_hex_identity(boot_id),
            "Agent is not enrolled"
        );
        let mut snapshot = self.snapshot.clone();
        snapshot["host"]["name"] = json!(display_name(&snapshot["host"]["name"]));
        snapshot["node"]["name"] = json!(display_name(&snapshot["node"]["name"]));
        snapshot["node"]["id"] = json!(identity.node_id);
        snapshot["nodes"] = json!([snapshot["node"].clone()]);
        let envelope = json!({"agent_id":identity.agent_id,"node_id":identity.node_id,"boot_id":boot_id,"sequence":self.sequence,"sampled_at":self.sampled_at,"snapshot":snapshot});
        ensure!(
            serde_json::to_vec(&envelope)?.len() <= MAX_PUSH_BODY,
            "Push body exceeds capacity"
        );
        Ok(envelope)
    }
}
#[derive(Clone, Default)]
pub struct Latest {
    sample: Arc<Mutex<Option<Sample>>>,
    changed: Arc<Notify>,
}
impl Latest {
    pub fn publish(&self, sample: Sample) {
        if let Ok(mut latest) = self.sample.lock() {
            // A delayed result never overwrites a newer sample.
            if latest
                .as_ref()
                .is_none_or(|old| sample.sequence > old.sequence)
            {
                *latest = Some(sample);
                self.changed.notify_one();
            }
        }
    }
    pub fn get(&self) -> Option<Sample> {
        self.sample.lock().ok().and_then(|value| value.clone())
    }
    async fn wait(&self) {
        let _ = tokio::time::timeout(Duration::from_millis(250), self.changed.notified()).await;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryError {
    Retry,
    Unauthorized,
    Rejected,
}
impl std::fmt::Display for DeliveryError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str(match self {
            Self::Retry => "Collector unavailable",
            Self::Unauthorized => "Collector rejected the agent credential; pair the agent again",
            Self::Rejected => "Collector rejected telemetry",
        })
    }
}
impl std::error::Error for DeliveryError {}
#[derive(Clone)]
pub struct Transport {
    client: reqwest::Client,
    base: Url,
}
impl Transport {
    pub fn new(options: &Options) -> Result<Self> {
        let base = collector_base(&options.collector_url, options.allow_insecure_http)?;
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(5))
            .pool_max_idle_per_host(1)
            .user_agent(concat!("glimdock-agent/", env!("CARGO_PKG_VERSION")));
        if let Some(path) = &options.collector_ca_cert {
            let raw = config::read_bounded(path, 128 * 1024)?;
            let certificates = reqwest::Certificate::from_pem_bundle(&raw)
                .map_err(|_| anyhow::anyhow!("Invalid collector CA certificate"))?;
            ensure!(!certificates.is_empty(), "Invalid collector CA certificate");
            for certificate in certificates {
                builder = builder.add_root_certificate(certificate);
            }
        }
        Ok(Self {
            client: builder
                .build()
                .map_err(|_| anyhow::anyhow!("Cannot initialize collector connection"))?,
            base,
        })
    }
    async fn post(
        &self,
        endpoint: &str,
        token: &str,
        body: &Value,
    ) -> std::result::Result<Value, DeliveryError> {
        let url = self
            .base
            .join(endpoint)
            .map_err(|_| DeliveryError::Rejected)?;
        let mut response = self
            .client
            .post(url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(|_| DeliveryError::Retry)?;
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err(DeliveryError::Unauthorized),
            408 | 429 | 500..=599 => return Err(DeliveryError::Retry),
            _ => return Err(DeliveryError::Rejected),
        }
        let mut raw = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| DeliveryError::Retry)? {
            if raw.len() + chunk.len() > RESPONSE_BOUND {
                return Err(DeliveryError::Rejected);
            }
            raw.extend_from_slice(&chunk);
        }
        config::strict_json(&raw).map_err(|_| DeliveryError::Rejected)
    }
    pub async fn enroll(
        &self,
        identity: &Identity,
        boot_id: &str,
        descriptor: &Value,
        interval: f64,
        key: &str,
    ) -> std::result::Result<Enrollment, DeliveryError> {
        let response = self.post("api/v1/agents/enroll", key, &json!({"agent_id":identity.agent_id,"agent_token":identity.agent_token,"boot_id":boot_id,"name":display_name(&descriptor["name"]),"platform":descriptor["platform"],"kind":"server","interval_s":interval})).await?;
        serde_json::from_value(response).map_err(|_| DeliveryError::Rejected)
    }
    pub async fn push(
        &self,
        identity: &Identity,
        boot_id: &str,
        sample: &Sample,
    ) -> std::result::Result<Acknowledgment, DeliveryError> {
        let envelope = sample
            .envelope(identity, boot_id)
            .map_err(|_| DeliveryError::Rejected)?;
        let response = self
            .post("api/v1/agents/push", &identity.agent_token, &envelope)
            .await?;
        if response["schema"] == 1
            && response["accepted"] == true
            && response["node_id"].as_str() == identity.node_id.as_deref()
            && response["sequence"].as_u64() == Some(sample.sequence)
        {
            let interval_s = response.get("interval_s").and_then(Value::as_f64);
            let ttl_s = response.get("ttl_s").and_then(Value::as_f64);
            match (interval_s, ttl_s) {
                (Some(interval), Some(ttl)) if valid_cadence(interval, ttl) => {}
                (None, None)
                    if response.get("interval_s").is_none() && response.get("ttl_s").is_none() => {}
                _ => return Err(DeliveryError::Rejected),
            }
            Ok(Acknowledgment { interval_s, ttl_s })
        } else {
            Err(DeliveryError::Rejected)
        }
    }
}

pub fn retry_delay(attempt: u32, entropy: u8) -> Duration {
    let cap = 1u64 << attempt.min(5);
    // 50..100% jitter, bounded to half a second through 32 seconds.
    Duration::from_millis(cap * (500 + u64::from(entropy) * 500 / 255))
}
fn next_retry(attempt: &mut u32) -> Duration {
    let mut random = [0u8; 1];
    let _ = getrandom::fill(&mut random);
    let result = retry_delay(*attempt, random[0]);
    *attempt = attempt.saturating_add(1);
    result
}

pub fn run(options: Options, mut collector: Box<dyn Collector>) -> Result<()> {
    let transport = Transport::new(&options)?;
    let mut store = StateStore::open(&options.state_dir, transport.base.as_str())?;
    if options.re_enroll {
        let key_path = options
            .enrollment_key_file
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Re-enrollment requires --enrollment-key-file"))?;
        // Verify the supplied key file before changing a working saved binding.
        config::read_token(key_path, 32)?;
        store.prepare_reenrollment()?;
    }
    if !store.identity.enrolled() && options.enrollment_key_file.is_none() {
        bail!("First enrollment requires --enrollment-key-file");
    }
    let descriptor = collector.descriptor();
    let interval = collector.interval_s();
    ensure!(
        (1.0..=300.0).contains(&interval),
        "Invalid sampling interval"
    );
    let cadence = Arc::new(Mutex::new(store.identity.interval_s.unwrap_or(interval)));
    let worker_cadence = cadence.clone();
    let latest = Latest::default();
    let published = latest.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    let worker_descriptor = descriptor.clone();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let worker = std::thread::spawn(move || {
        let clock = Instant::now();
        let mut sequence = 0u64;
        let mut next = Instant::now();
        let mut previous_cadence = worker_cadence
            .lock()
            .map(|value| *value)
            .unwrap_or(interval);
        while !worker_stop.load(Ordering::Relaxed) {
            let delay = worker_cadence
                .lock()
                .map(|value| *value)
                .unwrap_or(interval);
            if delay != previous_cadence {
                previous_cadence = delay;
                next = Instant::now();
            }
            let remaining = next.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                std::thread::sleep(remaining.min(Duration::from_millis(100)));
                continue;
            }
            let began = Instant::now();
            let now = crate::epoch();
            sequence = sequence.saturating_add(1);
            let sampled = collector
                .sample(now, clock.elapsed().as_secs_f64())
                .or_else(|_| {
                    let mut safe = protocol::empty_snapshot(&worker_descriptor, sequence, now);
                    safe["sources"]["collector"] =
                        protocol::source_status(now, Some("Telemetry unavailable"), true);
                    protocol::finish_snapshot(safe, &worker_descriptor)
                });
            if let Ok(snapshot) = sampled {
                if let Ok(mut sample) = Sample::new(sequence, now, snapshot) {
                    sample.sampled_mono = began;
                    published.publish(sample);
                }
            }
            let delay = worker_cadence
                .lock()
                .map(|value| *value)
                .unwrap_or(interval);
            next = Instant::now() + Duration::from_secs_f64(delay).saturating_sub(began.elapsed());
        }
    });
    let result = runtime.block_on(async {
        tokio::select! {
            result = deliver(&options, &transport, &mut store, &descriptor, interval, &cadence, &latest) => result,
            _ = shutdown_signal() => Ok(()),
        }
    });
    stop.store(true, Ordering::Relaxed);
    let _ = worker.join();
    result
}
async fn deliver(
    options: &Options,
    transport: &Transport,
    store: &mut StateStore,
    descriptor: &Value,
    interval: f64,
    cadence: &Mutex<f64>,
    latest: &Latest,
) -> Result<()> {
    let boot_id = random_identity()?;
    let mut attempt = 0;
    let mut outage = false;
    while !store.identity.enrolled() {
        let key = config::read_token(options.enrollment_key_file.as_ref().unwrap(), 32)?;
        match transport
            .enroll(&store.identity, &boot_id, descriptor, interval, &key)
            .await
        {
            Ok(enrollment) => {
                store.enroll(enrollment)?;
                if let Ok(mut value) = cadence.lock() {
                    *value = store.identity.interval_s.unwrap();
                }
            }
            Err(DeliveryError::Retry) => {
                if !outage {
                    eprintln!("Collector unavailable; sampling continues while enrollment retries");
                    outage = true;
                }
                tokio::time::sleep(next_retry(&mut attempt)).await;
            }
            Err(error) => return Err(error.into()),
        }
    }
    eprintln!(
        "Agent paired as {}; pushing every {:.1}s",
        store.identity.node_id.as_deref().unwrap(),
        store.identity.interval_s.unwrap()
    );
    attempt = 0;
    outage = false;
    let mut accepted = 0;
    let mut rejected = 0;
    loop {
        let Some(sample) = latest.get() else {
            latest.wait().await;
            continue;
        };
        if sample.sequence <= accepted
            || sample.sequence <= rejected
            || !sample.fresh(crate::epoch(), store.identity.ttl_s.unwrap())
        {
            latest.wait().await;
            continue;
        }
        match transport.push(&store.identity, &boot_id, &sample).await {
            Ok(ack) => {
                if let (Some(interval), Some(ttl)) = (ack.interval_s, ack.ttl_s) {
                    store.update_cadence(interval, ttl)?;
                    if let Ok(mut value) = cadence.lock() {
                        *value = interval;
                    }
                }
                accepted = sample.sequence;
                attempt = 0;
                if outage {
                    eprintln!("Collector connection restored");
                    outage = false;
                }
            }
            Err(DeliveryError::Retry) => {
                if !outage {
                    eprintln!("Collector unavailable; sampling continues and only the latest reading is retained");
                    outage = true;
                }
                tokio::time::sleep(next_retry(&mut attempt)).await;
            }
            Err(DeliveryError::Rejected) => {
                rejected = sample.sequence;
                if !outage {
                    eprintln!("Collector rejected a reading; waiting for a fresh sample");
                    outage = true;
                }
                tokio::time::sleep(next_retry(&mut attempt)).await;
            }
            Err(DeliveryError::Unauthorized) => {
                if !outage {
                    eprintln!("Collector refused publishing; check whether this node is paused or its credential was revoked");
                    outage = true;
                }
                tokio::time::sleep(next_retry(&mut attempt)).await;
            }
        }
    }
}
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(windows)]
fn windows_private_directory(path: &Path) -> Result<()> {
    // A protected DACL grants the current account full access. Atomic state files
    // inherit it, including on non-profile locations supplied through --state-dir.
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
    type Handle = *mut c_void;
    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(
            token: Handle,
            class: u32,
            info: *mut c_void,
            size: u32,
            returned: *mut u32,
        ) -> i32;
        fn ConvertSidToStringSidW(sid: *mut c_void, text: *mut *mut u16) -> i32;
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn GetSecurityDescriptorDacl(
            descriptor: *mut c_void,
            present: *mut i32,
            dacl: *mut *mut c_void,
            defaulted: *mut i32,
        ) -> i32;
        fn SetNamedSecurityInfoW(
            name: *mut u16,
            kind: u32,
            info: u32,
            owner: *mut c_void,
            group: *mut c_void,
            dacl: *mut c_void,
            sacl: *mut c_void,
        ) -> u32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    unsafe {
        let mut token = ptr::null_mut();
        ensure!(
            OpenProcessToken(GetCurrentProcess(), 8, &mut token) != 0,
            "Cannot secure Windows agent state"
        );
        let mut required = 0;
        GetTokenInformation(token, 1, ptr::null_mut(), 0, &mut required);
        let mut buffer = vec![0usize; (required as usize).div_ceil(std::mem::size_of::<usize>())];
        let got = GetTokenInformation(
            token,
            1,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        );
        CloseHandle(token);
        ensure!(
            got != 0 && !buffer.is_empty(),
            "Cannot secure Windows agent state"
        );
        let sid = *buffer.as_ptr().cast::<*mut c_void>();
        let mut sid_text = ptr::null_mut();
        ensure!(
            ConvertSidToStringSidW(sid, &mut sid_text) != 0,
            "Cannot secure Windows agent state"
        );
        let mut length = 0;
        while *sid_text.add(length) != 0 {
            length += 1;
        }
        let account = String::from_utf16_lossy(std::slice::from_raw_parts(sid_text, length));
        LocalFree(sid_text.cast());
        let rule: Vec<u16> = format!("D:P(A;OICI;FA;;;{account})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut descriptor = ptr::null_mut();
        ensure!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                rule.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut()
            ) != 0,
            "Cannot secure Windows agent state"
        );
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = ptr::null_mut();
        let got = GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
        let mut name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let result = if got != 0 && present != 0 {
            SetNamedSecurityInfoW(
                name.as_mut_ptr(),
                1,
                4 | 0x80000000,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null_mut(),
            )
        } else {
            1
        };
        LocalFree(descriptor);
        ensure!(result == 0, "Cannot secure Windows agent state directory");
    }
    Ok(())
}
