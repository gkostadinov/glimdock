//! Portable native host collection. Optional OS readings remain null when unavailable.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use sysinfo::{Components, CpuRefreshKind, DiskRefreshKind, Disks, Networks, System};

use crate::config::{validate_identity, validate_platform};
use crate::protocol::{empty_snapshot, finish_snapshot, source_status, text, Rates};
use crate::Collector;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HostConfig {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub address: String,
    pub interval_s: f64,
    pub network_interfaces: Vec<String>,
    pub storage_mounts: Vec<String>,
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            platform: "auto".into(),
            address: String::new(),
            interval_s: 3.0,
            network_interfaces: Vec::new(),
            storage_mounts: Vec::new(),
        }
    }
}

impl HostConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.id.is_empty() {
            validate_identity(&self.id)?;
        }
        if self.platform != "auto" {
            validate_platform(&self.platform)?;
        }
        for (label, value, cap) in [("name", &self.name, 96), ("address", &self.address, 255)] {
            ensure!(valid_text(value, cap, true), "Invalid host {label}");
        }
        ensure!(
            self.interval_s.is_finite() && (1.0..=300.0).contains(&self.interval_s),
            "interval_s must be between 1 and 300 seconds"
        );
        for (label, names) in [
            ("network_interfaces", &self.network_interfaces),
            ("storage_mounts", &self.storage_mounts),
        ] {
            ensure!(
                names.len() <= 16 && names.iter().all(|name| valid_text(name, 255, false)),
                "{label} must contain at most 16 valid names"
            );
        }
        Ok(())
    }
}

fn valid_text(value: &str, cap: usize, empty_allowed: bool) -> bool {
    (empty_allowed || !value.is_empty())
        && value.chars().count() <= cap
        && !value.chars().any(|c| c < ' ')
}

pub fn platform_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        _ => "other",
    }
}

fn default_id(name: &str) -> String {
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
    if !slug.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        slug.insert_str(0, "host_");
    }
    if slug.len() > 32 {
        let digest = format!("{:x}", Sha256::digest(name.as_bytes()));
        slug.truncate(23);
        slug.push('-');
        slug.push_str(&digest[..8]);
    }
    slug
}

pub struct HostCollector {
    pub config: HostConfig,
    descriptor: Value,
    sequence: u64,
    system: System,
    networks: Networks,
    disks: Disks,
    components: Components,
    rates: Rates,
    previous_mono: Option<f64>,
    previous_network_names: HashSet<String>,
    previous_disk_names: Vec<String>,
    #[cfg(target_os = "linux")]
    previous_cpu_times: Option<CpuTimes>,
}

impl HostCollector {
    pub fn new(mut config: HostConfig) -> Result<Self> {
        config.validate()?;
        deduplicate(&mut config.network_interfaces);
        deduplicate(&mut config.storage_mounts);
        let hostname = System::host_name().unwrap_or_else(|| "Host".into());
        let name = if config.name.is_empty() {
            &hostname
        } else {
            &config.name
        };
        let platform = if config.platform == "auto" {
            platform_name()
        } else {
            &config.platform
        };
        let descriptor = json!({
            "id": format!("server:{}", if config.id.is_empty() { default_id(name) } else { config.id.clone() }),
            "type": "server", "platform": platform,
            "name": text(name, 96), "address": config.address,
        });
        Ok(Self {
            config,
            descriptor,
            sequence: 0,
            system: System::new(),
            networks: Networks::new(),
            disks: Disks::new(),
            components: Components::new(),
            rates: Rates::default(),
            previous_mono: None,
            previous_network_names: HashSet::new(),
            previous_disk_names: Vec::new(),
            #[cfg(target_os = "linux")]
            previous_cpu_times: None,
        })
    }

    fn network_rows(&mut self, mono: f64, snapshot: &mut Value) -> Vec<Value> {
        self.networks.refresh(true);
        let names: Vec<String> = if self.config.network_interfaces.is_empty() {
            let mut names: Vec<_> = self
                .networks
                .keys()
                .filter(|name| !is_loopback(name))
                .cloned()
                .collect();
            names.sort();
            names
        } else {
            self.config.network_interfaces.clone()
        };
        let current_names: HashSet<_> = names.iter().take(16).cloned().collect();
        for disappeared in self.previous_network_names.difference(&current_names) {
            self.rates.clear_prefix(&format!("net/rx/{disappeared}"));
            self.rates.clear_prefix(&format!("net/tx/{disappeared}"));
        }
        self.previous_network_names = current_names;
        let flags = interface_flags();
        let rows: Vec<_> = names.iter().take(16).map(|name| {
            let value = self.networks.get(name);
            let rx = self.rates.rate(&format!("net/rx/{name}"), value.map(|v| v.total_received()), mono);
            let tx = self.rates.rate(&format!("net/tx/{name}"), value.map(|v| v.total_transmitted()), mono);
            let status = flags.get(name).map_or("unknown", |up| if *up { "up" } else { "down" });
            let extras = interface_extras(name);
            let addresses: Vec<_> = value.map(|v| v.ip_networks().iter().take(8).map(ToString::to_string).collect()).unwrap_or_default();
            let mac = value.map(|v| v.mac_address()).filter(|v| *v != sysinfo::MacAddr::UNSPECIFIED).map(|v| v.to_string());
            json!({
                "name": text(name, 64), "status": if value.is_some() {status} else {"unknown"},
                "speed_mbps": extras.speed_mbps, "mtu": value.map(|v| v.mtu()).filter(|v| *v > 0),
                "rx_bps": rx, "tx_bps": tx,
                "errors_in": value.map(|v| v.total_errors_on_received()),
                "errors_out": value.map(|v| v.total_errors_on_transmitted()),
                "drops_in": extras.drops_in, "drops_out": extras.drops_out,
                "mac_address": mac, "addresses": addresses,
            })
        }).collect();
        snapshot["host"]["net_rx_bps"] = aggregate(&rows, "rx_bps");
        snapshot["host"]["net_tx_bps"] = aggregate(&rows, "tx_bps");
        snapshot["limits"]["network_interfaces"] =
            json!(rows.iter().map(|v| &v["name"]).collect::<Vec<_>>());
        snapshot["limits"]["counts"]["network_interfaces"] = json!(names.len());
        snapshot["limits"]["truncated"]["network_interfaces"] =
            json!(names.len().saturating_sub(rows.len()));
        rows
    }

    fn storage_and_disk_io(&mut self, mono: f64, snapshot: &mut Value) -> Result<()> {
        // Capacity comes from filesystem_space below. Skip duplicate capacity probes,
        // including macOS purgeable-space accounting, while retaining disk metadata/I/O.
        self.disks
            .refresh_specifics(true, DiskRefreshKind::everything().without_storage());
        let mounts = if self.config.storage_mounts.is_empty() {
            let mut mounts: Vec<_> = self
                .disks
                .iter()
                .map(|d| d.mount_point().to_string_lossy().into_owned())
                .collect();
            deduplicate(&mut mounts);
            mounts
        } else {
            self.config.storage_mounts.clone()
        };
        snapshot["limits"]["counts"]["storage"] = json!(mounts.len());
        let mut rows = Vec::new();
        let mut unavailable = false;
        for mount in mounts.iter().take(16) {
            let disk = self
                .disks
                .iter()
                .find(|disk| disk.mount_point() == Path::new(mount));
            let mut row = json!({
                "id": text(&format!("filesystem/{mount}"), 96), "name": text(mount, 96),
                "used_bytes": null, "total_bytes": null, "status": "unknown",
                "filesystem": disk.map(|d| text(&d.file_system().to_string_lossy(), 64)),
                "device": disk.map(|d| text(&d.name().to_string_lossy(), 96)),
            });
            match filesystem_space(Path::new(mount)) {
                Ok((used, total)) => {
                    row["used_bytes"] = json!(used);
                    row["total_bytes"] = json!(total);
                    row["status"] = json!("online");
                }
                Err(_) => unavailable = true,
            }
            rows.push(row);
        }
        snapshot["storage"] = json!(rows);

        // A volume may have several mount points. Count each reported device only once.
        let mut seen = HashSet::new();
        let mut read = Some(0_u64);
        let mut written = Some(0_u64);
        let mut io_names = Vec::new();
        for disk in &self.disks {
            let name = disk.name().to_string_lossy().into_owned();
            if !seen.insert(name.clone()) {
                continue;
            }
            let usage = disk.usage();
            // sysinfo returns 0 when the platform cannot read these counters.
            if usage.total_read_bytes == 0 && usage.total_written_bytes == 0 {
                continue;
            }
            read = read.and_then(|v| v.checked_add(usage.total_read_bytes));
            written = written.and_then(|v| v.checked_add(usage.total_written_bytes));
            io_names.push(text(&name, 96));
        }
        if io_names.is_empty() {
            read = None;
            written = None;
        }
        // Changing the device set must not turn a newly included counter into traffic.
        io_names.sort();
        if self.previous_disk_names != io_names {
            self.rates.clear_prefix("disk/");
        }
        self.previous_disk_names = io_names.clone();
        snapshot["host"]["disk_read_bps"] = json!(self.rates.rate("disk/read", read, mono));
        snapshot["host"]["disk_write_bps"] = json!(self.rates.rate("disk/write", written, mono));
        snapshot["limits"]["disk_devices"] = json!(io_names.iter().take(16).collect::<Vec<_>>());
        snapshot["platform"]["disk_io_available"] = json!(read.is_some() && written.is_some());
        if unavailable {
            bail!("Storage unavailable");
        }
        Ok(())
    }

    fn temperatures(&mut self, now: f64, snapshot: &mut Value) {
        self.components.refresh(true);
        let mut cpu = None::<f64>;
        for (index, component) in self.components.iter().enumerate() {
            let label = component.label();
            let value = finite(component.temperature().map(f64::from));
            let critical = finite(component.critical().map(f64::from));
            let words = label.to_lowercase();
            if ["cpu", "coretemp", "k10temp", "package", "soc"]
                .iter()
                .any(|word| words.contains(word))
            {
                if let Some(value) = value {
                    cpu = Some(cpu.map_or(value, |old| old.max(value)));
                }
            }
            let row = sensor(
                &format!("temperature/{index}"),
                label,
                "component",
                "temperature",
                "°C",
                value,
                None,
                critical,
                "temperatures",
                now,
            );
            append_sensor(snapshot, row);
        }
        snapshot["power"]["cpu_temp_c"] = json!(cpu);
    }
}

impl Collector for HostCollector {
    fn descriptor(&self) -> Value {
        self.descriptor.clone()
    }
    fn interval_s(&self) -> f64 {
        self.config.interval_s
    }

    fn sample(&mut self, now: f64, mono: f64) -> Result<Value> {
        ensure!(
            now.is_finite() && mono.is_finite(),
            "Invalid collection timestamp"
        );
        self.sequence = self.sequence.saturating_add(1);
        let mut snapshot = empty_snapshot(&self.descriptor, self.sequence, now);
        let cpu_interval_valid = self
            .previous_mono
            .is_some_and(|last| mono - last >= sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.as_secs_f64());
        self.system
            .refresh_cpu_specifics(CpuRefreshKind::everything());
        self.system.refresh_memory();
        let cpus = self.system.cpus();
        let cpu_count = cpus.len();
        let frequency = {
            let frequencies: Vec<_> = cpus
                .iter()
                .map(|cpu| cpu.frequency())
                .filter(|v| *v > 0)
                .collect();
            (!frequencies.is_empty()).then(|| {
                frequencies.iter().map(|v| *v as f64).sum::<f64>() / frequencies.len() as f64
            })
        };
        snapshot["host"]["cpu_pct"] = json!(if cpu_interval_valid && !cpus.is_empty() {
            finite(Some(f64::from(self.system.global_cpu_usage()))).map(|v| v.clamp(0.0, 100.0))
        } else {
            None
        });
        snapshot["host"]["cpu_cores"] = json!(cpus
            .iter()
            .map(|cpu| if cpu_interval_valid {
                finite(Some(f64::from(cpu.cpu_usage()))).map(|v| v.clamp(0.0, 100.0))
            } else {
                None
            })
            .collect::<Vec<_>>());
        snapshot["sources"]["cpu"] = source_status(
            now,
            if cpus.is_empty() {
                Some("Probe unavailable")
            } else {
                None
            },
            true,
        );
        let total = self.system.total_memory();
        let available = self.system.available_memory();
        if total > 0 && available <= total {
            snapshot["host"]["mem_total_bytes"] = json!(total);
            snapshot["host"]["mem_used_bytes"] = json!(total - available);
            snapshot["sources"]["memory"] = source_status(now, None, true);
        } else {
            snapshot["sources"]["memory"] = source_status(now, Some("Probe unavailable"), true);
        }
        snapshot["host"]["swap_total_bytes"] = json!(self.system.total_swap());
        snapshot["host"]["swap_used_bytes"] = json!(self.system.used_swap());
        snapshot["sources"]["swap"] = snapshot["sources"]["memory"].clone();
        snapshot["host"]["uptime_s"] = json!(System::uptime());
        snapshot["sources"]["uptime"] = source_status(now, None, true);
        if cfg!(windows) {
            snapshot["sources"]["load"] = source_status(now, None, false);
        } else {
            let load = System::load_average();
            snapshot["host"]["load"] = json!([
                finite(Some(load.one)),
                finite(Some(load.five)),
                finite(Some(load.fifteen))
            ]);
            snapshot["sources"]["load"] = source_status(now, None, true);
        }
        let interfaces = self.network_rows(mono, &mut snapshot);
        let network_error = self
            .config
            .network_interfaces
            .iter()
            .any(|name| !self.networks.contains_key(name));
        snapshot["sources"]["network"] = source_status(
            now,
            if network_error {
                Some("Probe unavailable")
            } else {
                None
            },
            !interfaces.is_empty(),
        );
        let release = text(&System::kernel_version().unwrap_or_default(), 96);
        let version = text(&System::long_os_version().unwrap_or_default(), 96);
        let arch = text(&System::cpu_arch(), 96);
        snapshot["platform"] = json!({
            "os": self.descriptor["platform"], "release": release, "version": version,
            "architecture": arch, "cpu_count": cpu_count, "interfaces": interfaces,
            "battery": null, "batteries": [], "cpu_frequency_mhz": frequency,
        });
        snapshot["host"]["os"] = self.descriptor["platform"].clone();
        snapshot["host"]["os_release"] = snapshot["platform"]["release"].clone();
        snapshot["host"]["os_version"] = snapshot["platform"]["version"].clone();
        snapshot["host"]["architecture"] = snapshot["platform"]["architecture"].clone();
        snapshot["host"]["cpu_count"] = json!(cpu_count);
        snapshot["power"]["cpu_mhz"] = json!(frequency);
        snapshot["sources"]["cpu_frequency"] = source_status(now, None, frequency.is_some());
        let storage_result = self.storage_and_disk_io(mono, &mut snapshot);
        snapshot["sources"]["storage"] =
            source_status(now, storage_result.err().map(|_| "Probe unavailable"), true);
        let disk_io_available = snapshot["platform"]["disk_io_available"]
            .as_bool()
            .unwrap_or(false);
        snapshot["sources"]["disk_io"] = source_status(now, None, disk_io_available);
        self.temperatures(now, &mut snapshot);
        snapshot["sources"]["temperatures"] = source_status(now, None, !self.components.is_empty());
        let battery_result = batteries(now, &mut snapshot);
        snapshot["sources"]["battery"] = source_status(
            now,
            battery_result.err().map(|_| "Probe unavailable"),
            cfg!(any(target_os = "linux", target_os = "macos", windows)),
        );
        collect_platform_extras(self, now, &mut snapshot);
        self.previous_mono = Some(mono);
        finish_snapshot(snapshot, &self.descriptor)
    }
}

fn deduplicate(names: &mut Vec<String>) {
    let mut seen = HashSet::new();
    names.retain(|name| seen.insert(name.clone()));
}

fn is_loopback(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "lo" | "lo0" | "loopback pseudo-interface 1"
    )
}

fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite())
}

fn aggregate(rows: &[Value], key: &str) -> Value {
    let values: Option<Vec<_>> = rows.iter().map(|row| row[key].as_f64()).collect();
    match values.filter(|v| !v.is_empty()) {
        Some(values) => json!(finite(Some(values.iter().sum()))),
        None => Value::Null,
    }
}

#[allow(clippy::too_many_arguments)]
fn sensor(
    id: &str,
    name: &str,
    chip: &str,
    kind: &str,
    unit: &str,
    value: Option<f64>,
    high: Option<f64>,
    critical: Option<f64>,
    source: &str,
    now: f64,
) -> Value {
    json!({
        "id": text(id,96), "name": text(name,96), "chip": text(chip,96), "kind": kind,
        "unit":unit, "value":finite(value), "high":finite(high), "crit":finite(critical),
        "alarm":false, "fault":false, "source":source, "updated_at":now, "age_s":0,
    })
}

fn append_sensor(snapshot: &mut Value, row: Value) {
    if let Some(value) = row["value"].as_f64() {
        let severity = if row["crit"].as_f64().is_some_and(|v| value >= v) {
            Some("critical")
        } else if row["high"].as_f64().is_some_and(|v| value >= v) {
            Some("warning")
        } else {
            None
        };
        if let Some(severity) = severity {
            snapshot["alerts"].as_array_mut().unwrap().push(json!({
                "id":row["id"], "severity":severity,
                "message":text(&format!("{} above threshold", row["name"].as_str().unwrap_or("Sensor")),96),
            }));
        }
    }
    snapshot["sensors"].as_array_mut().unwrap().push(row);
}

#[cfg(not(target_os = "macos"))]
fn batteries(now: f64, snapshot: &mut Value) -> Result<()> {
    use battery::units::{
        power::watt, ratio::percent, thermodynamic_temperature::degree_celsius, time::second,
    };
    let manager = battery::Manager::new().context("Battery probe unavailable")?;
    let mut rows = Vec::new();
    let mut failed = false;
    for (index, value) in manager
        .batteries()
        .context("Battery probe unavailable")?
        .take(16)
        .enumerate()
    {
        let Ok(battery) = value else {
            failed = true;
            continue;
        };
        let charge = finite(Some(f64::from(battery.state_of_charge().get::<percent>())));
        let seconds = finite(
            battery
                .time_to_empty()
                .map(|v| f64::from(v.get::<second>())),
        )
        .filter(|v| *v >= 0.0);
        let plugged = match battery.state() {
            battery::State::Charging | battery::State::Full => Some(true),
            battery::State::Discharging | battery::State::Empty => Some(false),
            _ => None,
        };
        rows.push(json!({
            "percent":charge, "seconds_left":seconds, "power_plugged":plugged,
            "state":battery.state().to_string(), "cycles":battery.cycle_count(),
            "health_pct":finite(Some(f64::from(battery.state_of_health().get::<percent>()))),
            "power_w":finite(Some(f64::from(battery.energy_rate().get::<watt>()))),
            "model":battery.model().map(|v| text(v,96)),
        }));
        append_sensor(
            snapshot,
            sensor(
                &format!("battery/{index}/charge"),
                "Battery charge",
                "battery",
                "battery",
                "%",
                charge,
                None,
                None,
                "battery",
                now,
            ),
        );
        if let Some(temperature) = battery.temperature() {
            append_sensor(
                snapshot,
                sensor(
                    &format!("battery/{index}/temperature"),
                    "Battery temperature",
                    "battery",
                    "temperature",
                    "°C",
                    finite(Some(f64::from(temperature.get::<degree_celsius>()))),
                    None,
                    None,
                    "battery",
                    now,
                ),
            );
        }
    }
    snapshot["platform"]["battery"] = rows.first().cloned().unwrap_or(Value::Null);
    snapshot["platform"]["batteries"] = json!(rows);
    if failed {
        bail!("Battery probe unavailable");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn batteries(now: f64, snapshot: &mut Value) -> Result<()> {
    let rows = macos_power_sources::read()?;
    for (index, row) in rows.iter().enumerate() {
        append_sensor(
            snapshot,
            sensor(
                &format!("battery/{index}/charge"),
                "Battery charge",
                "battery",
                "battery",
                "%",
                row["percent"].as_f64(),
                None,
                None,
                "battery",
                now,
            ),
        );
    }
    snapshot["platform"]["battery"] = rows.first().cloned().unwrap_or(Value::Null);
    snapshot["platform"]["batteries"] = json!(rows);
    Ok(())
}

// IOPowerSources works without SMC access, root privileges, or private battery properties.
// The public key definitions and ownership rules are in IOPSKeys.h / IOPowerSources.h.
#[cfg(target_os = "macos")]
mod macos_power_sources {
    use super::*;
    use std::ffi::{c_char, c_void, CStr, CString};
    type Ref = *const c_void;
    const UTF8: u32 = 0x0800_0100;
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> Ref;
        fn IOPSCopyPowerSourcesList(info: Ref) -> Ref;
        fn IOPSGetPowerSourceDescription(info: Ref, source: Ref) -> Ref;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: Ref);
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFArrayGetCount(array: Ref) -> isize;
        fn CFArrayGetValueAtIndex(array: Ref, index: isize) -> Ref;
        fn CFDictionaryGetValue(dictionary: Ref, key: Ref) -> Ref;
        fn CFStringCreateWithCString(allocator: Ref, string: *const c_char, encoding: u32) -> Ref;
        fn CFStringGetTypeID() -> usize;
        fn CFStringGetCString(string: Ref, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
        fn CFNumberGetTypeID() -> usize;
        fn CFNumberGetValue(number: Ref, kind: isize, value: *mut c_void) -> u8;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(value: Ref) -> u8;
    }
    struct Owned(Ref);
    impl Owned {
        fn new(value: Ref) -> Result<Self> {
            ensure!(!value.is_null(), "Battery probe unavailable");
            Ok(Self(value))
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: Owned only wraps non-null CF objects returned by Copy/Create calls.
            unsafe { CFRelease(self.0) };
        }
    }

    unsafe fn get(dictionary: Ref, key: &str) -> Ref {
        let name = CString::new(key).expect("fixed IOPS key");
        let Ok(key) = Owned::new(CFStringCreateWithCString(
            std::ptr::null(),
            name.as_ptr(),
            UTF8,
        )) else {
            return std::ptr::null();
        };
        CFDictionaryGetValue(dictionary, key.0)
    }
    unsafe fn number(dictionary: Ref, key: &str) -> Option<f64> {
        let value = get(dictionary, key);
        if value.is_null() || CFGetTypeID(value) != CFNumberGetTypeID() {
            return None;
        }
        let mut result = 0.0_f64;
        // kCFNumberDoubleType = 13, with a writable f64 destination.
        (CFNumberGetValue(value, 13, &mut result as *mut f64 as *mut c_void) != 0)
            .then_some(result)
            .filter(|v| v.is_finite())
    }
    unsafe fn boolean(dictionary: Ref, key: &str) -> Option<bool> {
        let value = get(dictionary, key);
        if value.is_null() || CFGetTypeID(value) != CFBooleanGetTypeID() {
            return None;
        }
        Some(CFBooleanGetValue(value) != 0)
    }
    unsafe fn string(dictionary: Ref, key: &str) -> Option<String> {
        let value = get(dictionary, key);
        if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() {
            return None;
        }
        let mut buffer = [0 as c_char; 512];
        if CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) == 0 {
            return None;
        }
        Some(
            CStr::from_ptr(buffer.as_ptr())
                .to_string_lossy()
                .into_owned(),
        )
    }

    fn charge(current: Option<f64>, maximum: Option<f64>) -> Option<f64> {
        let (current, maximum) = (current?, maximum?);
        (current >= 0.0 && maximum > 0.0 && current <= maximum).then(|| 100.0 * current / maximum)
    }
    fn remaining(
        minutes: Option<f64>,
        plugged: Option<bool>,
        charging: Option<bool>,
    ) -> Option<f64> {
        if plugged != Some(false) || charging == Some(true) {
            return None;
        }
        minutes.filter(|v| *v >= 0.0).map(|v| v * 60.0)
    }

    pub(super) fn read() -> Result<Vec<Value>> {
        let mut rows = Vec::new();
        // SAFETY: each Copy result is owned and released by its guard. Borrowed array
        // and dictionary entries are read only while the owning info/list guards live;
        // all dictionary values are type-checked before conversion.
        unsafe {
            let info = Owned::new(IOPSCopyPowerSourcesInfo())?;
            let sources = Owned::new(IOPSCopyPowerSourcesList(info.0))?;
            for index in 0..CFArrayGetCount(sources.0).min(16) {
                let source = CFArrayGetValueAtIndex(sources.0, index);
                let dictionary = IOPSGetPowerSourceDescription(info.0, source);
                if dictionary.is_null() || boolean(dictionary, "Is Present") == Some(false) {
                    continue;
                }
                let percent = charge(
                    number(dictionary, "Current Capacity"),
                    number(dictionary, "Max Capacity"),
                );
                let plugged = match string(dictionary, "Power Source State").as_deref() {
                    Some("AC Power") => Some(true),
                    Some("Battery Power") => Some(false),
                    _ => None,
                };
                let charging = boolean(dictionary, "Is Charging");
                let state = match (plugged, charging, percent) {
                    (_, Some(true), _) => "charging",
                    (Some(false), _, _) => "discharging",
                    (Some(true), _, Some(v)) if v >= 100.0 => "full",
                    _ => "unknown",
                };
                rows.push(json!({
                    "percent":percent,"power_plugged":plugged,
                    "seconds_left":remaining(number(dictionary,"Time to Empty"),plugged,charging),
                    "state":state,"model":string(dictionary,"Name").map(|v| text(&v,96)),
                    "cycles":null,"health_pct":null,"power_w":null,
                }));
            }
        }
        Ok(rows)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn capacity_and_minutes_are_validated_and_converted() {
            assert_eq!(charge(Some(75.0), Some(100.0)), Some(75.0));
            assert_eq!(charge(Some(600.0), Some(1200.0)), Some(50.0));
            assert_eq!(charge(Some(1.0), Some(0.0)), None);
            assert_eq!(charge(Some(101.0), Some(100.0)), None);
            assert_eq!(
                remaining(Some(120.0), Some(false), Some(false)),
                Some(7200.0)
            );
            assert_eq!(remaining(Some(-1.0), Some(false), Some(false)), None);
            assert_eq!(remaining(Some(120.0), Some(true), Some(false)), None);
            assert_eq!(remaining(Some(120.0), Some(false), Some(true)), None);
        }
    }
}

#[derive(Default)]
struct InterfaceExtras {
    speed_mbps: Option<u64>,
    drops_in: Option<u64>,
    drops_out: Option<u64>,
}

#[cfg(target_os = "linux")]
fn interface_extras(name: &str) -> InterfaceExtras {
    // Only kernel-enumerated names are read; configured names containing path separators are ignored.
    if name.contains('/') || name == "." || name == ".." {
        return InterfaceExtras::default();
    }
    let directory = Path::new("/sys/class/net").join(name);
    InterfaceExtras {
        speed_mbps: read_u64(&directory.join("speed")).filter(|v| *v > 0),
        drops_in: read_u64(&directory.join("statistics/rx_dropped")),
        drops_out: read_u64(&directory.join("statistics/tx_dropped")),
    }
}

#[cfg(not(target_os = "linux"))]
fn interface_extras(_name: &str) -> InterfaceExtras {
    InterfaceExtras::default()
}

#[cfg(unix)]
fn interface_flags() -> BTreeMap<String, bool> {
    use std::ffi::CStr;
    let mut result = BTreeMap::new();
    let mut first = std::ptr::null_mut::<libc::ifaddrs>();
    // SAFETY: getifaddrs initializes an owned list, which is freed after traversal.
    unsafe {
        if libc::getifaddrs(&mut first) != 0 {
            return result;
        }
        let mut current = first;
        while !current.is_null() {
            let row = &*current;
            if !row.ifa_name.is_null() {
                let name = CStr::from_ptr(row.ifa_name).to_string_lossy().into_owned();
                let ready = (libc::IFF_UP | libc::IFF_RUNNING) as u32;
                result.insert(name, row.ifa_flags & ready == ready);
            }
            current = row.ifa_next;
        }
        libc::freeifaddrs(first);
    }
    result
}

#[cfg(not(unix))]
fn interface_flags() -> BTreeMap<String, bool> {
    BTreeMap::new()
}

#[cfg(unix)]
fn filesystem_space(path: &Path) -> Result<(u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(path.as_os_str().as_bytes()).context("Invalid filesystem path")?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: valid NUL-terminated path and writable statvfs result; read only on success.
    let stats = unsafe {
        ensure!(
            libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) == 0,
            "Filesystem unavailable"
        );
        stats.assume_init()
    };
    let block = stats.f_frsize as u64;
    let total = (stats.f_blocks as u64)
        .checked_mul(block)
        .context("Filesystem size overflow")?;
    let free = (stats.f_bfree as u64)
        .checked_mul(block)
        .context("Filesystem size overflow")?;
    ensure!(total > 0 && free <= total, "Filesystem size unavailable");
    Ok((total - free, total))
}

#[cfg(windows)]
fn filesystem_space(path: &Path) -> Result<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            available: *mut u64,
            total: *mut u64,
            free: *mut u64,
        ) -> i32;
    }
    let mut directory: Vec<u16> = path.as_os_str().encode_wide().collect();
    ensure!(!directory.contains(&0), "Invalid filesystem path");
    directory.push(0);
    let (mut available, mut total, mut free) = (0_u64, 0_u64, 0_u64);
    // SAFETY: terminated UTF-16 path and valid writable output pointers.
    unsafe {
        ensure!(
            GetDiskFreeSpaceExW(directory.as_ptr(), &mut available, &mut total, &mut free) != 0,
            "Filesystem unavailable"
        );
    }
    ensure!(total > 0 && free <= total, "Filesystem size unavailable");
    Ok((total - free, total))
}

#[cfg(not(any(unix, windows)))]
fn filesystem_space(_path: &Path) -> Result<(u64, u64)> {
    bail!("Filesystem probe unavailable")
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug)]
struct CpuTimes {
    fields: Vec<u64>,
}

#[cfg(target_os = "linux")]
fn parse_cpu_times(raw: &str) -> Option<CpuTimes> {
    let mut fields = raw
        .lines()
        .find(|line| line.starts_with("cpu "))?
        .split_whitespace();
    fields.next()?;
    let fields: Vec<_> = fields
        .take(8)
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    (fields.len() >= 5).then_some(CpuTimes { fields })
}

#[cfg(target_os = "linux")]
fn io_wait(current: &CpuTimes, previous: Option<&CpuTimes>) -> Option<f64> {
    let previous = previous?;
    if current.fields.len() != previous.fields.len() {
        return None;
    }
    let deltas: Option<Vec<_>> = current
        .fields
        .iter()
        .zip(&previous.fields)
        .map(|(a, b)| a.checked_sub(*b))
        .collect();
    let deltas = deltas?;
    let total = deltas
        .iter()
        .try_fold(0_u64, |acc, v| acc.checked_add(*v))?;
    (total > 0).then(|| 100.0 * deltas[4] as f64 / total as f64)
}

#[cfg(target_os = "linux")]
fn read_u64(path: &Path) -> Option<u64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn collect_platform_extras(collector: &mut HostCollector, now: f64, snapshot: &mut Value) {
    let times = std::fs::read_to_string("/proc/stat")
        .ok()
        .and_then(|v| parse_cpu_times(&v));
    snapshot["host"]["io_wait_pct"] = json!(times
        .as_ref()
        .and_then(|v| io_wait(v, collector.previous_cpu_times.as_ref())));
    collector.previous_cpu_times = times;
    snapshot["sources"]["io_wait"] = source_status(
        now,
        if collector.previous_cpu_times.is_some() {
            None
        } else {
            Some("Probe unavailable")
        },
        true,
    );
    let arc = std::fs::read_to_string("/proc/spl/kstat/zfs/arcstats")
        .ok()
        .and_then(|raw| {
            raw.lines().find_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.first() == Some(&"size") {
                    fields.get(2).and_then(|v| v.parse::<u64>().ok())
                } else {
                    None
                }
            })
        });
    snapshot["host"]["arc_bytes"] = json!(arc);
    snapshot["sources"]["arc"] = source_status(now, None, arc.is_some());
    let mut fan_count = 0;
    if let Ok(chips) = std::fs::read_dir("/sys/class/hwmon") {
        let mut chips: Vec<_> = chips.flatten().collect();
        chips.sort_by_key(|chip| chip.file_name());
        for chip in chips {
            let Ok(entries) = std::fs::read_dir(chip.path()) else {
                continue;
            };
            let name = std::fs::read_to_string(chip.path().join("name"))
                .unwrap_or_else(|_| "hwmon".into());
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let filename = entry.file_name().to_string_lossy().into_owned();
                let Some(number) = filename
                    .strip_prefix("fan")
                    .and_then(|v| v.strip_suffix("_input"))
                    .filter(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()))
                else {
                    continue;
                };
                let value = read_u64(&entry.path()).map(|v| v as f64);
                let label = std::fs::read_to_string(chip.path().join(format!("fan{number}_label")))
                    .unwrap_or_else(|_| format!("{} fan {number}", name.trim()));
                let mut row = sensor(
                    &format!("fan/{}/{number}", chip.file_name().to_string_lossy()),
                    label.trim(),
                    name.trim(),
                    "fan",
                    "RPM",
                    value,
                    None,
                    None,
                    "fans",
                    now,
                );
                row["alarm"] = json!(read_u64(&chip.path().join(format!("fan{number}_alarm")))
                    .is_some_and(|v| v != 0));
                row["fault"] = json!(read_u64(&chip.path().join(format!("fan{number}_fault")))
                    .is_some_and(|v| v != 0));
                append_sensor(snapshot, row);
                fan_count += 1;
            }
        }
    }
    snapshot["sources"]["fans"] = source_status(now, None, fan_count > 0);
}

#[cfg(not(target_os = "linux"))]
fn collect_platform_extras(_collector: &mut HostCollector, now: f64, snapshot: &mut Value) {
    snapshot["sources"]["io_wait"] = source_status(now, None, false);
    snapshot["sources"]["arc"] = source_status(now, None, false);
    snapshot["sources"]["fans"] = source_status(now, None, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_names_are_bounded_ascii_identifiers() {
        for name in ["", "-host", "пробен хост", &"long".repeat(40)] {
            let id = default_id(name);
            assert!(id.len() <= 32);
            validate_identity(&id).unwrap();
        }
        assert_ne!(
            default_id(&"same".repeat(30)),
            default_id(&format!("{}x", "same".repeat(30)))
        );
    }

    #[test]
    fn incomplete_network_sets_do_not_show_partial_aggregates() {
        assert_eq!(
            aggregate(&[json!({"rx":2.0}), json!({"rx":3.0})], "rx"),
            json!(5.0)
        );
        assert!(aggregate(&[json!({"rx":2.0}), json!({"rx":null})], "rx").is_null());
        assert!(aggregate(&[], "rx").is_null());
    }

    #[test]
    fn observed_max_is_not_used_as_a_temperature_warning() {
        let mut snapshot = empty_snapshot(&json!({"name":"Fixture","address":""}), 1, 1000.0);
        let row = sensor(
            "temp",
            "CPU",
            "cpu",
            "temperature",
            "°C",
            Some(70.0),
            None,
            Some(95.0),
            "temperatures",
            1000.0,
        );
        append_sensor(&mut snapshot, row);
        assert!(snapshot["alerts"].as_array().unwrap().is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_iowait_ignores_guest_and_handles_resets() {
        let first = parse_cpu_times("cpu 10 0 10 80 10 0 0 0 900 0\ncpu0 10\n").unwrap();
        let second = parse_cpu_times("cpu 20 0 20 100 20 0 0 0 990 0\n").unwrap();
        assert_eq!(io_wait(&first, None), None);
        assert_eq!(io_wait(&second, Some(&first)), Some(20.0));
        assert_eq!(io_wait(&first, Some(&second)), None);
        assert_eq!(io_wait(&first, Some(&first)), None);
        assert!(parse_cpu_times("cpu malformed").is_none());
    }
}
