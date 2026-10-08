//! Fixed QGA telemetry actions, real OS memory, and DRM engine delta rates.
//! Linux payload executes only in the guest; the host collector is native Rust.
use crate::{
    probes::{command, num, text, wall},
    procfs::{arc_size, mem_values},
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
pub const GPU_METRICS: [&str; 8] = [
    "utilization_pct",
    "mem_used_bytes",
    "mem_total_bytes",
    "temp_c",
    "power_w",
    "graphics_mhz",
    "memory_mhz",
    "fan_pct",
];
pub const NVIDIA_FIELDS:&str="name,pci.bus_id,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,clocks.current.graphics,clocks.current.memory,fan.speed";
pub fn memory(
    total: Option<f64>,
    available: Option<f64>,
    arc: Option<f64>,
) -> anyhow::Result<Value> {
    let total = total
        .filter(|v| v.is_finite() && *v > 0.)
        .ok_or_else(|| anyhow::anyhow!("Guest OS total memory invalid"))?;
    let available = available
        .filter(|v| v.is_finite() && *v >= 0. && *v <= total)
        .ok_or_else(|| anyhow::anyhow!("Guest OS available memory invalid"))?;
    if arc.is_some_and(|v| !v.is_finite() || v < 0. || v > total) {
        anyhow::bail!("Guest ARC size invalid")
    };
    let used = total - available;
    Ok(
        json!({"mem_used_bytes":used,"mem_total_bytes":total,"mem_available_bytes":available,"mem_cache_bytes":arc,"mem_noncache_used_bytes":arc.map(|a|(used-a).max(0.)),"memory_basis":if arc.is_some(){"guest-os-arc"}else{"guest-os"}}),
    )
}
pub fn validate_gpu_metrics(gpu: &mut Value) {
    for k in GPU_METRICS {
        let value = num(&gpu[k]).filter(|v| {
            if k == "utilization_pct" || k == "fan_pct" {
                (0.0..=100.0).contains(v)
            } else if k == "temp_c" {
                (-50.0..=150.0).contains(v)
            } else if k == "mem_total_bytes" {
                *v > 0.
            } else {
                *v >= 0.
            }
        });
        gpu[k] = json!(value)
    }
    if matches!((num(&gpu["mem_used_bytes"]),num(&gpu["mem_total_bytes"])),(Some(a),Some(b))if a>b)
    {
        gpu["mem_used_bytes"] = Value::Null
    }
}
pub fn parse_nvidia_csv(input: &str) -> Vec<Value> {
    let mut result = vec![];
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(input.as_bytes());
    for row in reader.records().filter_map(Result::ok) {
        if row.len() != 10 {
            continue;
        };
        let mut gpu = json!({"name":text(&row[0],96),"pci_bus":row[1].to_lowercase(),"vendor":"NVIDIA","vendor_id":"10de","kind":"discrete","driver":"nvidia","status":"active","error":null});
        for (k, v) in GPU_METRICS.iter().zip(row.iter().skip(2)) {
            let n = num(&json!(v)).map(|v| {
                if *k == "mem_used_bytes" || *k == "mem_total_bytes" {
                    v * 1024. * 1024.
                } else {
                    v
                }
            });
            gpu[*k] = json!(n)
        }
        validate_gpu_metrics(&mut gpu);
        result.push(gpu)
    }
    result
}
pub fn parse_document(d: &Value, now: f64) -> anyhow::Result<Value> {
    if d["schema"] != 1 {
        anyhow::bail!("Guest telemetry schema invalid")
    };
    let ts = num(&d["generated_at"]).ok_or_else(|| anyhow::anyhow!("Guest timestamp absent"))?;
    if !(-60.0..=90.0).contains(&(now - ts)) {
        anyhow::bail!("Guest telemetry sample expired/invalid")
    };
    let mem = if let Some(s) = d["meminfo"].as_str() {
        let v = mem_values(s)?;
        memory(
            v.get("MemTotal").copied(),
            v.get("MemAvailable").copied(),
            d["arcstats"].as_str().and_then(arc_size),
        )?
    } else {
        memory(
            num(&d["memory"]["total_bytes"]),
            num(&d["memory"]["available_bytes"]),
            None,
        )?
    };
    let re = regex::Regex::new(r"(?i)VEN_([0-9A-F]{4}).*DEV_([0-9A-F]{4})").unwrap();
    let mut cards = vec![];
    for raw in d["cards"].as_array().into_iter().flatten().take(32) {
        if !raw.is_object() {
            continue;
        };
        let mut card = raw.clone();
        if let Some(pnp) = card["pnp_id"].as_str() {
            let Some(m) = re.captures(pnp) else { continue };
            let vendor = m[1].to_lowercase();
            let device = m[2].to_lowercase();
            card["vendor_id"] = json!(vendor);
            card["device_id"] = json!(device)
        }
        if ["1234", "1af4", "15ad", "1414", "1b36"]
            .contains(&card["vendor_id"].as_str().unwrap_or(""))
        {
            continue;
        };
        validate_gpu_metrics(&mut card);
        cards.push(card)
    }
    for (index, gpu) in parse_nvidia_csv(d["nvidia_csv"].as_str().unwrap_or(""))
        .into_iter()
        .enumerate()
    {
        let exact = cards.iter().position(|c| {
            c["vendor_id"] == "10de" && (c["pci_bus"] == gpu["pci_bus"] || c["name"] == gpu["name"])
        });
        let matching = exact.or_else(|| {
            cards
                .iter()
                .enumerate()
                .filter(|(_, c)| c["vendor_id"] == "10de")
                .nth(index)
                .map(|(i, _)| i)
        });
        if let Some(i) = matching {
            let driver = cards[i]["driver"].as_str().map(str::to_owned);
            cards[i]
                .as_object_mut()
                .unwrap()
                .extend(gpu.as_object().unwrap().clone());
            if let Some(driver) = driver.filter(|s| s != "nvidia") {
                cards[i]["driver"] = json!(text(&format!("nvidia {driver}"), 48))
            }
        } else {
            cards.push(gpu)
        }
    }
    cards.truncate(8);
    Ok(
        json!({"memory":mem,"gpus":cards,"generated_at":ts,"gpu_error":d["gpu_error"],"drm_stats":d["drm_stats"],"error":null}),
    )
}
#[derive(Default)]
pub struct DrmRates {
    previous: BTreeMap<(String, String), Value>,
    mono: Option<f64>,
}
impl DrmRates {
    pub fn apply(&mut self, cards: &mut Value, stats: &Value) {
        if !stats.is_object() {
            return;
        };
        let now = num(&stats["monotonic_s"]);
        let complete = stats["complete"] == true;
        let mut clients: BTreeMap<(String, String), Value> = BTreeMap::new();
        for client in stats["clients"].as_array().into_iter().flatten().take(128) {
            let Some(device) = client["device"].as_str() else {
                continue;
            };
            let id = client["client"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| client["client"].to_string());
            let key = (device.to_owned(), id);
            if let Some(old) = clients.get_mut(&key) {
                if let Some(counters) = client["counters"].as_object() {
                    for (k, v) in counters {
                        if let Some(n) = num(v) {
                            let previous = num(&old["counters"][k]).unwrap_or(0.);
                            old["counters"][k] = json!(n.max(previous))
                        }
                    }
                }
            } else {
                clients.insert(key, client.clone());
            }
        }
        let elapsed = now.zip(self.mono).map(|(a, b)| a - b);
        let mut sums: BTreeMap<(String, String), f64> = BTreeMap::new();
        let mut capacities: BTreeMap<(String, String), f64> = BTreeMap::new();
        let mut known = BTreeSet::new();
        let mut changed = BTreeSet::new();
        let mut decreased = BTreeSet::new();
        for (key, client) in clients.iter_mut() {
            let device = key.0.clone();
            let Some(previous) = self.previous.get(key) else {
                changed.insert(device);
                continue;
            };
            let counters = client["counters"].as_object().cloned().unwrap_or_default();
            for (engine, value) in counters {
                let n = num(&value);
                let old = num(&previous["counters"][&engine]);
                let (Some(n), Some(old)) = (n, old) else {
                    changed.insert(device.clone());
                    continue;
                };
                if n < old {
                    changed.insert(device.clone());
                    decreased.insert(device.clone());
                    client["counters"][&engine] = json!(old);
                    continue;
                };
                let cap = num(&client["capacities"][&engine]).unwrap_or(1.);
                if cap > 0. {
                    let identity = (device.clone(), engine);
                    *sums.entry(identity.clone()).or_default() += n - old;
                    capacities
                        .entry(identity)
                        .and_modify(|v| *v = v.max(cap))
                        .or_insert(cap);
                    known.insert(device.clone());
                }
            }
        }
        for key in self.previous.keys() {
            if !clients.contains_key(key) {
                changed.insert(key.0.clone());
            }
        }
        if let Some(cards) = cards.as_array_mut() {
            for card in cards {
                if !["i915", "xe"].contains(&card["driver"].as_str().unwrap_or("")) {
                    continue;
                };
                let dev = card["pci_bus"].as_str().unwrap_or("").to_owned();
                card["utilization_kind"] = json!("busiest-engine");
                card["utilization_pct"] = Value::Null;
                let error = if !complete {
                    Some("DRM engine counter scan incomplete")
                } else if decreased.contains(&dev) {
                    Some("DRM counters decreased; waiting for catchup")
                } else if !elapsed.is_some_and(|v| v > 0.) || !known.contains(&dev) {
                    Some("Collecting DRM engine baseline")
                } else {
                    let vals = sums
                        .iter()
                        .filter(|((d, _), _)| d == &dev)
                        .map(|(k, delta)| {
                            delta / (elapsed.unwrap() * 1e9 * capacities.get(k).unwrap_or(&1.))
                                * 100.
                        });
                    let max = vals.reduce(f64::max);
                    card["utilization_pct"] = json!(max.map(|v| v.clamp(0., 100.)));
                    if changed.contains(&dev) {
                        Some("DRM client coverage changed; partial engine load")
                    } else {
                        None
                    }
                };
                card["error"] = json!(error)
            }
        }
        if complete && now.is_some() {
            self.previous = clients;
            self.mono = now
        }
    }
}
#[derive(Clone)]
pub struct GuestEpoch {
    epoch: Arc<AtomicU64>,
    restarted: Arc<AtomicBool>,
}
impl Default for GuestEpoch {
    fn default() -> Self {
        Self {
            epoch: Arc::new(AtomicU64::new(0)),
            restarted: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl GuestEpoch {
    pub fn reset(&self, restarted: bool) {
        if restarted {
            self.restarted.store(true, Ordering::SeqCst)
        }
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }
    pub fn current(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }
}
pub struct GuestReader {
    vmid: u64,
    os: Option<&'static str>,
    pending: Option<u64>,
    drm: DrmRates,
    epoch: GuestEpoch,
    seen: u64,
}
impl GuestReader {
    pub fn new(vmid: u64, epoch: GuestEpoch) -> Self {
        Self {
            vmid,
            os: None,
            pending: None,
            drm: DrmRates::default(),
            epoch,
            seen: 0,
        }
    }
    fn check(&self, epoch: u64) -> anyhow::Result<()> {
        if self.epoch.current() != epoch {
            anyhow::bail!("Guest changed while telemetry probe was running")
        };
        Ok(())
    }
    pub fn read(&mut self) -> anyhow::Result<Value> {
        self.read_with(&mut qga_command)
    }
    pub fn read_with<F>(&mut self, runner: &mut F) -> anyhow::Result<Value>
    where
        F: FnMut(u64, &str, Option<u64>) -> anyhow::Result<Value>,
    {
        let epoch = self.epoch.current();
        if self.seen != epoch {
            self.os = None;
            self.drm = DrmRates::default();
            if self.epoch.restarted.swap(false, Ordering::SeqCst) {
                self.pending = None
            };
            self.seen = epoch
        }
        if self.pending.is_none() {
            if self.os.is_none() {
                let info = runner(self.vmid, "get-osinfo", None)?;
                self.check(epoch)?;
                self.os = Some(
                    if info["id"] == "mswindows"
                        || info["id"] == "windows"
                        || info["name"]
                            .as_str()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains("windows")
                    {
                        "windows"
                    } else {
                        "linux"
                    },
                )
            }
            self.check(epoch)?;
            let launch = runner(self.vmid, &format!("exec-{}", self.os.unwrap()), None)?;
            self.check(epoch)?;
            self.pending = Some(
                launch["pid"]
                    .as_u64()
                    .filter(|p| *p > 0 && *p <= u32::MAX as u64)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Guest telemetry launch returned no valid process ID")
                    })?,
            )
        }
        let deadline = Instant::now() + Duration::from_secs(6);
        let missing_pid =
            regex::Regex::new(r"(?i)(?:pid|process).*(?:not found|not exist)|invalid.*pid")
                .unwrap();
        let result = loop {
            self.check(epoch)?;
            let result = match runner(self.vmid, "exec-status", self.pending) {
                Ok(v) => v,
                Err(e) => {
                    if missing_pid.is_match(&e.to_string()) {
                        self.pending = None
                    };
                    return Err(e);
                }
            };
            self.check(epoch)?;
            if result["exited"] == true || result["exited"] == 1 || Instant::now() >= deadline {
                break result;
            };
            std::thread::sleep(Duration::from_millis(200))
        };
        if result["exited"] != true && result["exited"] != 1 {
            if let Some(pid) = result["pid"]
                .as_u64()
                .filter(|p| *p > 0 && *p <= u32::MAX as u64)
            {
                self.pending = Some(pid)
            }
            anyhow::bail!("Guest telemetry query still running")
        };
        self.pending = None;
        if result["exitcode"] != 0
            || result["out-truncated"] == true
            || result["out-truncated"] == 1
            || result["err-truncated"] == true
            || result["err-truncated"] == 1
        {
            self.os = None;
            anyhow::bail!("Guest telemetry query failed/truncated")
        };
        let output = result["out-data"].as_str().unwrap_or("");
        if output.len() > 64 * 1024 {
            self.os = None;
            anyhow::bail!("Guest telemetry output exceeds bound")
        };
        let parsed = serde_json::from_str(output)
            .map_err(anyhow::Error::from)
            .and_then(|d| parse_document(&d, wall()));
        match parsed {
            Ok(mut data) => {
                self.check(epoch)?;
                let stats = data["drm_stats"].clone();
                self.drm.apply(&mut data["gpus"], &stats);
                Ok(data)
            }
            Err(e) => {
                self.os = None;
                Err(e)
            }
        }
    }
}
pub fn qga_command(vmid: u64, action: &str, pid: Option<u64>) -> anyhow::Result<Value> {
    if !(100..=999999999).contains(&vmid)
        || !["get-osinfo", "exec-windows", "exec-linux", "exec-status"].contains(&action)
    {
        anyhow::bail!("invalid fixed telemetry action")
    };
    if (action == "exec-status") != pid.is_some()
        || pid.is_some_and(|p| p == 0 || p > u32::MAX as u64)
    {
        anyhow::bail!("invalid telemetry process ID")
    };
    let vmid = vmid.to_string();
    let pid = pid.map(|p| p.to_string());
    let mut args = vec![
        "-e",
        include_str!("../helpers/qga-bridge.pl"),
        &vmid,
        action,
    ];
    if let Some(pid) = pid.as_ref() {
        args.push(pid)
    }
    let r = command("perl", &args, 20., false)?;
    if r.stdout.len() > 3 * 64 * 1024 {
        anyhow::bail!("Guest telemetry response exceeds bound")
    };
    let d: Value = serde_json::from_str(&r.stdout)?;
    if !d.is_object() {
        anyhow::bail!("Guest telemetry response not object")
    };
    Ok(d)
}
pub fn merge_guest_memory(
    guests: &[Value],
    data: &BTreeMap<u64, Value>,
    states: &BTreeMap<u64, Value>,
    enabled: &BTreeSet<u64>,
    nas: u64,
    pve: &Value,
    now: f64,
) -> Vec<Value> {
    guests
        .iter()
        .map(|original| {
            let mut g = original.clone();
            let id = g["id"].as_u64().unwrap_or(0);
            for (k, v) in [
                ("pve_mem_used_bytes", original["mem_used_bytes"].clone()),
                ("pve_mem_total_bytes", original["mem_total_bytes"].clone()),
                ("mem_assigned_bytes", original["mem_assigned_bytes"].clone()),
                ("mem_host_bytes", original["mem_host_bytes"].clone()),
                ("memory_basis", json!("proxmox")),
                ("mem_available_bytes", Value::Null),
                ("mem_cache_bytes", Value::Null),
                ("mem_noncache_used_bytes", Value::Null),
                ("mem_updated_at", pve["updated_at"].clone()),
                ("mem_age_s", pve["age_s"].clone()),
                ("mem_error", pve["error"].clone()),
            ] {
                g[k] = v
            }
            if enabled.contains(&id) {
                g["memory_basis"] = json!(if id == nas {
                    "guest-os-arc"
                } else {
                    "guest-os"
                });
                g["mem_used_bytes"] = Value::Null;
                g["mem_total_bytes"] = Value::Null;
                let state = states.get(&id).unwrap_or(&Value::Null);
                let mut sample = data.get(&id);
                let mut error = state["error"]
                    .as_str()
                    .unwrap_or("Guest OS memory unavailable")
                    .to_owned();
                if g["status"] != "running" {
                    error = format!("Guest is {}", g["status"].as_str().unwrap_or("unknown"));
                    sample = None
                } else if pve["ok"] != true {
                    error = "Proxmox guest state unavailable".into();
                    sample = None
                } else if state["ok"] != true {
                    sample = None
                };
                let ts = sample
                    .and_then(|s| num(&s["generated_at"]))
                    .or_else(|| num(&state["updated_at"]));
                g["mem_updated_at"] = json!(ts);
                g["mem_age_s"] = json!(ts.map(|t| (now - t).max(0.)));
                if let Some(sample) = sample {
                    if let Some(mem) = sample["memory"].as_object() {
                        g.as_object_mut().unwrap().extend(mem.clone())
                    }
                    g["mem_error"] = Value::Null
                } else {
                    g["mem_error"] = json!(text(&error, 120))
                }
            }
            g
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn os_memory_and_arc() {
        let m = memory(Some(1000.), Some(400.), Some(100.)).unwrap();
        assert_eq!(m["mem_used_bytes"], 600.);
        assert_eq!(m["mem_noncache_used_bytes"], 500.);
        assert!(memory(Some(10.), Some(11.), None).is_err())
    }
    #[test]
    fn csv_units_and_unknown() {
        let a = parse_nvidia_csv("RTX 3070,00000000:01:00.0,20,1024,8192,45,70,1500,7000,[N/A]\n");
        assert_eq!(a[0]["mem_used_bytes"], 1073741824.);
        assert!(a[0]["fan_pct"].is_null())
    }
    #[test]
    fn enabled_probe_never_falls_back() {
        let g = json!({"id":100,"status":"running","mem_used_bytes":100,"mem_total_bytes":100,"mem_assigned_bytes":100});
        let r = merge_guest_memory(
            &[g],
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeSet::from([100]),
            101,
            &json!({"ok":true}),
            100.,
        );
        assert!(r[0]["mem_used_bytes"].is_null());
        assert_eq!(r[0]["pve_mem_used_bytes"], 100);
        assert_eq!(r[0]["memory_basis"], "guest-os")
    }
    #[test]
    fn drm_baseline_delta_reset() {
        let mut r = DrmRates::default();
        let mut cards = json!([{"driver":"i915","pci_bus":"0000:00:02.0"}]);
        let stat = |mono, counter, complete| json!({"monotonic_s":mono,"complete":complete,"clients":[{"device":"0000:00:02.0","client":"1","counters":{"render":counter},"capacities":{"render":1}}]});
        r.apply(&mut cards, &stat(1., 1000000000., true));
        assert!(cards[0]["utilization_pct"].is_null());
        r.apply(&mut cards, &stat(2., 1250000000., true));
        assert_eq!(cards[0]["utilization_pct"], 25.);
        r.apply(&mut cards, &stat(3., 1200000000., true));
        assert!(cards[0]["utilization_pct"].is_null());
        r.apply(&mut cards, &stat(4., 1500000000., false));
        assert!(cards[0]["utilization_pct"].is_null());
        assert_eq!(cards[0]["error"], "DRM engine counter scan incomplete")
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn qga_pending_pid_survives_transport_timeout() {
        let epoch = GuestEpoch::default();
        let mut reader = GuestReader::new(100, epoch.clone());
        let mut actions = Vec::<String>::new();
        let mut status_calls = 0;
        let document = json!({"schema":1,"generated_at":wall(),"memory":{"total_bytes":1000,"available_bytes":400},"cards":[]});
        let mut runner = |_id: u64, action: &str, pid: Option<u64>| -> anyhow::Result<Value> {
            actions.push(action.into());
            match action {
                "get-osinfo" => Ok(json!({"id":"mswindows"})),
                "exec-windows" => Ok(json!({"pid":123})),
                "exec-status" => {
                    assert_eq!(pid, Some(123));
                    status_calls += 1;
                    if status_calls == 1 {
                        anyhow::bail!("transport timeout")
                    };
                    Ok(json!({"exited":1,"exitcode":0,"out-data":document.to_string()}))
                }
                _ => panic!("unexpected action"),
            }
        };
        assert!(reader.read_with(&mut runner).is_err());
        assert_eq!(reader.pending, Some(123));
        let result = reader.read_with(&mut runner).unwrap();
        assert_eq!(result["memory"]["mem_used_bytes"], 600.);
        assert_eq!(reader.pending, None);
        assert_eq!(
            actions,
            ["get-osinfo", "exec-windows", "exec-status", "exec-status"]
        );
    }
    #[test]
    fn reset_discards_previous_boot_pid() {
        let epoch = GuestEpoch::default();
        let mut reader = GuestReader::new(100, epoch.clone());
        reader.pending = Some(99);
        reader.os = Some("windows");
        epoch.reset(true);
        let mut calls = Vec::new();
        let mut runner = |_id: u64, action: &str, _pid: Option<u64>| -> anyhow::Result<Value> {
            calls.push(action.to_string());
            if action == "get-osinfo" {
                Ok(json!({"id":"linux"}))
            } else {
                anyhow::bail!("intentional unavailable")
            }
        };
        assert!(reader.read_with(&mut runner).is_err());
        assert_eq!(calls, ["get-osinfo", "exec-linux"]);
        assert!(reader.pending.is_none());
    }
    #[test]
    fn document_clock_and_memory_validation() {
        assert!(parse_document(&json!({"schema":1,"generated_at":10,"memory":{"total_bytes":100,"available_bytes":50}}),200.).is_err());
        assert!(parse_document(&json!({"schema":1,"generated_at":100,"memory":{"total_bytes":100,"available_bytes":101}}),100.).is_err());
        assert!(parse_document(&json!({"schema":1,"generated_at":100,"meminfo":"MemTotal: 10 kB\nMemAvailable: 5 kB\n","arcstats":"size 4 1024"}),100.).is_ok());
    }
    #[test]
    fn invalid_gpu_metrics_null() {
        let mut g = json!({"utilization_pct":200,"fan_pct":-1,"temp_c":200,"mem_used_bytes":30,"mem_total_bytes":20,"power_w":"NaN","graphics_mhz":-1,"memory_mhz":1});
        validate_gpu_metrics(&mut g);
        for key in [
            "utilization_pct",
            "fan_pct",
            "temp_c",
            "mem_used_bytes",
            "power_w",
            "graphics_mhz",
        ] {
            assert!(g[key].is_null(), "{key}");
        }
        assert_eq!(g["memory_mhz"], 1.);
    }
}
