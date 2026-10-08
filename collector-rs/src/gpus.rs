//! PCI identities and VM passthrough ownership; unsupported engines stay null.
use crate::{
    guests::{parse_nvidia_csv, validate_gpu_metrics, GPU_METRICS, NVIDIA_FIELDS},
    probes::{command, num, text, wall},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
pub fn pci_id(value: &str) -> Option<String> {
    let re =
        regex::Regex::new(r"(?i)(?:([0-9a-f]{4,8}):)?([0-9a-f]{2}):([0-9a-f]{2})(?:\.([0-7]))?")
            .unwrap();
    let m = re.captures(value)?;
    let dom = m.get(1).map(|m| m.as_str()).unwrap_or("0000");
    Some(
        format!(
            "{}:{}:{}.{}",
            &dom[dom.len().saturating_sub(4)..],
            &m[2],
            &m[3],
            m.get(4).map(|m| m.as_str()).unwrap_or("0")
        )
        .to_lowercase(),
    )
}
pub fn gpu_kind(vendor: &str, name: &str, device: &str) -> &'static str {
    let matches = |pattern: &str| regex::Regex::new(pattern).unwrap().is_match(name);
    if matches(r"(?i)Arc|DG[12]") {
        "discrete"
    } else if vendor == "8086"
        && (device == "a780" || matches(r"(?i)UHD|HD Graphics|Iris|GT[123]|Raptor|Alder"))
    {
        "integrated"
    } else if vendor == "10de" || matches(r"(?i)GeForce|Quadro|RTX|Radeon RX|Navi|Arc|DG[12]") {
        "discrete"
    } else if matches(r"(?i)Raphael|Renoir|Cezanne|APU") {
        "integrated"
    } else {
        "unknown"
    }
}
pub fn ownership(root: &Path) -> BTreeMap<String, String> {
    let mut owners = BTreeMap::new();
    let re = regex::Regex::new(r"^hostpci\d+:\s*").unwrap();
    for entry in fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.extension().is_none_or(|v| v != "conf") {
            continue;
        };
        let Some(id) = path
            .file_stem()
            .and_then(|v| v.to_str())
            .and_then(|v| v.parse::<u64>().ok())
        else {
            continue;
        };
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines().filter(|line| re.is_match(line)) {
            for token in line
                .split_once(':')
                .unwrap()
                .1
                .split(',')
                .next()
                .unwrap_or("")
                .split(';')
            {
                if let Some(slot) = pci_id(token) {
                    owners.insert(slot, format!("vm:{id}"));
                }
            }
        }
    }
    owners
}
fn file_number(path: impl AsRef<Path>, scale: f64) -> Option<f64> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| num(&json!(s.trim())))
        .map(|v| v * scale)
}
fn directories(path: impl AsRef<Path>) -> Vec<PathBuf> {
    let mut out = fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect::<Vec<_>>();
    out.sort();
    out
}
pub fn read_gpus() -> anyhow::Result<Value> {
    let mut names = BTreeMap::new();
    if let Ok(r) = command("lspci", &["-D", "-mm"], 5., false) {
        let re = regex::Regex::new(r#"^([^ ]+)\s+"[^"]*"\s+"[^"]*"\s+"([^"]*)""#).unwrap();
        for line in r.stdout.lines() {
            if let Some(m) = re.captures(line) {
                if let Some(id) = pci_id(&m[1]) {
                    names.insert(id, m[2].to_owned());
                }
            }
        }
    }
    let owners = ownership(Path::new("/etc/pve/qemu-server"));
    let now = wall();
    let mut result = vec![];
    for path in directories("/sys/bus/pci/devices") {
        let Ok(class) = fs::read_to_string(path.join("class")) else {
            continue;
        };
        if !class.starts_with("0x03") {
            continue;
        };
        let Ok(vendor) = fs::read_to_string(path.join("vendor")) else {
            continue;
        };
        let Ok(device) = fs::read_to_string(path.join("device")) else {
            continue;
        };
        let vendor = vendor.trim().trim_start_matches("0x");
        let device = device.trim().trim_start_matches("0x");
        if ["1234", "1af4", "15ad", "1414", "1b36"].contains(&vendor) {
            continue;
        };
        let Some(slot) = pci_id(&path.file_name().unwrap_or_default().to_string_lossy()) else {
            continue;
        };
        let vendor_name = match vendor {
            "8086" => "Intel",
            "10de" => "NVIDIA",
            "1002" => "AMD",
            v => v,
        };
        let fallback = format!("{vendor_name} GPU [{vendor}:{device}]");
        let name = names.get(&slot).unwrap_or(&fallback);
        let driver = fs::canonicalize(path.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let owner = owners.get(&slot).map(String::as_str).unwrap_or("host");
        let mut gpu = json!({"id":format!("pci:{slot}"),"name":text(name,96),"vendor":vendor_name,"kind":gpu_kind(vendor,name,device),"owner":owner,"driver":text(&driver,48),"status":"inventory","updated_at":now,"age_s":0,"error":null,"_vendor_id":vendor,"_device_id":device,"utilization_kind":"gpu"});
        for k in GPU_METRICS {
            gpu[k] = Value::Null
        }
        if owner == "host" && driver != "vfio-pci" && driver != "vfio_pci" {
            for (field, file) in [
                ("utilization_pct", "gpu_busy_percent"),
                ("mem_used_bytes", "mem_info_vram_used"),
                ("mem_total_bytes", "mem_info_vram_total"),
            ] {
                gpu[field] = json!(file_number(path.join(file), 1.))
            }
            for drm in directories(path.join("drm")) {
                for clock in ["gt/gt0/rps_act_freq_mhz", "gt_cur_freq_mhz"] {
                    if let Some(v) = file_number(drm.join(clock), 1.) {
                        gpu["graphics_mhz"] = json!(v);
                        break;
                    }
                }
            }
            for hwmon in directories(path.join("hwmon")) {
                if gpu["temp_c"].is_null() {
                    gpu["temp_c"] = json!(file_number(hwmon.join("temp1_input"), 0.001))
                }
                if gpu["power_w"].is_null() {
                    gpu["power_w"] = json!(file_number(hwmon.join("power1_average"), 0.000001))
                }
                if let (Some(pwm), Some(max)) = (
                    file_number(hwmon.join("pwm1"), 1.),
                    file_number(hwmon.join("pwm1_max"), 1.),
                ) {
                    if max > 0. {
                        gpu["fan_pct"] = json!(pwm / max * 100.)
                    }
                }
            }
            if GPU_METRICS.iter().any(|k| !gpu[*k].is_null()) {
                gpu["status"] = json!("partial")
            }
        }
        validate_gpu_metrics(&mut gpu);
        result.push(gpu)
    }
    if result
        .iter()
        .any(|g| g["owner"] == "host" && g["vendor"] == "NVIDIA")
    {
        let query = format!("--query-gpu={NVIDIA_FIELDS}");
        match command(
            "nvidia-smi",
            &[&query, "--format=csv,noheader,nounits"],
            5.,
            false,
        ) {
            Ok(r) => {
                for row in parse_nvidia_csv(&r.stdout) {
                    let id = row["pci_bus"]
                        .as_str()
                        .and_then(pci_id)
                        .map(|v| format!("pci:{v}"));
                    for gpu in &mut result {
                        if gpu["owner"] == "host" && id.as_deref() == gpu["id"].as_str() {
                            for key in GPU_METRICS {
                                gpu[key] = row[key].clone()
                            }
                            gpu["status"] = json!("active");
                            gpu["error"] = Value::Null
                        }
                    }
                }
            }
            Err(_) => {
                for gpu in &mut result {
                    if gpu["owner"] == "host" && gpu["vendor"] == "NVIDIA" {
                        gpu["error"] = json!("NVIDIA metrics unavailable")
                    }
                }
            }
        }
    }
    Ok(json!({"gpus":result,"error":null}))
}
pub fn merge_gpus(
    inventory: &[Value],
    data: &BTreeMap<u64, Value>,
    guests: &[Value],
    states: &Value,
    now: f64,
) -> Vec<Value> {
    inventory
        .iter()
        .map(|original| {
            let mut g = original.clone();
            g["age_s"] = json!(num(&g["updated_at"]).map(|t| (now - t).max(0.)));
            if let Some(id) = g["owner"]
                .as_str()
                .and_then(|o| o.strip_prefix("vm:"))
                .and_then(|s| s.parse::<u64>().ok())
            {
                let key = format!("guest_{id}");
                let state = &states[&key];
                let running = guests
                    .iter()
                    .any(|v| v["id"].as_u64() == Some(id) && v["status"] == "running");
                let sample = data.get(&id).filter(|_| running && state["ok"] == true);
                let matched = sample.and_then(|s| {
                    let cards = s["gpus"].as_array()?;
                    let matched = cards
                        .iter()
                        .filter(|card| {
                            card["vendor_id"] == g["_vendor_id"]
                                && card["device_id"] == g["_device_id"]
                        })
                        .collect::<Vec<_>>();
                    (matched.len() == 1).then_some(matched[0])
                });
                if let (Some(sample), Some(card)) = (sample, matched) {
                    for k in GPU_METRICS {
                        g[k] = card[k].clone()
                    }
                    g["driver"] = json!(text(
                        card["driver"]
                            .as_str()
                            .or(g["driver"].as_str())
                            .unwrap_or(""),
                        48
                    ));
                    g["updated_at"] = sample["generated_at"].clone();
                    g["age_s"] = json!(num(&sample["generated_at"]).map(|t| (now - t).max(0.)));
                    g["error"] = card["error"].clone();
                    g["utilization_kind"] =
                        json!(card["utilization_kind"].as_str().unwrap_or("gpu"));
                    g["status"] = json!(if !g["error"].is_null() {
                        "partial"
                    } else if GPU_METRICS.iter().any(|k| !g[*k].is_null()) {
                        "active"
                    } else {
                        "inventory"
                    })
                } else {
                    for k in GPU_METRICS {
                        g[k] = Value::Null
                    }
                    g["status"] = json!("unavailable");
                    g["error"] = json!(if !running {
                        "Owning guest is stopped/unavailable"
                    } else if state["ok"] != true {
                        state["error"]
                            .as_str()
                            .unwrap_or("Guest GPU telemetry unavailable")
                    } else {
                        "Guest GPU identity missing/ambiguous"
                    });
                    g["updated_at"] = state["updated_at"].clone();
                    g["age_s"] = state["age_s"].clone()
                }
            }
            g.as_object_mut()
                .unwrap()
                .retain(|k, _| !k.starts_with('_'));
            g
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pci_and_kind() {
        assert_eq!(pci_id("00000000:01:00.0"), Some("0000:01:00.0".into()));
        assert_eq!(gpu_kind("8086", "Raptor Lake UHD", "a780"), "integrated");
        assert_eq!(gpu_kind("8086", "Intel Arc A770", "56a0"), "discrete");
        assert_eq!(gpu_kind("1002", "unidentified", "abcd"), "unknown")
    }
    #[test]
    fn passthrough_guest_identity_not_pci_bus() {
        let inv = json!({"id":"pci:0000:01:00.0","owner":"vm:100","_vendor_id":"10de","_device_id":"2484","driver":"vfio-pci"});
        let data = BTreeMap::from([(
            100,
            json!({"generated_at":100,"gpus":[{"vendor_id":"10de","device_id":"2484","pci_bus":"0000:05:00.0","utilization_pct":20,"power_w":70,"error":null}]}),
        )]);
        let states = json!({"guest_100":{"ok":true}});
        let r = merge_gpus(
            std::slice::from_ref(&inv),
            &data,
            &[json!({"id":100,"status":"running"})],
            &states,
            100.,
        );
        assert_eq!(r[0]["power_w"], 70);
        assert_eq!(r[0]["status"], "active");
        assert!(r[0].get("_vendor_id").is_none());
        let r = merge_gpus(
            &[inv],
            &data,
            &[json!({"id":100,"status":"stopped"})],
            &states,
            100.,
        );
        assert!(r[0]["power_w"].is_null());
        assert_eq!(r[0]["status"], "unavailable")
    }
}
