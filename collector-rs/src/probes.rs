//! Native local collector: independent single-flight, expiring source caches.
use crate::{
    config::Config,
    guests::{GuestEpoch, GuestReader, GPU_METRICS},
    procfs::ProcReader,
    proxmox::ProxmoxReader,
    runtime::{bounded_snapshot, empty_snapshot},
    truenas::TrueNasReader,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    process::{Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{sync::Semaphore, task::JoinHandle};
pub fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
    .filter(|v| v.is_finite())
}
pub fn text(s: &str, max: usize) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect()
}
pub fn round(v: f64, digits: i32) -> f64 {
    let p = 10_f64.powi(digits);
    (v * p).round() / p
}
pub fn wall() -> f64 {
    crate::epoch()
}
pub fn monotonic() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}
/// Fixed executable and argv, no shell; each pipe is drained and bounded.
/// Kill/reap timed-out children; callers never accept user-supplied commands.
pub fn command(
    program: &str,
    args: &[&str],
    timeout: f64,
    allow_nonzero: bool,
) -> anyhow::Result<CommandOutput> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| anyhow::anyhow!("{program} unavailable"))?;
    let read_pipe = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let mut buf = [0u8; 8192];
            let mut exceeded = false;
            loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if data.len() + n > 4 * 1024 * 1024 {
                            exceeded = true
                        } else {
                            data.extend_from_slice(&buf[..n])
                        }
                    }
                    Err(_) => break,
                }
            }
            (data, exceeded)
        })
    };
    let out = read_pipe(Box::new(child.stdout.take().unwrap()));
    let err = read_pipe(Box::new(child.stderr.take().unwrap()));
    let deadline = Instant::now() + Duration::from_secs_f64(timeout);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20))
    };
    let (stdout, overout) = out
        .join()
        .map_err(|_| anyhow::anyhow!("output reader failed"))?;
    let (stderr, overerr) = err
        .join()
        .map_err(|_| anyhow::anyhow!("output reader failed"))?;
    if timed_out {
        anyhow::bail!("{program} probe timed out")
    };
    if overout || overerr {
        anyhow::bail!("{program} output exceeds bound")
    };
    let result = CommandOutput {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        status: status.code().unwrap_or(1),
    };
    if result.status != 0 && !allow_nonzero {
        anyhow::bail!(
            "{program} exit {}: {}",
            result.status,
            text(&result.stderr, 120)
        )
    };
    Ok(result)
}
type Job = Arc<dyn Fn() -> anyhow::Result<Value> + Send + Sync>;
struct Source {
    job: Job,
    interval: f64,
    ttl: f64,
    enabled: bool,
    handle: Option<JoinHandle<(u64, anyhow::Result<Value>)>>,
    generation: u64,
    next_run: f64,
    data: Option<Value>,
    updated_at: Option<f64>,
    updated_mono: Option<f64>,
    ok: bool,
    last_succeeded: bool,
    error: Option<String>,
}
impl Source {
    fn new(job: Job, interval: f64, ttl: f64, enabled: bool) -> Self {
        Self {
            job,
            interval,
            ttl,
            enabled,
            handle: None,
            generation: 0,
            next_run: 0.,
            data: None,
            updated_at: None,
            updated_mono: None,
            ok: false,
            last_succeeded: false,
            error: Some("initializing".into()),
        }
    }
    fn reset(&mut self) {
        self.generation += 1;
        self.data = None;
        self.updated_at = None;
        self.updated_mono = None;
        self.ok = false;
        self.last_succeeded = false;
        self.error = Some("Guest changed; waiting for OS sample".into());
        self.next_run = 0.;
    }
    fn commit(&mut self, result: (u64, anyhow::Result<Value>), mono: f64, wall: f64) {
        if result.0 != self.generation {
            return;
        }
        match result.1 {
            Ok(data) => {
                self.error = data["error"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| text(s, 180));
                self.ok = self.error.is_none();
                self.last_succeeded = true;
                self.data = Some(data);
                self.updated_at = Some(wall);
                self.updated_mono = Some(mono)
            }
            Err(e) => {
                self.ok = false;
                self.last_succeeded = false;
                self.error = Some(text(&e.to_string(), 180));
            }
        }
    }
    async fn advance(&mut self, mono: f64, wall: f64, semaphore: Arc<Semaphore>) {
        if !self.enabled {
            self.error = Some("disabled in configuration".into());
            return;
        }
        if self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            if let Ok(result) = self.handle.take().unwrap().await {
                self.commit(result, mono, wall)
            } else {
                self.ok = false;
                self.last_succeeded = false;
                self.error = Some("Source worker failed".into())
            }
        }
        if self.handle.is_none() && mono >= self.next_run {
            let job = self.job.clone();
            let generation = self.generation;
            self.handle = Some(tokio::spawn(async move {
                let permit = semaphore.acquire_owned().await;
                let result = if let Ok(_permit) = permit {
                    match tokio::task::spawn_blocking(move || job()).await {
                        Ok(r) => r,
                        Err(_) => Err(anyhow::anyhow!("Source worker failed")),
                    }
                } else {
                    Err(anyhow::anyhow!("Source queue unavailable"))
                };
                (generation, result)
            }));
            self.next_run = mono + self.interval
        }
    }
    async fn initialize(&mut self) {
        if let Some(handle) = self.handle.take() {
            if let Ok(result) = handle.await {
                self.commit(result, monotonic(), wall())
            }
        }
    }
    fn current(&self, mono: f64) -> Option<&Value> {
        if !self.enabled || !self.updated_mono.is_some_and(|t| mono - t <= self.ttl) {
            None
        } else {
            self.data.as_ref()
        }
    }
    fn status(&self, mono: f64) -> Value {
        let age = self.updated_mono.map(|t| round((mono - t).max(0.), 1));
        let expired = age.is_some_and(|a| a > self.ttl);
        json!({"ok":self.enabled&&self.ok&&!expired,"enabled":self.enabled,"updated_at":self.updated_at,"age_s":age,"error":if !self.enabled{Some("disabled in configuration")}else if expired{Some("sample expired")}else{self.error.as_deref()}})
    }
}
pub struct LocalCollector {
    proc: ProcReader,
    sources: BTreeMap<String, Source>,
    guest_epochs: BTreeMap<u64, GuestEpoch>,
    guest_states: BTreeMap<u64, (String, Option<f64>, Option<f64>)>,
    nas_reset_at: f64,
    semaphore: Arc<Semaphore>,
}
impl LocalCollector {
    pub fn new(c: &Config) -> Self {
        let mut sources = BTreeMap::new();
        let pve = Arc::new(Mutex::new(ProxmoxReader::default()));
        let nas = Arc::new(Mutex::new(TrueNasReader::default()));
        let c1 = c.clone();
        sources.insert(
            "proxmox".into(),
            Source::new(
                Arc::new(move || {
                    pve.lock()
                        .map_err(|_| anyhow::anyhow!("PVE state unavailable"))?
                        .read(&c1)
                }),
                c.proxmox_interval_s,
                (3. * c.proxmox_interval_s).max(30.),
                true,
            ),
        );
        sources.insert(
            "sensors".into(),
            Source::new(
                Arc::new(|| {
                    let r = command("sensors", &["-j"], 5., false)?;
                    let d: Value = serde_json::from_str(&r.stdout)?;
                    Ok(json!({"sensors":crate::sensors::parse_sensors(&d)}))
                }),
                c.sensor_interval_s,
                (3. * c.sensor_interval_s).max(20.),
                true,
            ),
        );
        sources.insert(
            "turbostat".into(),
            Source::new(
                Arc::new(|| {
                    let r = command(
                        "turbostat",
                        &[
                            "--quiet",
                            "--Summary",
                            "--interval",
                            "1",
                            "--num_iterations",
                            "1",
                        ],
                        5.,
                        false,
                    )?;
                    Ok(json!({"power":crate::sensors::parse_turbostat(&r.stdout)?}))
                }),
                c.turbostat_interval_s,
                (3. * c.turbostat_interval_s).max(20.),
                c.enable_turbostat,
            ),
        );
        let c1 = c.clone();
        sources.insert(
            "smart".into(),
            Source::new(
                Arc::new(move || crate::storage::read_smart(&c1)),
                c.smart_interval_s,
                (3. * c.smart_interval_s).max(900.),
                c.enable_smart,
            ),
        );
        sources.insert(
            "zfs".into(),
            Source::new(
                Arc::new(crate::storage::read_zfs),
                c.zfs_interval_s,
                (3. * c.zfs_interval_s).max(90.),
                c.enable_zfs,
            ),
        );
        let c1 = c.clone();
        sources.insert(
            "nas".into(),
            Source::new(
                Arc::new(move || {
                    nas.lock()
                        .map_err(|_| anyhow::anyhow!("NAS state unavailable"))?
                        .read(&c1)
                }),
                c.truenas_interval_s,
                (3. * c.truenas_interval_s).max(90.),
                !c.truenas_ssh_host.is_empty(),
            ),
        );
        sources.insert(
            "faults".into(),
            Source::new(
                Arc::new(crate::faults::read_faults),
                c.faults_interval_s,
                (3. * c.faults_interval_s).max(90.),
                c.enable_faults,
            ),
        );
        sources.insert(
            "gpus".into(),
            Source::new(
                Arc::new(crate::gpus::read_gpus),
                c.gpu_interval_s,
                (3. * c.gpu_interval_s).max(90.),
                c.enable_gpus,
            ),
        );
        let mut guest_epochs = BTreeMap::new();
        for id in &c.qga_guest_ids {
            let epoch = GuestEpoch::default();
            let reader = Arc::new(Mutex::new(GuestReader::new(*id, epoch.clone())));
            guest_epochs.insert(*id, epoch);
            sources.insert(
                format!("guest_{id}"),
                Source::new(
                    Arc::new(move || {
                        reader
                            .lock()
                            .map_err(|_| anyhow::anyhow!("Guest state unavailable"))?
                            .read()
                    }),
                    c.guest_interval_s,
                    (3. * c.guest_interval_s).max(45.),
                    true,
                ),
            );
        }
        Self {
            proc: ProcReader::default(),
            sources,
            guest_epochs,
            guest_states: BTreeMap::new(),
            nas_reset_at: 0.,
            semaphore: Arc::new(Semaphore::new(10)),
        }
    }
    pub async fn initialize(&mut self) {
        for source in self.sources.values_mut() {
            source.initialize().await
        }
    }
    fn observe_guests(&mut self, guests: &[Value], c: &Config, wall: f64) {
        let mut present = BTreeSet::new();
        for g in guests {
            let Some(id) = g["id"].as_u64() else { continue };
            present.insert(id);
            let state = (
                g["status"].as_str().unwrap_or("unknown").to_owned(),
                num(&g["uptime_s"]),
                num(&g["mem_total_bytes"]),
            );
            if let Some(old) = self.guest_states.get(&id) {
                let rebooted = matches!((state.1,old.1),(Some(a),Some(b))if a<b);
                if old.0 != state.0 || old.2 != state.2 || rebooted {
                    if let Some(epoch) = self.guest_epochs.get(&id) {
                        epoch.reset(old.0 != state.0 || rebooted);
                        if let Some(s) = self.sources.get_mut(&format!("guest_{id}")) {
                            s.reset()
                        }
                    }
                    if id == c.truenas_guest_id {
                        self.nas_reset_at = wall
                    }
                }
            }
            self.guest_states.insert(id, state);
        }
        let removed = self
            .guest_states
            .keys()
            .filter(|id| !present.contains(id))
            .copied()
            .collect::<Vec<_>>();
        for id in removed {
            self.guest_states.remove(&id);
            if let Some(epoch) = self.guest_epochs.get(&id) {
                epoch.reset(true);
                if let Some(s) = self.sources.get_mut(&format!("guest_{id}")) {
                    s.reset()
                }
            }
            if id == c.truenas_guest_id {
                self.nas_reset_at = wall
            }
        }
    }
    pub async fn collect(&mut self, c: &Config, sequence: u64) -> Value {
        let mono = monotonic();
        let now = wall();
        let mut snapshot = empty_snapshot(&crate::procfs::hostname(), &c.host_ip, sequence, now);
        let proc = self.proc.read(c, mono);
        snapshot["host"] = proc["host"].clone();
        snapshot["sources"]["proc"] = json!({"ok":proc["error"].is_null(),"enabled":true,"updated_at":now,"age_s":0,"error":proc["error"]});
        for source in self.sources.values_mut() {
            source.advance(mono, now, self.semaphore.clone()).await;
        }
        let pve = self.sources["proxmox"].current(mono).cloned();
        if let Some(pve) = pve.as_ref() {
            if let Some(guests) = pve["guests"].as_array() {
                self.observe_guests(guests, c, now)
            }
        }
        for (name, source) in &self.sources {
            snapshot["sources"][name] = source.status(mono);
        }
        // Assembly order is semantic: local inventories form the base, then
        // ZFS and NAS append to them. Map iteration must never overwrite NAS.
        for name in [
            "proxmox",
            "sensors",
            "turbostat",
            "smart",
            "zfs",
            "nas",
            "faults",
            "gpus",
        ] {
            let source = &self.sources[name];
            let Some(data) = source.current(mono) else {
                continue;
            };
            match name {
                "proxmox" => {
                    snapshot["guests"] = data["guests"].clone();
                    snapshot["storage"] = data["storage"].clone();
                    snapshot["sources"]["proxmox_node"] = json!({"ok":data["node_error"].is_null(),"updated_at":source.updated_at,"age_s":source.status(mono)["age_s"],"error":data["node_error"]});
                    if !data["io_wait_pct"].is_null() {
                        snapshot["host"]["io_wait_pct"] = data["io_wait_pct"].clone()
                    }
                }
                "sensors" => snapshot["sensors"] = data["sensors"].clone(),
                "turbostat" => snapshot["power"] = data["power"].clone(),
                "smart" => {
                    snapshot["disks"] = data["disks"].clone();
                    snapshot["limits"]["counts"]["disks"] = data["disks_total"].clone()
                }
                "zfs" => {
                    if let Some(pools) = data["pools"].as_array() {
                        for p in pools {
                            let mut pool = p.clone();
                            pool["id"] =
                                json!(format!("zpool/{}", p["name"].as_str().unwrap_or("")));
                            snapshot["storage"].as_array_mut().unwrap().push(pool)
                        }
                    }
                }
                "nas" => {
                    snapshot["limits"]["counts"]["storage"] = json!(
                        snapshot["storage"].as_array().unwrap().len()
                            + data["storage"]
                                .as_array()
                                .map_or(0, Vec::len)
                                .max(data["counts"]["pools"].as_u64().unwrap_or(0) as usize)
                    );
                    snapshot["limits"]["counts"]["disks"] = json!(
                        snapshot["disks"]
                            .as_array()
                            .unwrap()
                            .len()
                            .max(snapshot["limits"]["counts"]["disks"].as_u64().unwrap_or(0)
                                as usize)
                            + data["disks"]
                                .as_array()
                                .map_or(0, Vec::len)
                                .max(data["counts"]["disks"].as_u64().unwrap_or(0) as usize)
                    );
                    for field in ["storage", "disks", "alerts"] {
                        if let Some(items) = data[field].as_array() {
                            snapshot[field]
                                .as_array_mut()
                                .unwrap()
                                .extend(items.clone())
                        }
                    }
                    snapshot["sources"]["nas"]["temperature_cache_interval_s"] =
                        data["temperature_cache_interval_s"].clone()
                }
                "faults" => snapshot["faults"] = data["faults"].clone(),
                _ => {}
            }
        }
        let mut guest_data = BTreeMap::new();
        let mut memory_states = BTreeMap::new();
        for id in &c.qga_guest_ids {
            let source = &self.sources[&format!("guest_{id}")];
            let mut state = source.status(mono);
            let current = source.current(mono);
            if let Some(d) = current {
                state["updated_at"] = d["generated_at"].clone();
                state["age_s"] = json!(num(&d["generated_at"]).map(|t| (now - t).max(0.)));
                if num(&state["age_s"]).is_none_or(|a| a > source.ttl) {
                    state["ok"] = json!(false);
                    state["error"] = json!("Guest OS sample expired")
                }
            }
            if state["ok"] == true {
                if let Some(d) = current {
                    guest_data.insert(*id, d.clone());
                }
            }
            snapshot["sources"][format!("guest_{id}")] = state.clone();
            memory_states.insert(*id, state);
        }
        let mut enabled = c.qga_guest_ids.iter().copied().collect::<BTreeSet<_>>();
        if !c.truenas_ssh_host.is_empty() {
            let id = c.truenas_guest_id;
            enabled.insert(id);
            let source = &self.sources["nas"];
            let data = source.current(mono);
            let mut state = source.status(mono);
            let error = if let Some(d) = data {
                if num(&d["generated_at"]).is_none_or(|t| t < self.nas_reset_at) {
                    Some("Guest changed; waiting for NAS OS sample")
                } else if num(&d["generated_at"]).is_none_or(|t| now - t > source.ttl) {
                    Some("NAS OS memory sample expired")
                } else {
                    d["memory_error"].as_str().or(if d["memory"].is_null() {
                        Some("NAS OS memory unavailable")
                    } else {
                        None
                    })
                }
            } else {
                state["error"].as_str()
            };
            let error = error.map(str::to_owned);
            state["ok"] = json!(data.is_some() && source.last_succeeded && error.is_none());
            state["error"] = json!(error);
            if state["ok"] == true {
                let d = data.unwrap();
                guest_data.insert(
                    id,
                    json!({"memory":d["memory"],"generated_at":d["generated_at"],"gpus":[]}),
                );
            }
            memory_states.insert(id, state);
        }
        let guests = snapshot["guests"].as_array().cloned().unwrap_or_default();
        snapshot["guests"] = json!(crate::guests::merge_guest_memory(
            &guests,
            &guest_data,
            &memory_states,
            &enabled,
            c.truenas_guest_id,
            &snapshot["sources"]["proxmox"],
            now
        ));
        if let Some(inv) = self.sources["gpus"].current(mono) {
            if let Some(gpus) = inv["gpus"].as_array() {
                let mut gpus = crate::gpus::merge_gpus(
                    gpus,
                    &guest_data,
                    snapshot["guests"].as_array().unwrap(),
                    &snapshot["sources"],
                    now,
                );
                if snapshot["sources"]["gpus"]["ok"] != true {
                    for g in &mut gpus {
                        for k in GPU_METRICS {
                            g[k] = Value::Null
                        }
                        g["status"] = json!("unavailable");
                        g["error"] = snapshot["sources"]["gpus"]["error"].clone()
                    }
                }
                snapshot["gpus"] = json!(gpus)
            }
        }
        if snapshot["power"]["cpu_temp_c"].is_null() {
            let re = regex::Regex::new(r"(?i)coretemp|k10temp|zenpower|cpu_thermal").unwrap();
            let max = snapshot["sensors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| {
                    s["kind"] == "temperature" && re.is_match(s["chip"].as_str().unwrap_or(""))
                })
                .filter_map(|s| num(&s["value"]))
                .reduce(f64::max);
            snapshot["power"]["cpu_temp_c"] = json!(max)
        }
        snapshot["sensors"] = json!(crate::sensors::merge_power_sensors(&snapshot, c));
        let disks = snapshot["disks"].as_array_mut().unwrap();
        for disk in disks.iter_mut() {
            if let Some((read, write)) = disk["name"]
                .as_str()
                .and_then(|n| self.proc.disk_rates.get(n))
            {
                disk["read_bps"] = json!(read);
                disk["write_bps"] = json!(write)
            }
        }
        let known = disks
            .iter()
            .filter_map(|d| d["name"].as_str().map(str::to_owned))
            .collect::<BTreeSet<_>>();
        for name in &self.proc.devices {
            if !known.contains(name) {
                let (read, write) = self
                    .proc
                    .disk_rates
                    .get(name)
                    .copied()
                    .unwrap_or((None, None));
                disks.push(json!({"name":name,"model":"","temp_c":null,"health":null,"status":"unknown","read_bps":read,"write_bps":write}))
            }
        }
        snapshot["limits"]["network_interfaces"] = json!(self.proc.interfaces);
        snapshot["limits"]["disk_devices"] = json!(self.proc.devices);
        snapshot["alerts"] = json!(generate_alerts(&snapshot, c));
        bounded_snapshot(snapshot)
    }
}
pub fn generate_alerts(s: &Value, c: &Config) -> Vec<Value> {
    let mut alerts = s["alerts"].as_array().cloned().unwrap_or_default();
    let h = &s["host"];
    let memory = num(&h["mem_used_bytes"])
        .zip(num(&h["mem_total_bytes"]).filter(|v| *v > 0.))
        .map(|(u, t)| u * 100. / t);
    limit_alert(
        &mut alerts,
        "host/memory",
        "RAM used",
        memory,
        c.memory_warning_pct,
        c.memory_critical_pct,
        "%",
    );
    limit_alert(
        &mut alerts,
        "host/iowait",
        "IO wait",
        num(&h["io_wait_pct"]),
        c.io_wait_warning_pct,
        c.io_wait_critical_pct,
        "%",
    );
    limit_alert(
        &mut alerts,
        "host/temperature",
        "CPU temperature",
        num(&s["power"]["cpu_temp_c"]),
        c.temperature_warning_c,
        c.temperature_critical_c,
        "°C",
    );
    for sensor in s["sensors"].as_array().into_iter().flatten() {
        let id = sensor["id"].as_str().unwrap_or("sensor");
        let name = sensor["name"].as_str().unwrap_or("Sensor");
        if sensor["fault"] == true {
            push(
                &mut alerts,
                format!("{id}/fault"),
                "warning",
                format!("{name} sensor fault"),
            )
        }
        if sensor["alarm"] == true {
            push(
                &mut alerts,
                format!("{id}/alarm"),
                "critical",
                format!("{name} hardware sensor alarm"),
            )
        }
        if sensor["kind"] == "temperature" {
            limit_alert(
                &mut alerts,
                id,
                name,
                num(&sensor["value"]),
                num(&sensor["high"]).unwrap_or(c.temperature_warning_c),
                num(&sensor["crit"]).unwrap_or(c.temperature_critical_c),
                "°C",
            )
        }
    }
    for d in s["disks"].as_array().into_iter().flatten() {
        let name = d["name"].as_str().unwrap_or("disk");
        if d["health"] == "failed" {
            push(
                &mut alerts,
                format!("disk/{name}/smart"),
                "critical",
                format!("{name} SMART health failed"),
            )
        } else if d["status"] == "error" {
            push(
                &mut alerts,
                format!("disk/{name}/read"),
                "warning",
                format!("{name} SMART unavailable"),
            )
        }
        limit_alert(
            &mut alerts,
            &format!("disk/{name}/temperature"),
            name,
            num(&d["temp_c"]),
            c.disk_temperature_warning_c,
            c.disk_temperature_critical_c,
            "°C",
        );
        for (i, w) in d["warnings"].as_array().into_iter().flatten().enumerate() {
            push(
                &mut alerts,
                format!("disk/{name}/warning{i}"),
                "warning",
                format!("{name}: {}", w.as_str().unwrap_or("SMART warning")),
            )
        }
        for (field, title) in [
            ("reallocated_sectors", "reallocated sectors"),
            ("pending_sectors", "pending sectors"),
        ] {
            if let Some(v) = num(&d[field]).filter(|v| *v != 0.) {
                push(
                    &mut alerts,
                    format!("disk/{name}/{field}"),
                    if field == "pending_sectors" {
                        "critical"
                    } else {
                        "warning"
                    },
                    format!("{name}: {v:.0} {title}"),
                )
            }
        }
        if ["DEGRADED", "FAULTED", "UNAVAIL", "OFFLINE", "REMOVED"]
            .contains(&d["zfs_status"].as_str().unwrap_or(""))
        {
            push(
                &mut alerts,
                format!("disk/{name}/zfs"),
                "critical",
                format!("{name} ZFS {}", d["zfs_status"].as_str().unwrap()),
            )
        }
        for field in ["read_errors", "write_errors", "checksum_errors"] {
            if let Some(v) = num(&d[field]).filter(|v| *v != 0.) {
                push(
                    &mut alerts,
                    format!("disk/{name}/{field}"),
                    "critical",
                    format!("{name}: {v:.0} ZFS {}", field.replace('_', " ")),
                )
            }
        }
    }
    for st in s["storage"].as_array().into_iter().flatten() {
        let id = st["id"].as_str().unwrap_or("storage");
        let name = st["name"].as_str().unwrap_or("Storage");
        let pct = num(&st["used_bytes"])
            .zip(num(&st["total_bytes"]).filter(|v| *v > 0.))
            .map(|(a, b)| a * 100. / b);
        limit_alert(
            &mut alerts,
            id,
            &format!("{name} used"),
            pct,
            c.storage_warning_pct,
            c.storage_critical_pct,
            "%",
        );
        if ["offline", "DEGRADED", "FAULTED", "UNAVAIL", "SUSPENDED"]
            .contains(&st["status"].as_str().unwrap_or(""))
        {
            push(
                &mut alerts,
                format!("{id}/health"),
                "critical",
                format!("{name} {}", st["status"].as_str().unwrap()),
            )
        } else if st["healthy"] == false {
            push(
                &mut alerts,
                format!("{id}/health"),
                "critical",
                format!("{name} reports unhealthy"),
            )
        }
        if num(&st["scrub_errors"]).is_some_and(|v| v != 0.) {
            push(
                &mut alerts,
                format!("{id}/scrub"),
                "critical",
                format!("{name}: scrub errors detected"),
            )
        }
    }
    for name in [
        "proc",
        "proxmox",
        "sensors",
        "smart",
        "turbostat",
        "zfs",
        "nas",
        "faults",
        "gpus",
    ] {
        let state = &s["sources"][name];
        if state.is_object() && state["enabled"] != false && state["ok"] != true {
            push(
                &mut alerts,
                format!("source/{name}"),
                "warning",
                format!("{name} telemetry unavailable"),
            )
        }
    }
    if s["sources"]["proxmox"]["ok"] == true {
        for id in &c.expected_running_guests {
            let guest = s["guests"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|g| g["id"].as_u64() == Some(*id));
            match guest {
                None => push(
                    &mut alerts,
                    format!("guest/{id}/missing"),
                    "warning",
                    format!("Expected guest {id} absent from node"),
                ),
                Some(g) if g["status"] != "running" => push(
                    &mut alerts,
                    format!("guest/{id}/stopped"),
                    "critical",
                    format!(
                        "{} is {} (expected running)",
                        g["name"].as_str().unwrap_or("Guest"),
                        g["status"].as_str().unwrap_or("unknown")
                    ),
                ),
                _ => {}
            }
        }
    }
    for g in s["guests"].as_array().into_iter().flatten() {
        if g["memory_basis"] != "proxmox" && g["status"] == "running" && !g["mem_error"].is_null() {
            push(
                &mut alerts,
                format!("guest/{}/memory", g["id"].as_u64().unwrap_or(0)),
                "warning",
                format!(
                    "{} OS memory unavailable",
                    g["name"].as_str().unwrap_or("Guest")
                ),
            )
        }
    }
    let faults = &s["faults"];
    if let Some(count) = num(&faults["segfault_count_24h"]).filter(|v| *v > 0.) {
        push(
            &mut alerts,
            "faults/segfaults".into(),
            if count >= 3. { "critical" } else { "warning" },
            format!("{count:.0} segfaults in the last 24h"),
        )
    }
    for e in faults["events"].as_array().into_iter().flatten() {
        if num(&e["timestamp"]).is_some_and(|t| t >= wall() - 86400.) && e["kind"] != "segfault" {
            push(
                &mut alerts,
                format!("faults/{}", e["id"].as_str().unwrap_or("event")),
                if ["hardware", "oom", "lockup"].contains(&e["kind"].as_str().unwrap_or("")) {
                    "critical"
                } else {
                    "warning"
                },
                e["message"].as_str().unwrap_or("Kernel fault").into(),
            )
        }
    }
    alerts.sort_by_key(|a| a["severity"] != "critical");
    alerts.truncate(24);
    alerts
}
fn push(alerts: &mut Vec<Value>, id: String, severity: &str, message: String) {
    alerts.push(json!({"id":text(&id,96),"severity":severity,"message":text(&message,120)}))
}
fn limit_alert(
    alerts: &mut Vec<Value>,
    id: &str,
    title: &str,
    value: Option<f64>,
    warning: f64,
    critical: f64,
    unit: &str,
) {
    if let Some(v) = value.filter(|v| *v >= warning || *v >= critical) {
        push(
            alerts,
            id.into(),
            if v >= critical { "critical" } else { "warning" },
            format!("{title} {v:.0}{unit}"),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_values_and_text() {
        assert_eq!(num(&json!(true)), None);
        assert_eq!(num(&json!("NaN")), None);
        assert_eq!(num(&json!("2.5")), Some(2.5));
        assert_eq!(text("é\nlong", 3), "é l")
    }
    #[test]
    fn source_expiry_and_failure() {
        let mut s = Source::new(Arc::new(|| Ok(json!({}))), 3., 10., true);
        s.commit((0, Ok(json!({"value":1}))), 10., 100.);
        assert_eq!(s.status(11.)["ok"], true);
        assert!(s.current(21.).is_none());
        assert_eq!(s.status(21.)["error"], "sample expired");
        s.commit((0, Err(anyhow::anyhow!("failed"))), 12., 102.);
        assert_eq!(s.status(12.)["ok"], false);
        s.reset();
        s.commit((0, Ok(json!({"value":2}))), 13., 103.);
        assert!(s.current(13.).is_none());
        assert_eq!(
            s.status(13.)["error"],
            "Guest changed; waiting for OS sample"
        )
    }
    #[test]
    fn guest_lifecycle_invalidation() {
        let c = Config {
            qga_guest_ids: vec![100],
            ..Config::default()
        };
        let mut collector = LocalCollector::new(&c);
        collector.observe_guests(
            &[json!({"id":100,"status":"running","uptime_s":100,"mem_total_bytes":1000})],
            &c,
            10.,
        );
        let source = collector.sources.get_mut("guest_100").unwrap();
        source.commit(
            (
                0,
                Ok(json!({"generated_at":10,"memory":{"mem_used_bytes":500}})),
            ),
            1.,
            10.,
        );
        collector.observe_guests(
            &[json!({"id":100,"status":"running","uptime_s":1,"mem_total_bytes":1000})],
            &c,
            11.,
        );
        assert!(collector.sources["guest_100"].data.is_none());
        assert_eq!(collector.guest_epochs[&100].current(), 1);
        collector.observe_guests(&[], &c, 12.);
        assert_eq!(collector.guest_epochs[&100].current(), 2)
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    #[test]
    fn command_timeout_reaps_child() {
        let began = Instant::now();
        let error = command("/bin/sleep", &["1"], 0.04, false).err().unwrap();
        assert!(error.to_string().contains("timed out"));
        assert!(began.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn nonzero_and_missing_program() {
        assert!(command("/usr/bin/false", &[], 1., false).is_err());
        assert_ne!(command("/usr/bin/false", &[], 1., true).unwrap().status, 0);
        assert!(command("/__glimdock_missing_program__", &[], 1., false).is_err());
    }
}

#[cfg(test)]
mod alert_tests {
    use super::*;
    #[test]
    fn hardware_critical_limit_below_default_warning() {
        let mut s = empty_snapshot("synthetic", "192.0.2.1", 1, 100.);
        s["sensors"] = json!([{"id":"nvme/temp1","name":"NVMe","kind":"temperature","value":78,"high":null,"crit":75,"alarm":false,"fault":false}]);
        let alerts = generate_alerts(&s, &Config::default());
        assert!(alerts
            .iter()
            .any(|a| a["id"] == "nvme/temp1" && a["severity"] == "critical"));
    }
    #[test]
    fn disabled_sources_and_current_faults_only() {
        let mut s = empty_snapshot("synthetic", "192.0.2.1", 1, wall());
        s["sources"] =
            json!({"zfs":{"enabled":false,"ok":false},"sensors":{"enabled":true,"ok":false}});
        s["faults"] = json!({"segfault_count_24h":3,"events":[{"id":"old","kind":"hardware","timestamp":wall()-90000.,"message":"Historical fault"},{"id":"now","kind":"hardware","timestamp":wall(),"message":"Current hardware fault"}]});
        let a = generate_alerts(&s, &Config::default());
        assert!(!a
            .iter()
            .any(|a| a["id"] == "source/zfs" || a["id"] == "faults/old"));
        assert!(a.iter().any(|a| a["id"] == "source/sensors"));
        assert!(a
            .iter()
            .any(|a| a["id"] == "faults/segfaults" && a["severity"] == "critical"));
        assert!(a
            .iter()
            .any(|a| a["id"] == "faults/now" && a["severity"] == "critical"));
    }
}

#[cfg(test)]
mod assembly_tests {
    use super::*;
    #[tokio::test]
    async fn nas_inventory_survives_local_inventory_assembly() {
        let folder = tempfile::tempdir().unwrap();
        let proc = folder.path().join("proc");
        let sys = folder.path().join("sys");
        std::fs::create_dir_all(proc.join("net")).unwrap();
        std::fs::create_dir_all(sys.join("block")).unwrap();
        for (file, content) in [
            (
                "stat",
                "cpu 100 0 20 80 10 0 0 0\ncpu0 100 0 20 80 10 0 0 0\n",
            ),
            ("meminfo", "MemTotal: 100 kB\nMemAvailable: 50 kB\n"),
            ("uptime", "100 0\n"),
            ("loadavg", "0.1 0.2 0.3 1/10 100\n"),
            ("diskstats", ""),
            ("net/dev", "eth0: 100 0 0 0 0 0 0 0 100 0 0 0 0 0 0 0\n"),
            (
                "net/route",
                "Iface Destination Gateway Flags\neth0 00000000 00000000 0003\n",
            ),
        ] {
            std::fs::write(proc.join(file), content).unwrap();
        }
        let c = Config {
            truenas_ssh_host: "nas.example.invalid".into(),
            enable_gpus: false,
            enable_faults: false,
            enable_turbostat: false,
            ..Config::default()
        };
        let mut collector = LocalCollector::new(&c);
        collector.proc = ProcReader::new(proc, sys);
        let now = wall();
        let mono = monotonic();
        for source in collector.sources.values_mut() {
            source.next_run = f64::INFINITY;
        }
        let local_disks=(0..2).map(|n|json!({"name":format!("nvme{n}n1"),"model":"synthetic","temp_c":35,"health":"passed","status":"active","read_bps":null,"write_bps":null})).collect::<Vec<_>>();
        let nas_disks=(0..4).map(|n|json!({"name":format!("NAS/disk{n}"),"model":"synthetic","temp_c":40,"health":null,"status":"unknown","zfs_status":"ONLINE","read_bps":null,"write_bps":null})).collect::<Vec<_>>();
        let local_storage=(0..3).map(|n|json!({"id":format!("storage/node/{n}"),"name":format!("Local {n}"),"used_bytes":100,"total_bytes":200,"status":"online"})).collect::<Vec<_>>();
        let nas_storage=(0..2).map(|n|json!({"id":format!("nas/pool/{n}"),"name":format!("NAS {n}"),"used_bytes":100,"total_bytes":200,"status":"ONLINE"})).collect::<Vec<_>>();
        for (name, data) in [
            (
                "proxmox",
                json!({"guests":[],"storage":local_storage,"node_error":null,"io_wait_pct":null}),
            ),
            (
                "smart",
                json!({"disks":local_disks,"disks_total":2,"error":null}),
            ),
            ("zfs", json!({"pools":[]})),
            (
                "nas",
                json!({"disks":nas_disks,"storage":nas_storage,"alerts":[],"memory":null,"memory_error":"Synthetic no VM","counts":{"disks":4,"pools":2},"generated_at":now,"temperature_cache_interval_s":300,"error":null}),
            ),
        ] {
            collector
                .sources
                .get_mut(name)
                .unwrap()
                .commit((0, Ok(data)), mono, now);
        }
        let snapshot = collector.collect(&c, 1).await;
        assert_eq!(snapshot["disks"].as_array().unwrap().len(), 6);
        assert_eq!(snapshot["storage"].as_array().unwrap().len(), 5);
        assert_eq!(snapshot["limits"]["counts"]["disks"], 6);
        assert_eq!(snapshot["limits"]["counts"]["storage"], 5);
        assert_eq!(snapshot["sources"]["nas"]["ok"], true);
        assert!(snapshot["disks"][2]["health"].is_null());
        assert!(snapshot["disks"][2]["name"]
            .as_str()
            .unwrap()
            .starts_with("NAS/"));
    }
}
