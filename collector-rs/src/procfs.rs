//! Linux counters. First samples and counter resets are unknown, never boot totals.
use crate::{
    config::Config,
    probes::{num, round, text},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct Rates {
    previous: BTreeMap<String, (f64, f64)>,
}
impl Rates {
    pub fn rate(&mut self, key: &str, counter: Option<f64>, now: f64) -> Option<f64> {
        let Some(value) = counter.filter(|v| v.is_finite() && *v >= 0.) else {
            self.previous.remove(key);
            return None;
        };
        let before = self.previous.insert(key.into(), (value, now));
        before.and_then(|(old, t)| {
            if now > t && value >= old {
                Some(round((value - old) / (now - t), 2))
            } else {
                None
            }
        })
    }
    pub fn clear_prefix(&mut self, prefix: &str) {
        self.previous.retain(|k, _| !k.starts_with(prefix));
    }
}
pub fn parse_cpu(input: &str) -> anyhow::Result<BTreeMap<String, Vec<u64>>> {
    let mut out = BTreeMap::new();
    for line in input.lines() {
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        if key == "cpu"
            || key
                .strip_prefix("cpu")
                .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
        {
            let vals = it
                .take(8)
                .map(str::parse::<u64>)
                .collect::<Result<Vec<_>, _>>()?;
            if vals.len() < 5 {
                anyhow::bail!("short CPU counters")
            };
            out.insert(key.into(), vals);
        }
    }
    if !out.contains_key("cpu") {
        anyhow::bail!("/proc/stat has no CPU counters")
    };
    Ok(out)
}
pub fn cpu_delta(current: &[u64], previous: Option<&Vec<u64>>) -> (Option<f64>, Option<f64>) {
    let Some(old) = previous else {
        return (None, None);
    };
    if current.len() != old.len() || current.len() < 5 {
        return (None, None);
    };
    let delta = current
        .iter()
        .zip(old)
        .map(|(a, b)| a.checked_sub(*b))
        .collect::<Option<Vec<_>>>();
    let Some(d) = delta else { return (None, None) };
    let sum = d.iter().map(|v| *v as f64).sum::<f64>();
    if sum <= 0. {
        return (None, None);
    }
    (
        Some(round((sum - d[3] as f64 - d[4] as f64) * 100. / sum, 2)),
        Some(round(d[4] as f64 * 100. / sum, 2)),
    )
}
pub fn mem_values(input: &str) -> anyhow::Result<BTreeMap<String, f64>> {
    let mut out = BTreeMap::new();
    for line in input.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let fields = rest.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() {
            continue;
        };
        let v = fields[0].parse::<f64>()?
            * if fields.get(1) == Some(&"kB") {
                1024.
            } else {
                1.
            };
        if !v.is_finite() || v < 0. {
            anyhow::bail!("invalid memory counter")
        };
        out.insert(key.into(), v);
    }
    Ok(out)
}
pub fn parse_mem(input: &str) -> anyhow::Result<Value> {
    let v = mem_values(input)?;
    let total = *v
        .get("MemTotal")
        .ok_or_else(|| anyhow::anyhow!("MemTotal absent"))?;
    let avail = *v
        .get("MemAvailable")
        .ok_or_else(|| anyhow::anyhow!("MemAvailable absent"))?;
    if total <= 0. || avail > total {
        anyhow::bail!("invalid host memory total/available")
    }
    Ok(
        json!({"mem_total_bytes":total,"mem_used_bytes":total-avail,"swap_total_bytes":v.get("SwapTotal"),"swap_used_bytes":(v.get("SwapTotal").copied().unwrap_or(0.)-v.get("SwapFree").copied().unwrap_or(0.)).max(0.)}),
    )
}
pub fn parse_net(input: &str) -> anyhow::Result<BTreeMap<String, (f64, f64)>> {
    let mut out = BTreeMap::new();
    for line in input.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let f = rest.split_whitespace().collect::<Vec<_>>();
        if f.len() >= 16 {
            out.insert(name.trim().into(), (f[0].parse()?, f[8].parse()?));
        }
    }
    Ok(out)
}
pub fn parse_diskstats(input: &str) -> anyhow::Result<BTreeMap<String, (f64, f64)>> {
    let mut out = BTreeMap::new();
    for line in input.lines() {
        let f = line.split_whitespace().collect::<Vec<_>>();
        if f.len() >= 10 {
            out.insert(
                f[2].into(),
                (f[5].parse::<f64>()? * 512., f[9].parse::<f64>()? * 512.),
            );
        }
    }
    Ok(out)
}
pub fn arc_size(input: &str) -> Option<f64> {
    input.lines().find_map(|line| {
        let f = line.split_whitespace().collect::<Vec<_>>();
        if f.len() == 3 && f[0] == "size" {
            num(&Value::String(f[2].into()))
        } else {
            None
        }
    })
}
pub fn hostname() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| fs::read_to_string("/etc/hostname"))
        .unwrap_or_else(|_| "node".into())
        .trim()
        .split('.')
        .next()
        .unwrap_or("node")
        .into()
}
pub struct ProcReader {
    pub interfaces: Vec<String>,
    pub devices: Vec<String>,
    pub disk_rates: BTreeMap<String, (Option<f64>, Option<f64>)>,
    cpu: BTreeMap<String, Vec<u64>>,
    rates: Rates,
    proc: PathBuf,
    sys: PathBuf,
}
impl Default for ProcReader {
    fn default() -> Self {
        Self::new("/proc", "/sys")
    }
}
impl ProcReader {
    pub fn new(proc: impl Into<PathBuf>, sys: impl Into<PathBuf>) -> Self {
        Self {
            interfaces: vec![],
            devices: vec![],
            disk_rates: BTreeMap::new(),
            cpu: BTreeMap::new(),
            rates: Rates::default(),
            proc: proc.into(),
            sys: sys.into(),
        }
    }
    fn interfaces(
        &self,
        c: &Config,
        available: &BTreeMap<String, (f64, f64)>,
    ) -> anyhow::Result<Vec<String>> {
        if !c.network_interfaces.is_empty() {
            for n in &c.network_interfaces {
                if !available.contains_key(n) {
                    anyhow::bail!("configured interface absent: {}", text(n, 32))
                }
            }
            let mut v = c.network_interfaces.clone();
            v.sort();
            v.dedup();
            return Ok(v);
        }
        let v = available
            .keys()
            .filter(|n| n.as_str() != "lo")
            .filter(|n| {
                let p = self.sys.join("class/net").join(n);
                (p.join("device").exists() || p.join("bonding").exists())
                    && !p.join("master/bonding").exists()
            })
            .cloned()
            .collect::<Vec<_>>();
        if !v.is_empty() {
            return Ok(v);
        }
        if let Ok(route) = fs::read_to_string(self.proc.join("net/route")) {
            for line in route.lines().skip(1) {
                let f = line.split_whitespace().collect::<Vec<_>>();
                if f.len() > 3 && f[1] == "00000000" && available.contains_key(f[0]) {
                    return Ok(vec![f[0].into()]);
                }
            }
        }
        Ok(vec![])
    }
    fn disks(
        &self,
        c: &Config,
        available: &BTreeMap<String, (f64, f64)>,
    ) -> anyhow::Result<Vec<String>> {
        if !c.disk_devices.is_empty() {
            let mut v = c
                .disk_devices
                .iter()
                .filter_map(|n| {
                    Path::new(n)
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                })
                .collect::<Vec<_>>();
            v.sort();
            v.dedup();
            if v.iter().any(|n| !available.contains_key(n)) {
                anyhow::bail!("configured disk absent")
            };
            return Ok(v);
        }
        let re =
            regex::Regex::new(r"^(?:sd[a-z]+|hd[a-z]+|vd[a-z]+|xvd[a-z]+|nvme\d+n\d+|mmcblk\d+)$")
                .unwrap();
        Ok(available
            .keys()
            .filter(|n| re.is_match(n) && self.sys.join("block").join(n).exists())
            .cloned()
            .collect())
    }
    pub fn read(&mut self, c: &Config, mono: f64) -> Value {
        let mut host = json!({"name":hostname(),"ip":c.host_ip,"cpu_cores":[],"uptime_s":null,"cpu_pct":null,"load":null,"mem_used_bytes":null,"mem_total_bytes":null,"swap_used_bytes":null,"swap_total_bytes":null,"arc_bytes":null,"io_wait_pct":null,"net_rx_bps":null,"net_tx_bps":null,"disk_read_bps":null,"disk_write_bps":null});
        let mut errors = Vec::new();
        match fs::read_to_string(self.proc.join("stat"))
            .map_err(anyhow::Error::from)
            .and_then(|s| parse_cpu(&s))
        {
            Ok(current) => {
                let (cpu, wait) = cpu_delta(&current["cpu"], self.cpu.get("cpu"));
                host["cpu_pct"] = json!(cpu);
                host["io_wait_pct"] = json!(wait);
                let mut cores = current
                    .keys()
                    .filter(|k| k.as_str() != "cpu")
                    .collect::<Vec<_>>();
                cores.sort_by_key(|k| k[3..].parse::<u32>().unwrap_or(0));
                host["cpu_cores"] = json!(cores
                    .into_iter()
                    .take(128)
                    .map(|k| cpu_delta(&current[k], self.cpu.get(k)).0)
                    .collect::<Vec<_>>());
                self.cpu = current
            }
            Err(e) => {
                self.cpu.clear();
                errors.push(format!("cpu: {e}"))
            }
        }
        match fs::read_to_string(self.proc.join("meminfo"))
            .map_err(anyhow::Error::from)
            .and_then(|s| parse_mem(&s))
        {
            Ok(v) => host
                .as_object_mut()
                .unwrap()
                .extend(v.as_object().unwrap().clone()),
            Err(e) => errors.push(format!("memory: {e}")),
        }
        for (file, key, count) in [("uptime", "uptime_s", 1usize), ("loadavg", "load", 3)] {
            match fs::read_to_string(self.proc.join(file))
                .map_err(anyhow::Error::from)
                .and_then(|s| {
                    let a = s
                        .split_whitespace()
                        .take(count)
                        .map(str::parse::<f64>)
                        .collect::<Result<Vec<_>, _>>()?;
                    if a.len() != count || a.iter().any(|v| !v.is_finite() || *v < 0.) {
                        anyhow::bail!("invalid counter")
                    };
                    Ok(a)
                }) {
                Ok(v) => {
                    host[key] = if count == 1 {
                        json!(v[0] as u64)
                    } else {
                        json!(v)
                    }
                }
                Err(e) => errors.push(format!("{file}: {e}")),
            }
        }
        match fs::read_to_string(self.proc.join("net/dev"))
            .map_err(anyhow::Error::from)
            .and_then(|s| parse_net(&s))
            .and_then(|v| Ok((self.interfaces(c, &v)?, v)))
        {
            Ok((names, v)) => {
                self.interfaces = names;
                let pairs = self
                    .interfaces
                    .iter()
                    .map(|n| {
                        (
                            self.rates.rate(&format!("net/{n}/0"), Some(v[n].0), mono),
                            self.rates.rate(&format!("net/{n}/1"), Some(v[n].1), mono),
                        )
                    })
                    .collect::<Vec<_>>();
                for (i, k) in [(0, "net_rx_bps"), (1, "net_tx_bps")] {
                    let vals = pairs
                        .iter()
                        .map(|p| if i == 0 { p.0 } else { p.1 })
                        .collect::<Option<Vec<_>>>();
                    host[k] = json!(vals
                        .filter(|v| !v.is_empty())
                        .map(|v| round(v.iter().sum(), 2)))
                }
                if self.interfaces.is_empty() {
                    errors.push(
                        "network: no physical/default interface; configure network_interfaces"
                            .into(),
                    )
                }
            }
            Err(e) => {
                self.interfaces.clear();
                errors.push(format!("network: {e}"))
            }
        }
        match fs::read_to_string(self.proc.join("diskstats"))
            .map_err(anyhow::Error::from)
            .and_then(|s| parse_diskstats(&s))
            .and_then(|v| Ok((self.disks(c, &v)?, v)))
        {
            Ok((names, v)) => {
                self.devices = names;
                self.disk_rates = self
                    .devices
                    .iter()
                    .map(|n| {
                        (
                            n.clone(),
                            (
                                self.rates.rate(&format!("disk/{n}/0"), Some(v[n].0), mono),
                                self.rates.rate(&format!("disk/{n}/1"), Some(v[n].1), mono),
                            ),
                        )
                    })
                    .collect();
                for (i, k) in [(0, "disk_read_bps"), (1, "disk_write_bps")] {
                    let vals = self
                        .disk_rates
                        .values()
                        .map(|p| if i == 0 { p.0 } else { p.1 })
                        .collect::<Option<Vec<_>>>();
                    host[k] = json!(vals
                        .filter(|v| !v.is_empty())
                        .map(|v| round(v.iter().sum(), 2)))
                }
            }
            Err(e) => {
                self.devices.clear();
                self.disk_rates.clear();
                errors.push(format!("disk IO: {e}"))
            }
        }
        host["arc_bytes"] = json!(fs::read_to_string(self.proc.join("spl/kstat/zfs/arcstats"))
            .ok()
            .and_then(|s| arc_size(&s)));
        json!({"host":host,"error":if errors.is_empty(){None}else{Some(text(&errors.join("; "),180))}})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rates_reset() {
        let mut r = Rates::default();
        assert_eq!(r.rate("x", Some(100.), 1.), None);
        assert_eq!(r.rate("x", Some(200.), 3.), Some(50.));
        assert_eq!(r.rate("x", Some(50.), 4.), None);
        assert_eq!(r.rate("x", None, 5.), None);
        assert_eq!(r.rate("x", Some(60.), 6.), None)
    }
    #[test]
    fn cpu_excludes_guest() {
        let c = parse_cpu("cpu 100 0 10 80 10 0 0 0 99 88\n").unwrap();
        let old = vec![50, 0, 0, 40, 0, 0, 0, 0];
        assert_eq!(cpu_delta(&c["cpu"], Some(&old)), (Some(54.55), Some(9.09)))
    }
    #[test]
    fn memory_available() {
        assert_eq!(
            parse_mem("MemTotal: 100 kB\nMemAvailable: 25 kB\n").unwrap()["mem_used_bytes"],
            76800.
        );
        assert!(parse_mem("MemTotal: 10 kB\nMemAvailable: 25 kB\n").is_err())
    }
    #[test]
    fn sector_units() {
        assert_eq!(
            parse_diskstats("259 0 nvme0n1 2 0 8 0 1 0 12 0").unwrap()["nvme0n1"],
            (4096., 6144.)
        )
    }
}
