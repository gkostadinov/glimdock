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
use tokio::{
    sync::{oneshot, watch, Mutex},
    task::JoinHandle,
};

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
    // Portable agents already bound these nested inventories. Recompute their
    // omissions from the original count and retained rows rather than copying
    // arbitrary upstream truncation entries or adding the same omissions twice.
    for (parent, field, count_key, cap) in [
        ("host", "cpu_cores", "cpu_cores", 64),
        ("platform", "interfaces", "network_interfaces", 16),
    ] {
        let actual = snapshot[parent][field].as_array().map_or(0, Vec::len);
        let reported = snapshot["limits"]["counts"][count_key].as_u64();
        if snapshot[parent][field].is_array() || reported.is_some() {
            let count = reported.unwrap_or(actual as u64).max(actual as u64);
            if let Some(items) = snapshot
                .get_mut(parent)
                .and_then(|object| object.get_mut(field))
                .and_then(Value::as_array_mut)
            {
                items.truncate(cap);
            }
            snapshot["limits"]["counts"][count_key] = json!(count);
            snapshot["limits"]["truncated"][count_key] =
                json!(count.saturating_sub(actual.min(cap) as u64));
            snapshot["limits"][format!("max_{count_key}")] = json!(cap);
        }
    }
    if let Some(names) = snapshot["limits"]
        .get_mut("network_interfaces")
        .and_then(Value::as_array_mut)
    {
        names.truncate(16);
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
        if !alerts
            .iter()
            .any(|a| a["id"] == "capacity" || a["id"] == "display/truncated")
        {
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
impl Drop for Source {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
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
enum LocalCollector {
    Linux(Box<crate::probes::LocalCollector>),
    Portable(Box<glimdock_agent::host::HostCollector>),
}
impl LocalCollector {
    fn new(config: &Config) -> Result<Self> {
        if config.local_type == "proxmox" && !cfg!(target_os = "linux") {
            anyhow::bail!("Local Proxmox collection requires a Linux Proxmox host; configure it as a remote feed on this hub");
        }
        if cfg!(target_os = "linux") {
            Ok(Self::Linux(Box::new(crate::probes::LocalCollector::new(
                config,
            ))))
        } else {
            let native = config.native_name();
            let collector =
                glimdock_agent::host::HostCollector::new(glimdock_agent::host::HostConfig {
                    name: if config.display_name.is_empty() {
                        native
                    } else {
                        config.display_name.clone()
                    },
                    address: config.host_ip.clone(),
                    interval_s: config.interval_s.min(300.),
                    network_interfaces: config.network_interfaces.clone(),
                    ..Default::default()
                })?;
            Ok(Self::Portable(Box::new(collector)))
        }
    }
    async fn collect(&mut self, config: &Config, sequence: u64) -> Value {
        match self {
            Self::Linux(collector) => collector.collect(config, sequence).await,
            Self::Portable(collector) => {
                use glimdock_agent::Collector as _;
                let now = crate::epoch();
                match collector.sample(now, crate::probes::monotonic()) {
                    Ok(mut snapshot) => {
                        snapshot["sequence"] = json!(sequence);
                        snapshot["node"]["id"] = json!(config.local_node_id());
                        snapshot
                    }
                    Err(_) => {
                        let mut snapshot =
                            empty_snapshot(&config.native_name(), &config.host_ip, sequence, now);
                        snapshot["platform"] = json!({"os":config::native_platform(),"architecture":std::env::consts::ARCH});
                        snapshot["sources"]["host"] = json!({"enabled":true,"ok":false,"updated_at":null,"age_s":null,"error":"Native host collection unavailable"});
                        snapshot
                    }
                }
            }
        }
    }
    async fn initialize(&mut self) {
        if let Self::Linux(collector) = self {
            collector.initialize().await;
        }
    }
}
pub struct Collector {
    config: Config,
    local: Option<LocalCollector>,
    printers: Vec<Source>,
    remotes: Vec<Source>,
    sequence: u64,
}
impl Collector {
    pub fn new(config: Config) -> Result<Self> {
        let local = if config.enable_local {
            Some(LocalCollector::new(&config)?)
        } else {
            None
        };
        let printers = config
            .printers
            .iter()
            .filter(|c| c.enabled)
            .cloned()
            .map(|c| Source::new(c, false))
            .collect::<Result<Vec<_>>>()?;
        let remotes = config
            .remote_collectors
            .iter()
            .filter(|c| c.enabled)
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
            nodes.push(json!({"id":self.config.local_node_id(),"type":self.config.local_type,"platform":self.config.local_platform(),"name":if self.config.display_name.is_empty(){native}else{self.config.display_name.clone()},"address":self.config.host_ip,"snapshot":bounded_snapshot(snapshot)}));
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
            let platform = if source.config.platform.is_empty() {
                source
                    .data
                    .as_ref()
                    .and_then(|d| d["node_platform"].as_str())
                    .unwrap_or("")
            } else {
                &source.config.platform
            };
            nodes.push(json!({"id":format!("remote:{}",source.config.id),"type":source.config.node_type,"platform":platform,"name":source.config.name,"address":source_address(&source.config.url),"snapshot":snapshot}));
        }
        for agent in self.config.push_agents.iter().filter(|a| a.enabled) {
            nodes.push(crate::push::node_snapshot(
                agent,
                self.config.source_path.as_deref(),
                self.sequence,
                now,
            ));
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
pub async fn run(
    config: Config,
    output: &Path,
    once: bool,
    group: Option<&str>,
    reload_path: Option<&Path>,
) -> Result<()> {
    let (stop, shutdown) = watch::channel(false);
    let work = run_controlled(config, output, once, group, reload_path, shutdown, None);
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => result,
        _ = shutdown_signal() => {
            let _ = stop.send(true);
            work.await
        }
    }
}

/// The integrated hub owns shutdown and waits until the first publication exists.
pub async fn run_controlled(
    config: Config,
    output: &Path,
    once: bool,
    group: Option<&str>,
    reload_path: Option<&Path>,
    mut shutdown: watch::Receiver<bool>,
    mut ready: Option<oneshot::Sender<()>>,
) -> Result<()> {
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
    let mut reload_hash = reload_path
        .and_then(|path| config::read_bounded(path, 64 * 1024).ok())
        .map(|bytes| config::sha256(&bytes));
    let mut collector = Collector::new(config)?;
    let mut interval = tokio::time::interval(period);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {_ = wait_for_shutdown(&mut shutdown)=>return Ok(()),_ = interval.tick()=>{}};
        if let Some(path) = reload_path {
            if let Ok(bytes) = config::read_bounded(path, 64 * 1024) {
                let hash = config::sha256(&bytes);
                if reload_hash.as_ref() != Some(&hash) {
                    reload_hash = Some(hash);
                    match config::strict_json(&bytes)
                        .and_then(Config::from_value)
                        .and_then(Collector::new)
                    {
                        Ok(mut replacement) => {
                            replacement.config.source_path = Some(path.to_path_buf());
                            replacement.sequence = collector.sequence;
                            interval = tokio::time::interval(Duration::from_secs_f64(
                                replacement.config.interval_s,
                            ));
                            interval
                                .set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                            collector = replacement;
                        }
                        Err(_) => {
                            eprintln!("Configuration reload rejected; previous inventory retained")
                        }
                    }
                }
            }
        }
        let sample = collector.sample().await?;
        config::atomic_write(output, &serde_json::to_vec(&sample)?, 0o640)?;
        if let Some(ready) = ready.take() {
            let _ = ready.send(());
        }
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
pub async fn wait_for_shutdown(shutdown: &mut watch::Receiver<bool>) {
    while !*shutdown.borrow_and_update() {
        if shutdown.changed().await.is_err() {
            break;
        }
    }
}
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = signal.recv() => {},
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
