//! Cumulative component energy: CPU package (RAPL) plus each GPU's integrated
//! power, kept across restarts in a small state file.
//!
//! This counts the CPU package and GPU boards only. It is not the wall power of
//! the machine, and it says so (`scope: "components"`). A value that cannot be
//! read is unknown (`null`), never zero; a domain keeps what it has counted
//! even while its source is unreadable, so the total never goes down.
use crate::config::atomic_write;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// The snapshot lists at most this many domains: the CPU package and GPUs.
pub const MAX_DOMAINS: usize = 8;
const MAX_GPUS: usize = MAX_DOMAINS - 1;
/// A power reading is carried across at most this long between two reads;
/// a longer silence is not bridged.
pub const MAX_GAP_S: f64 = 300.;
/// The state file is written at most this often (and when the collector stops).
pub const SAVE_EVERY_S: f64 = 60.;
/// One RAPL package cannot plausibly draw more than this on average between
/// two reads; a larger implied power means the counter was reset, not wrapped.
const MAX_PACKAGE_W: f64 = 2000.;
/// A single GPU reading above this is not believed.
const MAX_SAMPLE_W: f64 = 5000.;
/// Shortest span over which a RAPL power is derived.
const MIN_POWER_SPAN_S: f64 = 0.5;
const MAX_ZONES: usize = 16;
const JOULES_PER_KWH: f64 = 3_600_000.;
/// No honest counter reaches this (about 2.8 GWh); larger saved values are corrupt.
const SANE_JOULES: f64 = 1e13;
pub const STATE_FILE: &str = "energy.json";
const STATE_BOUND: u64 = 16 * 1024;

fn round(value: f64, digits: i32) -> f64 {
    let scale = 10_f64.powi(digits);
    (value * scale).round() / scale
}
fn kwh(joules: f64) -> f64 {
    round(joules / JOULES_PER_KWH, 6)
}
fn finite(value: &Value) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}

/// A power reading and the wall-clock second the source took it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerSample {
    pub at: f64,
    pub watts: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct GpuPower {
    pub key: String,
    pub sample: PowerSample,
}
/// What the rest of the snapshot offers: the CPU package watts (used only when
/// RAPL cannot be read) and each GPU's reading.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    pub package: Option<PowerSample>,
    pub gpus: Vec<GpuPower>,
}
impl Input {
    /// Only readings from a source that is currently fresh and ok are offered.
    pub fn from_snapshot(snapshot: &Value) -> Self {
        let sample = |at: &Value, watts: &Value| {
            Some(PowerSample {
                at: finite(at).filter(|t| *t > 0.)?,
                watts: finite(watts).filter(|w| (0. ..=MAX_SAMPLE_W).contains(w))?,
            })
        };
        let turbostat = &snapshot["sources"]["turbostat"];
        let package = if turbostat["ok"] == true && turbostat["enabled"] != false {
            sample(&turbostat["updated_at"], &snapshot["power"]["package_w"])
        } else {
            None
        };
        let gpus = snapshot["gpus"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|gpu| {
                let key = gpu["id"]
                    .as_str()
                    .filter(|k| !k.is_empty() && k.len() <= 64)?;
                Some(GpuPower {
                    key: key.to_owned(),
                    sample: sample(&gpu["updated_at"], &gpu["power_w"])?,
                })
            })
            .collect();
        Self { package, gpus }
    }
}

/// Trapezoid integration of a power that is sampled now and then.
#[derive(Default)]
struct Integrator {
    last: Option<(f64, f64)>,
}
impl Integrator {
    /// Joules added by the step to `(at, watts)`. A repeated or older sample
    /// adds nothing and changes nothing; a step longer than [`MAX_GAP_S`] adds
    /// nothing but becomes the new starting point.
    fn advance(&mut self, at: f64, watts: f64) -> f64 {
        let added = match self.last {
            Some((before, _)) if at <= before => return 0.,
            Some((before, power)) if at - before <= MAX_GAP_S => {
                (at - before) * (power + watts) / 2.
            }
            _ => 0.,
        };
        self.last = Some((at, watts));
        added
    }
}

/// Microjoules between two reads of a cumulative RAPL counter. A counter that
/// went down wrapped at its range if that range is known and fits; a step that
/// is not plausible for the elapsed time is a reset, and yields nothing.
pub fn counter_delta_uj(
    previous: u64,
    current: u64,
    range: Option<u64>,
    elapsed_s: f64,
) -> Option<u64> {
    if !(elapsed_s.is_finite() && elapsed_s > 0.) {
        return None;
    }
    let delta = if current >= previous {
        u128::from(current - previous)
    } else {
        let range = range.filter(|r| *r >= previous && *r >= current)?;
        u128::from(range - previous) + u128::from(current)
    };
    let watts = delta as f64 / 1e6 / elapsed_s;
    if watts > MAX_PACKAGE_W {
        return None;
    }
    u64::try_from(delta).ok()
}

struct Package {
    key: String,
    raw: u64,
    max: Option<u64>,
}
fn read_small(path: &Path) -> Option<String> {
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(256)
        .read_to_string(&mut text)
        .ok()?;
    Some(text.trim().to_owned())
}
fn zone_index(name: &str) -> Option<u32> {
    let digits = name.strip_prefix("intel-rapl:")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}
/// CPU package zones (`intel-rapl:<n>`, named `package-*`). Sub-zones such as
/// cores or DRAM and the whole-platform `psys` zone are not packages.
fn read_packages(sys: &Path) -> Vec<Package> {
    let root = sys.join("class/powercap");
    let mut zones = fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            Some((zone_index(&name)?, name))
        })
        .collect::<Vec<_>>();
    zones.sort();
    zones.truncate(MAX_ZONES);
    zones
        .into_iter()
        .filter_map(|(_, name)| {
            let dir = root.join(&name);
            if !read_small(&dir.join("name"))?.starts_with("package") {
                return None;
            }
            let raw = read_small(&dir.join("energy_uj"))?.parse::<u64>().ok()?;
            let max = read_small(&dir.join("max_energy_range_uj"))
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|m| *m > 0);
            Some(Package {
                key: name,
                raw,
                max,
            })
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Zone {
    raw: u64,
    max: Option<u64>,
    at: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Gpu {
    key: String,
    n: u8,
    joules: f64,
}
/// The persisted counter. Raw RAPL values are kept so that, after a restart,
/// the hardware counter (which kept counting) accounts for the time between.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    schema: u32,
    since: u64,
    updated_at: u64,
    cpu_source: Option<String>,
    cpu_joules: f64,
    zones: BTreeMap<String, Zone>,
    gpus: Vec<Gpu>,
}
impl State {
    fn fresh(now: f64) -> Self {
        let at = now.max(1.) as u64;
        Self {
            schema: 1,
            since: at,
            updated_at: at,
            cpu_source: None,
            cpu_joules: 0.,
            zones: BTreeMap::new(),
            gpus: Vec::new(),
        }
    }
    fn valid(&self, now: f64) -> bool {
        let horizon = now + 86_400.;
        let joules = |v: f64| v.is_finite() && (0. ..=SANE_JOULES).contains(&v);
        self.schema == 1
            && self.since > 0
            && (self.since as f64) <= horizon
            && (self.updated_at as f64) <= horizon
            && self
                .cpu_source
                .as_deref()
                .is_none_or(|s| s == "rapl" || s == "power")
            && joules(self.cpu_joules)
            && self.zones.len() <= MAX_ZONES
            && self
                .zones
                .iter()
                .all(|(k, z)| zone_index(k).is_some() && z.at.is_finite())
            && self.gpus.len() <= MAX_GPUS
            && self.gpus.iter().enumerate().all(|(i, g)| {
                !g.key.is_empty()
                    && g.key.len() <= 64
                    && usize::from(g.n) < MAX_GPUS
                    && joules(g.joules)
                    && self.gpus[..i].iter().all(|o| o.key != g.key && o.n != g.n)
            })
    }
}

enum Loaded {
    Missing,
    Restored(Box<State>),
    Rejected,
}
fn load(path: &Path, now: f64) -> Loaded {
    let mut raw = Vec::new();
    let read = fs::File::open(path).and_then(|f| f.take(STATE_BOUND + 1).read_to_end(&mut raw));
    match read {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(_) => return Loaded::Rejected,
        Ok(_) if raw.len() as u64 > STATE_BOUND => return Loaded::Rejected,
        Ok(_) => {}
    }
    match serde_json::from_slice::<State>(&raw) {
        Ok(state) if state.valid(now) => Loaded::Restored(Box::new(state)),
        _ => Loaded::Rejected,
    }
}

pub struct EnergyMeter {
    state: State,
    dir: Option<PathBuf>,
    sys: PathBuf,
    cpu_integrator: Integrator,
    gpu_integrators: BTreeMap<String, Integrator>,
    zone_mono: BTreeMap<String, f64>,
    cpu_watts: Option<f64>,
    gpu_watts: BTreeMap<String, f64>,
    live: bool,
    dirty: bool,
    saved_mono: Option<f64>,
    save_error: Option<&'static str>,
}
impl EnergyMeter {
    /// Start a counter, or continue the one saved in `dir`. Without a state
    /// directory the counter lives in memory and starts now. An unreadable or
    /// invalid state file is set aside as `energy.json.corrupt` and a new
    /// counter starts, rather than guessing at its contents.
    pub fn open(dir: Option<&Path>, sys: impl Into<PathBuf>, now: f64) -> Self {
        let state = match dir {
            None => State::fresh(now),
            Some(dir) => {
                let path = dir.join(STATE_FILE);
                match load(&path, now) {
                    Loaded::Missing => State::fresh(now),
                    Loaded::Restored(state) => *state,
                    Loaded::Rejected => {
                        eprintln!("Energy state unreadable; kept aside and a new counter started");
                        let _ = fs::rename(&path, path.with_extension("json.corrupt"));
                        State::fresh(now)
                    }
                }
            }
        };
        Self {
            state,
            dir: dir.map(Path::to_path_buf),
            sys: sys.into(),
            cpu_integrator: Integrator::default(),
            gpu_integrators: BTreeMap::new(),
            zone_mono: BTreeMap::new(),
            cpu_watts: None,
            gpu_watts: BTreeMap::new(),
            live: false,
            dirty: false,
            saved_mono: None,
            save_error: None,
        }
    }

    /// Take one reading. `wall` is Unix seconds, `mono` a monotonic clock.
    pub fn observe(&mut self, input: &Input, wall: f64, mono: f64) {
        let mut live = false;
        let packages = read_packages(&self.sys);
        if packages.is_empty() {
            self.zone_mono.clear();
            if let Some(sample) = input.package {
                self.state.cpu_joules += self.cpu_integrator.advance(sample.at, sample.watts);
                self.state.cpu_source = Some("power".into());
                self.cpu_watts = Some(round(sample.watts, 2));
                live = true;
            } else {
                self.cpu_watts = None;
            }
        } else {
            let mut added_uj = 0u128;
            let mut watts = 0.;
            let mut watts_known = true;
            for package in &packages {
                let before = self.state.zones.get(&package.key).cloned();
                match &before {
                    Some(zone) => {
                        let (elapsed, continuous) = match self.zone_mono.get(&package.key) {
                            Some(then) => (mono - then, true),
                            None => (wall - zone.at, false),
                        };
                        let range = package.max.or(zone.max);
                        match counter_delta_uj(zone.raw, package.raw, range, elapsed) {
                            Some(delta) => {
                                added_uj += u128::from(delta);
                                if continuous && elapsed >= MIN_POWER_SPAN_S {
                                    watts += delta as f64 / 1e6 / elapsed;
                                } else {
                                    watts_known = false;
                                }
                            }
                            None => watts_known = false,
                        }
                    }
                    None => watts_known = false,
                }
                self.state.zones.insert(
                    package.key.clone(),
                    Zone {
                        raw: package.raw,
                        max: package.max.or(before.and_then(|z| z.max)),
                        at: wall,
                    },
                );
                self.zone_mono.insert(package.key.clone(), mono);
            }
            self.zone_mono
                .retain(|key, _| packages.iter().any(|p| &p.key == key));
            self.state.cpu_joules += added_uj as f64 / 1e6;
            self.state.cpu_source = Some("rapl".into());
            self.cpu_watts = watts_known.then(|| round(watts, 2));
            self.cpu_integrator = Integrator::default();
            live = true;
        }
        self.gpu_watts.clear();
        for gpu in &input.gpus {
            let index = match self.state.gpus.iter().position(|g| g.key == gpu.key) {
                Some(index) => index,
                None => {
                    if self.state.gpus.len() >= MAX_GPUS {
                        continue;
                    }
                    let n = (0..MAX_GPUS as u8)
                        .find(|n| self.state.gpus.iter().all(|g| g.n != *n))
                        .unwrap_or(0);
                    self.state.gpus.push(Gpu {
                        key: gpu.key.clone(),
                        n,
                        joules: 0.,
                    });
                    self.state.gpus.len() - 1
                }
            };
            let added = self
                .gpu_integrators
                .entry(gpu.key.clone())
                .or_default()
                .advance(gpu.sample.at, gpu.sample.watts);
            self.state.gpus[index].joules += added;
            self.gpu_watts
                .insert(gpu.key.clone(), round(gpu.sample.watts, 2));
            live = true;
        }
        self.live = live;
        if live {
            self.state.updated_at = wall.max(1.) as u64;
            self.dirty = true;
        }
        self.save_if_due(mono);
    }

    /// The snapshot's `energy` object, or `None` when this host has never had
    /// an energy source (the field is then simply absent).
    pub fn snapshot(&self) -> Option<Value> {
        let mut domains = Vec::new();
        if let Some(source) = &self.state.cpu_source {
            domains.push(json!({
                "id":"cpu-package","label":"CPU package",
                "kwh":kwh(self.state.cpu_joules),"source":source,"watts":self.cpu_watts
            }));
        }
        let mut gpus = self.state.gpus.iter().collect::<Vec<_>>();
        gpus.sort_by_key(|g| g.n);
        for gpu in gpus {
            domains.push(json!({
                "id":format!("gpu-{}",gpu.n),"label":format!("GPU {}",gpu.n),
                "kwh":kwh(gpu.joules),"source":"power","watts":self.gpu_watts.get(&gpu.key),
                "device":gpu.key
            }));
        }
        domains.truncate(MAX_DOMAINS);
        if domains.is_empty() {
            return None;
        }
        let total = round(
            domains
                .iter()
                .filter_map(|d| d["kwh"].as_f64())
                .sum::<f64>(),
            6,
        );
        Some(json!({
            "since":self.state.since,"updated_at":self.state.updated_at,
            "total_kwh":total,"domains":domains,"scope":"components",
            "persistent":self.dir.is_some()
        }))
    }

    /// Source status in the snapshot's usual shape. A counter that cannot be
    /// saved is reported, because its total would not survive a restart.
    pub fn status(&self, wall: f64) -> Value {
        let error = self.save_error.or(if self.live {
            None
        } else {
            Some("Energy sources unavailable")
        });
        json!({
            "enabled":true,"ok":error.is_none(),"updated_at":self.state.updated_at,
            "age_s":round((wall - self.state.updated_at as f64).max(0.), 1),"error":error
        })
    }

    fn save(&mut self) {
        let Some(dir) = &self.dir else {
            self.dirty = false;
            return;
        };
        let written = serde_json::to_vec(&self.state)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| atomic_write(&dir.join(STATE_FILE), &bytes, 0o600));
        if written.is_ok() {
            self.dirty = false;
            self.save_error = None;
        } else {
            self.save_error = Some("Energy state could not be saved");
        }
    }
    fn save_if_due(&mut self, mono: f64) {
        if !self.dirty || self.dir.is_none() {
            return;
        }
        if self.saved_mono.is_some_and(|t| mono - t < SAVE_EVERY_S) {
            return;
        }
        self.saved_mono = Some(mono);
        self.save();
    }
    /// Write any unsaved change now. Also done when the meter is dropped.
    pub fn flush(&mut self) {
        if self.dirty {
            self.save();
        }
    }
}
impl Drop for EnergyMeter {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGE: u64 = 262_143_328_850;

    fn zone(sys: &Path, name: &str, label: &str, energy: u64, range: Option<u64>) {
        let dir = sys.join("class/powercap").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("name"), format!("{label}\n")).unwrap();
        fs::write(dir.join("energy_uj"), format!("{energy}\n")).unwrap();
        if let Some(range) = range {
            fs::write(dir.join("max_energy_range_uj"), format!("{range}\n")).unwrap();
        }
    }
    fn gpu(key: &str, at: f64, watts: f64) -> GpuPower {
        GpuPower {
            key: key.into(),
            sample: PowerSample { at, watts },
        }
    }
    fn gpus(list: Vec<GpuPower>) -> Input {
        Input {
            package: None,
            gpus: list,
        }
    }
    fn watts_input(at: f64, watts: f64) -> Input {
        Input {
            package: Some(PowerSample { at, watts }),
            gpus: vec![],
        }
    }
    fn domain(snapshot: &Value, id: &str) -> Value {
        snapshot["domains"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == id)
            .unwrap_or_else(|| panic!("domain {id}"))
            .clone()
    }

    #[test]
    fn counter_steps_wrap_at_the_range_and_reject_resets() {
        assert_eq!(counter_delta_uj(100, 250, None, 3.), Some(150));
        assert_eq!(counter_delta_uj(7, 7, None, 3.), Some(0));
        assert_eq!(
            counter_delta_uj(RANGE - 100, 50, Some(RANGE), 3.),
            Some(150),
            "wrapped once"
        );
        assert_eq!(
            counter_delta_uj(500, 100, None, 3.),
            None,
            "reset, range unknown"
        );
        assert_eq!(
            counter_delta_uj(500, RANGE + 5, Some(RANGE), 3.),
            None,
            "a value above the range is not trusted"
        );
        assert_eq!(
            counter_delta_uj(1_000_000_000, 10, Some(RANGE), 3.),
            None,
            "a reset to zero looks like a huge wrap and is rejected as implausible"
        );
        assert_eq!(
            counter_delta_uj(0, 6_000_000_001, None, 3.),
            None,
            "over 2 kW"
        );
        assert_eq!(
            counter_delta_uj(0, 6_000_000_000, None, 3.),
            Some(6_000_000_000)
        );
        for elapsed in [0., -1., f64::NAN, f64::INFINITY] {
            assert_eq!(counter_delta_uj(1, 2, None, elapsed), None, "{elapsed}");
        }
    }

    #[test]
    fn trapezoid_integration_does_not_bridge_a_long_gap() {
        let mut i = Integrator::default();
        assert_eq!(i.advance(1000., 100.), 0., "first reading is only a start");
        assert_eq!(i.advance(1010., 200.), 1500.);
        assert_eq!(
            i.advance(1010., 900.),
            0.,
            "the same sample twice adds nothing"
        );
        assert_eq!(i.advance(1005., 900.), 0., "an older sample is ignored");
        assert_eq!(
            i.advance(1310., 100.),
            300. * 150.,
            "exactly five minutes is still bridged"
        );
        assert_eq!(i.advance(1611., 100.), 0., "301 s is not bridged");
        assert_eq!(
            i.advance(1621., 300.),
            2000.,
            "and counting resumes from it"
        );
    }

    #[test]
    fn gpu_energy_integrates_between_distinct_reads_with_a_gap() {
        let mut meter = EnergyMeter::open(None, "/nonexistent-sys", 900.);
        let card = "pci:0000:01:00.0";
        meter.observe(&gpus(vec![gpu(card, 1000., 100.)]), 1000., 1.);
        // Collector ticks between GPU source updates repeat the same sample.
        for tick in 1..=3 {
            meter.observe(
                &gpus(vec![gpu(card, 1000., 100.)]),
                1000. + f64::from(tick),
                1. + f64::from(tick),
            );
        }
        meter.observe(&gpus(vec![gpu(card, 1010., 200.)]), 1011., 12.);
        meter.observe(&gpus(vec![gpu(card, 1310., 100.)]), 1311., 312.);
        meter.observe(&gpus(vec![gpu(card, 1611., 100.)]), 1612., 613.);
        meter.observe(&gpus(vec![gpu(card, 1621., 300.)]), 1622., 623.);
        let energy = meter.snapshot().unwrap();
        let g = domain(&energy, "gpu-0");
        assert_eq!(g["kwh"], 0.013472, "1500 + 45000 + 0 + 2000 J");
        assert_eq!(g["watts"], 300.);
        assert_eq!(g["source"], "power");
        assert_eq!(g["label"], "GPU 0");
        assert_eq!(g["device"], "pci:0000:01:00.0");
        assert_eq!(energy["total_kwh"], 0.013472);
    }

    #[test]
    fn rapl_sums_packages_and_ignores_subzones_and_platform_zone() {
        let sys = tempfile::tempdir().unwrap();
        let set = |p0: u64, p1: u64, psys: u64| {
            zone(sys.path(), "intel-rapl:0", "package-0", p0, Some(RANGE));
            zone(sys.path(), "intel-rapl:1", "package-1", p1, Some(RANGE));
            zone(sys.path(), "intel-rapl:0:0", "core", 999, Some(RANGE));
            zone(sys.path(), "intel-rapl:2", "psys", psys, Some(RANGE));
            zone(sys.path(), "intel-rapl-mmio:0", "package-0", 7, Some(RANGE));
        };
        let mut meter = EnergyMeter::open(None, sys.path(), 1000.);
        set(1_000_000, 2_000_000, 5_000_000);
        meter.observe(&Input::default(), 1000., 10.);
        let first = meter.snapshot().unwrap();
        let cpu = domain(&first, "cpu-package");
        assert_eq!(cpu["kwh"], 0., "a new counter has counted nothing yet");
        assert!(cpu["watts"].is_null(), "no power from a single read");
        assert_eq!(cpu["source"], "rapl");
        set(
            1_000_000 + 1_800_000_000,
            2_000_000 + 1_800_000_000,
            5_000_000 + 9_000_000_000_000,
        );
        meter.observe(&Input::default(), 1003., 13.);
        let energy = meter.snapshot().unwrap();
        let cpu = domain(&energy, "cpu-package");
        assert_eq!(
            cpu["kwh"], 0.001,
            "two packages x 1800 J, nothing from psys"
        );
        assert_eq!(cpu["watts"], 1200., "3600 J over 3 s");
        assert_eq!(energy["domains"].as_array().unwrap().len(), 1);
        assert_eq!(energy["scope"], "components");
        assert_eq!(energy["total_kwh"], 0.001);
    }

    #[test]
    fn rapl_wrap_between_reads_is_counted_once() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1000.);
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            RANGE - 1_500_000_000,
            Some(RANGE),
        );
        meter.observe(&Input::default(), 1000., 10.);
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            300_000_000,
            Some(RANGE),
        );
        meter.observe(&Input::default(), 1003., 13.);
        let cpu = domain(&meter.snapshot().unwrap(), "cpu-package");
        assert_eq!(
            cpu["kwh"], 0.0005,
            "1500 J before the wrap + 300 J after = 1800 J"
        );
        assert_eq!(cpu["watts"], 600.);
    }

    #[test]
    fn rapl_counter_reset_adds_nothing_and_counting_continues() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1000.);
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            50_000_000_000,
            Some(RANGE),
        );
        meter.observe(&Input::default(), 1000., 10.);
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            50_000_000_000 + 360_000_000,
            Some(RANGE),
        );
        meter.observe(&Input::default(), 1003., 13.);
        let before = domain(&meter.snapshot().unwrap(), "cpu-package")["kwh"].clone();
        assert_eq!(before, 0.0001);
        // A reset (suspend, firmware): far lower value, implausible as a wrap.
        zone(sys.path(), "intel-rapl:0", "package-0", 4_000, Some(RANGE));
        meter.observe(&Input::default(), 1006., 16.);
        let cpu = domain(&meter.snapshot().unwrap(), "cpu-package");
        assert_eq!(cpu["kwh"], before, "the total never goes down or jumps");
        assert!(cpu["watts"].is_null());
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            4_000 + 360_000_000,
            Some(RANGE),
        );
        meter.observe(&Input::default(), 1009., 19.);
        assert_eq!(
            domain(&meter.snapshot().unwrap(), "cpu-package")["kwh"],
            0.0002
        );
    }

    #[test]
    fn rapl_that_returns_after_a_silence_counts_the_energy_but_reports_no_power() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1_000.);
        zone(sys.path(), "intel-rapl:0", "package-0", 0, Some(RANGE));
        meter.observe(&Input::default(), 1000., 10.);
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            360_000_000,
            Some(RANGE),
        );
        meter.observe(&Input::default(), 1003., 13.);
        assert_eq!(
            domain(&meter.snapshot().unwrap(), "cpu-package")["watts"],
            120.
        );
        fs::remove_dir_all(sys.path().join("class")).unwrap();
        meter.observe(&Input::default(), 1006., 16.);
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            720_000_000,
            Some(RANGE),
        );
        meter.observe(&Input::default(), 1009., 19.);
        let cpu = domain(&meter.snapshot().unwrap(), "cpu-package");
        assert_eq!(
            cpu["kwh"], 0.0002,
            "the cumulative counter covers the silence"
        );
        assert!(
            cpu["watts"].is_null(),
            "an average over a gap is not the latest power"
        );
    }

    #[test]
    fn package_watts_are_integrated_when_rapl_cannot_be_read() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1000.);
        meter.observe(&watts_input(1000., 100.), 1000., 10.);
        meter.observe(&watts_input(1000., 100.), 1003., 13.);
        meter.observe(&watts_input(1024., 200.), 1025., 35.);
        let energy = meter.snapshot().unwrap();
        let cpu = domain(&energy, "cpu-package");
        assert_eq!(cpu["source"], "power");
        assert_eq!(cpu["kwh"], 0.001, "24 s x 150 W = 3600 J");
        assert_eq!(cpu["watts"], 200.);
        meter.observe(&Input::default(), 1040., 50.);
        let cpu = domain(&meter.snapshot().unwrap(), "cpu-package");
        assert!(
            cpu["watts"].is_null(),
            "watts unknown when the source is silent"
        );
        assert_eq!(cpu["kwh"], 0.001, "what was counted is kept");
    }

    #[test]
    fn rapl_wins_over_package_watts_so_energy_is_not_counted_twice() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1000.);
        zone(sys.path(), "intel-rapl:0", "package-0", 0, Some(RANGE));
        meter.observe(&watts_input(1000., 400.), 1000., 10.);
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            360_000_000,
            Some(RANGE),
        );
        meter.observe(&watts_input(1010., 400.), 1010., 20.);
        let cpu = domain(&meter.snapshot().unwrap(), "cpu-package");
        assert_eq!(cpu["source"], "rapl");
        assert_eq!(
            cpu["kwh"], 0.0001,
            "only the 360 J RAPL counted, not 4000 J of watts"
        );
    }

    #[test]
    fn watts_integration_starts_over_after_rapl_has_counted_the_same_time() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1_000.);
        meter.observe(&watts_input(1000., 100.), 1000., 10.);
        // RAPL takes over for a while ...
        zone(sys.path(), "intel-rapl:0", "package-0", 0, Some(RANGE));
        meter.observe(&Input::default(), 1010., 20.);
        fs::remove_dir_all(sys.path().join("class")).unwrap();
        // ... then vanishes. The watts reading from before must not be bridged
        // across the time RAPL already counted.
        meter.observe(&watts_input(1100., 100.), 1100., 110.);
        let cpu = domain(&meter.snapshot().unwrap(), "cpu-package");
        assert_eq!(cpu["source"], "power");
        assert_eq!(
            cpu["kwh"], 0.,
            "nothing is integrated across the RAPL period"
        );
        meter.observe(&watts_input(1136., 100.), 1136., 146.);
        let cpu = domain(&meter.snapshot().unwrap(), "cpu-package");
        assert_eq!(cpu["kwh"], 0.001, "36 s x 100 W from the new start");
    }

    #[test]
    fn no_source_means_no_energy_field_at_all() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1000.);
        meter.observe(&Input::default(), 1000., 10.);
        assert!(meter.snapshot().is_none());
        // Unreadable zones are no source either.
        fs::create_dir_all(sys.path().join("class/powercap/intel-rapl:0")).unwrap();
        meter.observe(&Input::default(), 1003., 13.);
        assert!(meter.snapshot().is_none());
    }

    #[test]
    fn counter_survives_a_restart_and_the_hardware_counter_covers_the_downtime() {
        let sys = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut a = EnergyMeter::open(Some(state.path()), sys.path(), 1_000.);
        zone(sys.path(), "intel-rapl:0", "package-0", 0, Some(RANGE));
        a.observe(
            &gpus(vec![gpu("pci:0000:01:00.0", 1000., 100.)]),
            1000.,
            10.,
        );
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            1_800_000_000,
            Some(RANGE),
        );
        a.observe(
            &gpus(vec![gpu("pci:0000:01:00.0", 1010., 100.)]),
            1003.,
            13.,
        );
        let saved = a.snapshot().unwrap();
        assert_eq!(domain(&saved, "cpu-package")["kwh"], 0.0005);
        assert_eq!(domain(&saved, "gpu-0")["kwh"], 0.000278, "1000 J");
        drop(a);

        // The collector was stopped for a while; the CPU kept using power.
        zone(
            sys.path(),
            "intel-rapl:0",
            "package-0",
            1_800_000_000 + 360_000_000,
            Some(RANGE),
        );
        let mut b = EnergyMeter::open(Some(state.path()), sys.path(), 1_100.);
        let restored = b.snapshot().unwrap();
        assert_eq!(restored["since"], 1_000, "since survives the restart");
        assert_eq!(restored["total_kwh"], saved["total_kwh"]);
        assert!(domain(&restored, "cpu-package")["watts"].is_null());
        b.observe(&Input::default(), 1_100., 5.);
        let after = b.snapshot().unwrap();
        assert_eq!(
            domain(&after, "cpu-package")["kwh"],
            0.0006,
            "1800 J + 360 J"
        );
        assert_eq!(after["since"], 1_000);
        assert_eq!(
            domain(&after, "gpu-0")["kwh"],
            0.000278,
            "GPU downtime is not guessed"
        );
    }

    #[test]
    fn state_round_trips_through_the_file_atomically() {
        let sys = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(Some(state.path()), sys.path(), 1_000.);
        meter.observe(&gpus(vec![gpu("pci:0000:03:00.0", 1000., 50.)]), 1000., 10.);
        meter.flush();
        let path = state.path().join(STATE_FILE);
        let text = fs::read_to_string(&path).unwrap();
        let document: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(document["schema"], 1);
        assert_eq!(document["since"], 1_000);
        assert!(text.len() < 1024, "the state file stays small");
        let names = fs::read_dir(state.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(names, [STATE_FILE], "no temporary file is left behind");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let again = EnergyMeter::open(Some(state.path()), sys.path(), 2_000.);
        let snapshot = again.snapshot().unwrap();
        assert_eq!(snapshot["since"], 1_000);
        assert_eq!(snapshot["domains"][0]["id"], "gpu-0");
        assert_eq!(snapshot["persistent"], true);
    }

    #[test]
    fn corrupt_state_file_starts_a_fresh_counter_and_is_kept_aside() {
        let sys = tempfile::tempdir().unwrap();
        for (i, garbage) in [
            "{ not json".to_owned(),
            String::new(),
            r#"{"schema":2,"since":5,"updated_at":5,"cpu_source":null,"cpu_joules":1.0,"zones":{},"gpus":[]}"#.into(),
            r#"{"schema":1,"since":0,"updated_at":5,"cpu_source":null,"cpu_joules":1.0,"zones":{},"gpus":[]}"#.into(),
            r#"{"schema":1,"since":5,"updated_at":5,"cpu_source":"wall","cpu_joules":1.0,"zones":{},"gpus":[]}"#.into(),
            r#"{"schema":1,"since":5,"updated_at":5,"cpu_source":null,"cpu_joules":-1.0,"zones":{},"gpus":[]}"#.into(),
            r#"{"schema":1,"since":5,"updated_at":5,"cpu_source":null,"cpu_joules":1e30,"zones":{},"gpus":[]}"#.into(),
            r#"{"schema":1,"since":5,"updated_at":5,"cpu_source":null,"cpu_joules":1.0,"zones":{"../x":{"raw":1,"max":null,"at":1.0}},"gpus":[]}"#.into(),
            r#"{"schema":1,"since":5,"updated_at":5,"cpu_source":null,"cpu_joules":0.0,"zones":{},"gpus":[{"key":"a","n":0,"joules":1.0},{"key":"b","n":0,"joules":1.0}]}"#.into(),
            r#"{"schema":1,"since":5,"updated_at":5,"cpu_source":null,"cpu_joules":0.0,"zones":{},"gpus":[{"key":"a","n":9,"joules":1.0}]}"#.into(),
            "x".repeat(STATE_BOUND as usize + 1),
        ]
        .into_iter()
        .enumerate()
        {
            let state = tempfile::tempdir().unwrap();
            let path = state.path().join(STATE_FILE);
            fs::write(&path, &garbage).unwrap();
            let mut meter = EnergyMeter::open(Some(state.path()), sys.path(), 7_000.);
            assert_eq!(
                fs::read_to_string(state.path().join("energy.json.corrupt")).unwrap(),
                garbage,
                "case {i}: the bad file is kept"
            );
            meter.observe(&gpus(vec![gpu("pci:0000:01:00.0", 7000., 10.)]), 7000., 1.);
            let snapshot = meter.snapshot().unwrap();
            assert_eq!(snapshot["since"], 7_000, "case {i}: a new counter starts now");
            assert_eq!(snapshot["total_kwh"], 0., "case {i}");
            meter.flush();
            let fresh: Value =
                serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(fresh["since"], 7_000, "case {i}: a valid file replaces it");
        }
    }

    #[test]
    fn missing_state_file_is_a_normal_first_start() {
        let sys = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(Some(&state.path().join("not-yet")), sys.path(), 4_000.);
        meter.observe(&gpus(vec![gpu("pci:0000:01:00.0", 4000., 10.)]), 4000., 1.);
        assert_eq!(meter.snapshot().unwrap()["since"], 4_000);
        meter.flush();
        assert!(state.path().join("not-yet").join(STATE_FILE).is_file());
        assert!(!state
            .path()
            .join("not-yet")
            .join("energy.json.corrupt")
            .exists());
    }

    #[test]
    fn without_a_state_directory_the_counter_is_kept_in_memory() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 3_000.);
        meter.observe(&gpus(vec![gpu("pci:0000:01:00.0", 3000., 10.)]), 3000., 1.);
        let energy = meter.snapshot().unwrap();
        assert_eq!(energy["since"], 3_000, "since is the collector's start");
        assert_eq!(energy["persistent"], false);
        assert_eq!(
            meter.status(3_001.)["ok"],
            true,
            "in-memory is a choice, not a fault"
        );
        let later = EnergyMeter::open(None, sys.path(), 9_000.);
        assert!(later.snapshot().is_none());
    }

    #[test]
    fn saving_is_throttled_to_once_a_minute_and_flushed_on_drop() {
        let sys = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let path = state.path().join(STATE_FILE);
        let saved =
            || -> Value { serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap() };
        let mut meter = EnergyMeter::open(Some(state.path()), sys.path(), 1_000.);
        meter.observe(
            &gpus(vec![gpu("pci:0000:01:00.0", 1000., 100.)]),
            1000.,
            100.,
        );
        assert_eq!(
            saved()["updated_at"],
            1_000,
            "the first reading is saved at once"
        );
        meter.observe(
            &gpus(vec![gpu("pci:0000:01:00.0", 1030., 100.)]),
            1030.,
            130.,
        );
        assert_eq!(saved()["updated_at"], 1_000, "not again within a minute");
        meter.observe(
            &gpus(vec![gpu("pci:0000:01:00.0", 1059., 100.)]),
            1059.,
            159.,
        );
        assert_eq!(saved()["updated_at"], 1_000);
        meter.observe(
            &gpus(vec![gpu("pci:0000:01:00.0", 1061., 100.)]),
            1061.,
            160.,
        );
        assert_eq!(saved()["updated_at"], 1_061, "a minute has passed");
        meter.observe(
            &gpus(vec![gpu("pci:0000:01:00.0", 1070., 100.)]),
            1070.,
            169.,
        );
        assert_eq!(saved()["updated_at"], 1_061);
        drop(meter);
        assert_eq!(
            saved()["updated_at"],
            1_070,
            "shutdown flushes the last reading"
        );
    }

    #[test]
    fn an_unsaved_unchanged_meter_never_rewrites_the_file() {
        let sys = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut first = EnergyMeter::open(Some(state.path()), sys.path(), 1_000.);
        first.observe(&gpus(vec![gpu("pci:0000:01:00.0", 1000., 100.)]), 1000., 1.);
        drop(first);
        let path = state.path().join(STATE_FILE);
        let before = fs::read(&path).unwrap();
        drop(EnergyMeter::open(Some(state.path()), sys.path(), 5_000.));
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn an_unusable_state_directory_is_reported_without_losing_the_reading() {
        let sys = tempfile::tempdir().unwrap();
        let blocker = tempfile::tempdir().unwrap();
        let in_the_way = blocker.path().join("file");
        fs::write(&in_the_way, b"not a directory").unwrap();
        let mut meter = EnergyMeter::open(Some(&in_the_way), sys.path(), 1_000.);
        meter.observe(&gpus(vec![gpu("pci:0000:01:00.0", 1000., 100.)]), 1000., 1.);
        assert!(meter.snapshot().is_some(), "the counter still runs");
        let status = meter.status(1_001.);
        assert_eq!(status["ok"], false);
        assert_eq!(status["error"], "Energy state could not be saved");
    }

    #[test]
    fn gpu_numbers_are_stable_and_domains_are_bounded() {
        let sys = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(Some(state.path()), sys.path(), 1_000.);
        meter.observe(
            &gpus(vec![
                gpu("pci:0000:0a:00.0", 1000., 10.),
                gpu("pci:0000:01:00.0", 1000., 20.),
            ]),
            1000.,
            1.,
        );
        drop(meter);
        // Reordered inventory and a new card: numbers follow the card, not the position.
        let mut meter = EnergyMeter::open(Some(state.path()), sys.path(), 2_000.);
        meter.observe(
            &gpus(vec![
                gpu("pci:0000:01:00.0", 2000., 20.),
                gpu("pci:0000:05:00.0", 2000., 30.),
                gpu("pci:0000:0a:00.0", 2000., 10.),
            ]),
            2000.,
            1.,
        );
        let energy = meter.snapshot().unwrap();
        assert_eq!(domain(&energy, "gpu-0")["device"], "pci:0000:0a:00.0");
        assert_eq!(domain(&energy, "gpu-1")["device"], "pci:0000:01:00.0");
        assert_eq!(domain(&energy, "gpu-2")["device"], "pci:0000:05:00.0");

        let many = (0..20)
            .map(|i| gpu(&format!("pci:0000:{i:02x}:00.0"), 3000., 10.))
            .collect::<Vec<_>>();
        let sys_rapl = tempfile::tempdir().unwrap();
        zone(sys_rapl.path(), "intel-rapl:0", "package-0", 1, Some(RANGE));
        let mut full = EnergyMeter::open(None, sys_rapl.path(), 3_000.);
        full.observe(&gpus(many), 3000., 1.);
        let energy = full.snapshot().unwrap();
        assert_eq!(energy["domains"].as_array().unwrap().len(), MAX_DOMAINS);
        assert_eq!(energy["domains"][0]["id"], "cpu-package");
        assert_eq!(energy["domains"][7]["id"], "gpu-6");
    }

    #[test]
    fn a_vanished_gpu_keeps_its_energy_with_unknown_watts_so_the_total_never_drops() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1_000.);
        meter.observe(&gpus(vec![gpu("pci:0000:01:00.0", 1000., 100.)]), 1000., 1.);
        meter.observe(
            &gpus(vec![gpu("pci:0000:01:00.0", 1036., 100.)]),
            1036.,
            37.,
        );
        let before = meter.snapshot().unwrap();
        assert_eq!(before["total_kwh"], 0.001);
        meter.observe(&Input::default(), 1100., 101.);
        let after = meter.snapshot().unwrap();
        assert_eq!(after["total_kwh"], 0.001);
        let g = domain(&after, "gpu-0");
        assert_eq!(g["kwh"], 0.001);
        assert!(g["watts"].is_null());
        assert_eq!(meter.status(1_101.)["ok"], false, "nothing live right now");
    }

    #[test]
    fn total_is_the_sum_of_the_domains() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1_000.);
        let reads = [
            (1000., 33.3, 91.7, 17.9),
            (1007., 41.1, 88.2, 12.4),
            (1019., 12.9, 130.4, 55.5),
        ];
        for (i, (t, a, b, c)) in reads.into_iter().enumerate() {
            meter.observe(
                &Input {
                    package: Some(PowerSample { at: t, watts: a }),
                    gpus: vec![gpu("pci:a", t, b), gpu("pci:b", t, c)],
                },
                t,
                t - 900. + i as f64,
            );
        }
        let energy = meter.snapshot().unwrap();
        let sum = energy["domains"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["kwh"].as_f64().unwrap())
            .sum::<f64>();
        assert_eq!(energy["total_kwh"], round(sum, 6));
        assert!(energy["total_kwh"].as_f64().unwrap() > 0.);
        for key in ["since", "updated_at", "total_kwh", "domains", "scope"] {
            assert!(energy.get(key).is_some(), "{key}");
        }
    }

    #[test]
    fn input_takes_only_fresh_ok_sources_and_valid_readings() {
        let snapshot = json!({
            "sources":{"turbostat":{"ok":true,"enabled":true,"updated_at":1000.5}},
            "power":{"package_w":31.5},
            "gpus":[
                {"id":"pci:0000:01:00.0","power_w":70.25,"updated_at":990.},
                {"id":"pci:0000:02:00.0","power_w":null,"updated_at":990.},
                {"id":"pci:0000:03:00.0","power_w":-5,"updated_at":990.},
                {"id":"pci:0000:04:00.0","power_w":9e9,"updated_at":990.},
                {"id":"pci:0000:05:00.0","power_w":10,"updated_at":null},
                {"power_w":10,"updated_at":990.},
                {"id":"","power_w":10,"updated_at":990.}
            ]
        });
        let input = Input::from_snapshot(&snapshot);
        assert_eq!(
            input.package,
            Some(PowerSample {
                at: 1000.5,
                watts: 31.5
            })
        );
        assert_eq!(input.gpus, vec![gpu("pci:0000:01:00.0", 990., 70.25)]);
        for stale in [
            json!({"ok":false,"enabled":true,"updated_at":1000.}),
            json!({"ok":true,"enabled":false,"updated_at":1000.}),
            json!({"ok":true,"enabled":true,"updated_at":null}),
        ] {
            let mut s = snapshot.clone();
            s["sources"]["turbostat"] = stale;
            assert_eq!(Input::from_snapshot(&s).package, None);
        }
        assert_eq!(Input::from_snapshot(&json!({})), Input::default());
    }

    #[test]
    fn status_shape_follows_the_other_sources() {
        let sys = tempfile::tempdir().unwrap();
        let mut meter = EnergyMeter::open(None, sys.path(), 1_000.);
        meter.observe(&watts_input(1000., 10.), 1000., 1.);
        let status = meter.status(1_004.);
        assert_eq!(status["enabled"], true);
        assert_eq!(status["ok"], true);
        assert_eq!(status["updated_at"], 1_000);
        assert_eq!(status["age_s"], 4.);
        assert!(status["error"].is_null());
    }
}
