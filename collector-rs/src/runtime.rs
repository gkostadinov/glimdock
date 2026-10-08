//! Atomic publication and independent single-flight source scheduling.
use crate::{
    config::{self, Config, NodeConfig},
    printers::{printer_snapshot, MoonrakerReader},
    remote_feeds::{remote_snapshot, RemoteCollectorReader},
    MAX_AGGREGATE, MAX_PAYLOAD,
};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{sync::Mutex, task::JoinHandle};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceStatus {
    pub enabled: bool,
    pub ok: bool,
    pub updated_at: Option<f64>,
    pub age_s: Option<f64>,
    pub error: Option<String>,
}
impl Default for SourceStatus {
    fn default() -> Self {
        Self {
            enabled: true,
            ok: false,
            updated_at: None,
            age_s: None,
            error: Some("initializing".into()),
        }
    }
}
pub fn empty_snapshot(name: &str, address: &str, sequence: u64, now: f64) -> Value {
    let mut host = json!({"name":name,"ip":address,"cpu_cores":[],"load":[]});
    for key in [
        "uptime_s",
        "cpu_pct",
        "mem_used_bytes",
        "mem_total_bytes",
        "swap_used_bytes",
        "swap_total_bytes",
        "arc_bytes",
        "io_wait_pct",
        "net_rx_bps",
        "net_tx_bps",
        "disk_read_bps",
        "disk_write_bps",
    ] {
        host[key] = Value::Null;
    }
    json!({"schema":1,"sequence":sequence,"generated_at":now,"host":host,"power":{"package_w":null,"cores_w":null,"graphics_w":null,"cpu_mhz":null,"busy_mhz":null,"cpu_temp_c":null,"cstate_pct":{}},"guests":[],"storage":[],"disks":[],"sensors":[],"gpus":[],"alerts":[],"sources":{},"faults":{"lookback_days":7,"segfault_count_24h":null,"event_count_24h":null,"last_event_at":null,"events":[]},"limits":{"counts":{}}})
}
pub fn bounded_snapshot(mut snapshot: Value) -> Value {
    const CAPS: &[(&str, usize)] = &[
        ("guests", 48),
        ("sensors", 64),
        ("storage", 16),
        ("disks", 16),
        ("gpus", 8),
    ];
    for field in ["guests", "sensors", "storage", "disks", "gpus", "alerts"] {
        if !snapshot[field].is_array() {
            snapshot[field] = json!([]);
        }
    }
    if !snapshot["limits"].is_object() {
        snapshot["limits"] = json!({});
    }
    if !snapshot["limits"]["counts"].is_object() {
        snapshot["limits"]["counts"] = json!({});
    }
    snapshot["limits"]["truncated"] = json!({});
    for (field, cap) in CAPS {
        let actual = snapshot[*field].as_array().map_or(0, Vec::len);
        let count = snapshot["limits"]["counts"][*field]
            .as_u64()
            .unwrap_or(actual as u64)
            .max(actual as u64);
        snapshot["limits"]["counts"][*field] = json!(count);
        if let Some(items) = snapshot[*field].as_array_mut() {
            items.truncate(*cap);
        }
        snapshot["limits"]["truncated"][*field] =
            json!(count.saturating_sub(actual.min(*cap) as u64));
        snapshot["limits"][format!("max_{field}")] = json!(cap);
    }
    snapshot["limits"]["max_payload_bytes"] = json!(MAX_PAYLOAD);
    if let Some(alerts) = snapshot["alerts"].as_array_mut() {
        alerts.truncate(24);
    }
    if let Some(events) = snapshot["faults"]["events"].as_array_mut() {
        events.truncate(16);
    }
    // Upstream text/object expansion cannot exceed the display's wire bound.
    while serde_json::to_vec(&snapshot).map_or(usize::MAX, |v| v.len()) > MAX_PAYLOAD {
        let largest = CAPS
            .iter()
            .filter(|(f, _)| snapshot[*f].as_array().is_some_and(|a| !a.is_empty()))
            .max_by_key(|(f, _)| serde_json::to_vec(&snapshot[*f]).map_or(0, |v| v.len()))
            .map(|(f, _)| *f);
        if let Some(field) = largest {
            snapshot[field].as_array_mut().unwrap().pop();
            let n = snapshot["limits"]["truncated"][field].as_u64().unwrap_or(0);
            snapshot["limits"]["truncated"][field] = json!(n + 1);
        } else {
            let mut safe = empty_snapshot(
                snapshot["host"]["name"].as_str().unwrap_or("node"),
                snapshot["host"]["ip"].as_str().unwrap_or(""),
                snapshot["sequence"].as_u64().unwrap_or(0),
                crate::number(&snapshot["generated_at"]).unwrap_or_else(crate::epoch),
            );
            safe["alerts"] = json!([{"id":"capacity","severity":"warning","message":"Telemetry exceeded display capacity"}]);
            if let Some(node) = snapshot.get("node") {
                safe["node"] = node.clone();
            }
            if let Some(nodes) = snapshot.get("nodes") {
                safe["nodes"] = nodes.clone();
            }
            return safe;
        }
    }
    let truncated = snapshot["limits"]["truncated"]
        .as_object()
        .is_some_and(|v| v.values().any(|n| n.as_u64().unwrap_or(0) > 0));
    if truncated {
        let alerts = snapshot["alerts"].as_array_mut().unwrap();
        if !alerts.iter().any(|a| a["id"] == "capacity") {
            alerts.insert(0,json!({"id":"capacity","severity":"warning","message":"Some telemetry was omitted to fit display capacity"}));
            alerts.truncate(24);
        }
    }
    if serde_json::to_vec(&snapshot).map_or(usize::MAX, |v| v.len()) > MAX_PAYLOAD {
        if let Some(alerts) = snapshot["alerts"].as_array_mut() {
            alerts.truncate(1);
        }
        return bounded_snapshot(snapshot);
    }
    snapshot
}
pub fn http_client(timeout: f64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs_f64(timeout))
        .connect_timeout(Duration::from_secs_f64(timeout))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("HTTP client initialization failed")
}
pub fn read_secret(path: &str, minimum: usize) -> Result<String> {
    let raw = config::read_bounded(Path::new(path), 512).context("Source secret unavailable")?;
    let secret = String::from_utf8(raw)
        .map_err(|_| anyhow::anyhow!("Invalid source secret"))?
        .trim()
        .to_string();
    if secret.len() < minimum
        || secret.len() > 256
        || !secret.is_ascii()
        || secret.chars().any(char::is_whitespace)
    {
        bail!("Invalid source secret");
    }
    Ok(secret)
}
pub async fn bounded_http_json(request: reqwest::RequestBuilder, limit: usize) -> Result<Value> {
    let mut response = request
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Source connection failed"))?;
    if !response.status().is_success() {
        bail!("Source HTTP request failed");
    }
    if response.content_length().is_some_and(|n| n > limit as u64) {
        bail!("Source response exceeds capacity");
    }
    let mut raw = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow::anyhow!("Source response failed"))?
    {
        if raw.len() + chunk.len() > limit {
            bail!("Source response exceeds capacity");
        }
        raw.extend_from_slice(&chunk);
    }
    config::strict_json(&raw).context("Invalid source JSON")
}

enum Reader {
    Printer(MoonrakerReader),
    Remote(RemoteCollectorReader),
}
struct Source {
    config: NodeConfig,
    reader: Arc<Mutex<Reader>>,
    handle: Option<JoinHandle<Result<Value>>>,
    next_run: Instant,
    status: SourceStatus,
    data: Option<Value>,
}
impl Source {
    fn new(config: NodeConfig, remote: bool) -> Result<Self> {
        let reader = if remote {
            Reader::Remote(RemoteCollectorReader::new(config.clone())?)
        } else {
            Reader::Printer(MoonrakerReader::new(config.clone())?)
        };
        Ok(Self {
            config,
            reader: Arc::new(Mutex::new(reader)),
            handle: None,
            next_run: Instant::now(),
            status: SourceStatus::default(),
            data: None,
        })
    }
    async fn advance(&mut self, now: f64) {
        if self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            let result = self.handle.take().unwrap().await;
            match result {
                Ok(Ok(data)) => {
                    self.status.ok = true;
                    self.status.error = None;
                    self.status.updated_at = Some(now);
                    self.data = Some(data);
                }
                _ => {
                    self.status.ok = false;
                    self.status.error = Some("Source collection failed".into());
                }
            }
        }
        self.status.age_s = self.status.updated_at.map(|t| (now - t).max(0.));
        if self.status.age_s.is_some_and(|a| a > self.config.ttl_s) {
            self.status.ok = false;
            self.status.error = Some("Source sample expired".into());
        }
        if self.handle.is_none() && Instant::now() >= self.next_run {
            let reader = self.reader.clone();
            self.handle = Some(tokio::spawn(async move {
                match &mut *reader.lock().await {
                    Reader::Printer(r) => r.read().await,
                    Reader::Remote(r) => r.read().await,
                }
            }));
            self.next_run = Instant::now() + Duration::from_secs_f64(self.config.poll_interval_s);
        }
    }
    async fn initialized(&mut self) {
        if let Some(handle) = self.handle.take() {
            match handle.await {
                Ok(Ok(data)) => {
                    self.status.ok = true;
                    self.status.error = None;
                    self.status.updated_at = Some(crate::epoch());
                    self.data = Some(data);
                }
                _ => {
                    self.status.ok = false;
                    self.status.error = Some("Source collection failed".into());
                }
            }
        }
    }
}
pub struct Collector {
    config: Config,
    local: Option<crate::probes::LocalCollector>,
    printers: Vec<Source>,
    remotes: Vec<Source>,
    sequence: u64,
}
impl Collector {
    pub fn new(config: Config) -> Result<Self> {
        let local = if config.enable_proxmox {
            Some(crate::probes::LocalCollector::new(&config))
        } else {
            None
        };
        let printers = config
            .printers
            .iter()
            .cloned()
            .map(|c| Source::new(c, false))
            .collect::<Result<Vec<_>>>()?;
        let remotes = config
            .remote_collectors
            .iter()
            .cloned()
            .map(|c| Source::new(c, true))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            config,
            local,
            printers,
            remotes,
            sequence: 0,
        })
    }
    pub async fn sample(&mut self) -> Result<Value> {
        self.sequence += 1;
        let now = crate::epoch();
        let mut nodes = Vec::new();
        if let Some(local) = self.local.as_mut() {
            let snapshot = local.collect(&self.config, self.sequence).await;
            let native = self.config.native_name();
            nodes.push(json!({"id":config::proxmox_node_id(&native),"type":"proxmox","name":if self.config.display_name.is_empty(){native}else{self.config.display_name.clone()},"address":self.config.host_ip,"snapshot":bounded_snapshot(snapshot)}));
        }
        for source in &mut self.printers {
            source.advance(now).await;
            let snapshot = bounded_snapshot(printer_snapshot(
                &source.config,
                &source.status,
                source.data.as_ref(),
                self.sequence,
                now,
            ));
            nodes.push(json!({"id":format!("klipper:{}",source.config.id),"type":"klipper","name":source.config.name,"address":source_address(&source.config.url),"snapshot":snapshot}));
        }
        for source in &mut self.remotes {
            source.advance(now).await;
            let snapshot = bounded_snapshot(remote_snapshot(
                &source.config,
                &source.status,
                source.data.as_ref(),
                self.sequence,
                now,
            ));
            nodes.push(json!({"id":format!("remote:{}",source.config.id),"type":"proxmox","name":source.config.name,"address":source_address(&source.config.url),"snapshot":snapshot}));
        }
        for node in &mut nodes {
            node["status"] = json!(crate::server::node_status(&node["snapshot"], now));
        }
        let aggregate =
            json!({"schema":2,"sequence":self.sequence,"generated_at":now,"nodes":nodes});
        if serde_json::to_vec(&aggregate)?.len() > MAX_AGGREGATE {
            bail!("Node aggregate exceeds capacity");
        }
        Ok(aggregate)
    }
    pub async fn initialize(&mut self) {
        for source in self.printers.iter_mut().chain(self.remotes.iter_mut()) {
            source.initialized().await;
        }
        if let Some(local) = self.local.as_mut() {
            local.initialize().await;
        }
    }
}
fn source_address(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| {
            let host = u.host_str().unwrap_or("");
            let host = if host.contains(':') {
                format!("[{host}]")
            } else {
                host.to_string()
            };
            u.port().map(|p| format!("{host}:{p}")).unwrap_or(host)
        })
        .unwrap_or_default()
}
pub fn prepare_directory(path: &Path, group: Option<&str>) -> Result<()> {
    let parent = path
        .parent()
        .context("Output requires a parent directory")?;
    fs::create_dir_all(parent)?;
    if let Some(group) = group {
        let group = std::ffi::CString::new(group)?;
        let gid = unsafe {
            let item = libc::getgrnam(group.as_ptr());
            if item.is_null() {
                bail!("Snapshot group unavailable");
            }
            (*item).gr_gid
        };
        let parent_c = std::ffi::CString::new(parent.as_os_str().as_encoded_bytes())?;
        if unsafe { libc::chown(parent_c.as_ptr(), u32::MAX, gid) } != 0 {
            bail!("Runtime directory group update failed");
        }
        fs::set_permissions(parent, fs::Permissions::from_mode(0o2750))?;
    }
    Ok(())
}
pub async fn run(config: Config, output: &Path, once: bool, group: Option<&str>) -> Result<()> {
    prepare_directory(output, group)?;
    let parent = output.parent().unwrap();
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(parent.join(".collector.lock"))?;
    lock.try_lock_exclusive()
        .context("Another collector is already running")?;
    let period = Duration::from_secs_f64(config.interval_s);
    let mut collector = Collector::new(config)?;
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let stop = async {
        tokio::select! {_ = tokio::signal::ctrl_c()=>{},_ = shutdown_signal()=>{}}
    };
    tokio::pin!(stop);
    loop {
        tokio::select! {_ = &mut stop=>return Ok(()),_ = interval.tick()=>{}};
        let sample = collector.sample().await?;
        config::atomic_write(output, &serde_json::to_vec(&sample)?, 0o640)?;
        if once {
            collector.initialize().await;
            tokio::time::sleep(Duration::from_secs(1)).await;
            config::atomic_write(
                output,
                &serde_json::to_vec(&collector.sample().await?)?,
                0o640,
            )?;
            return Ok(());
        }
    }
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    }
}
