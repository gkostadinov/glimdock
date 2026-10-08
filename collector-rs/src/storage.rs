//! SMART exit-bit semantics and non-spinning ATA health checks.
use crate::{
    config::Config,
    probes::{command, num, text},
};
use serde_json::{json, Value};
use std::{fs, path::Path};
pub fn parse_smart(d: &Value, name: &str, code: i32) -> Value {
    let messages = d["smartctl"]["messages"]
        .as_array()
        .map(|m| {
            m.iter()
                .filter_map(|v| v["string"].as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let standby = code == 3
        || regex::Regex::new(r"(?i)STANDBY|SLEEP|low.power")
            .unwrap()
            .is_match(&messages);
    let passed = d["smart_status"]["passed"].as_bool();
    let status = if standby {
        "standby"
    } else if code & 7 != 0 {
        "error"
    } else {
        "active"
    };
    let mut health = if standby {
        None
    } else if passed == Some(false) || code & 8 != 0 {
        Some("failed")
    } else if passed == Some(true) {
        Some("passed")
    } else {
        Some("unknown")
    };
    let n = &d["nvme_smart_health_information_log"];
    let attributes = d["ata_smart_attributes"]["table"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let attr = |id: u64| {
        attributes
            .iter()
            .find(|a| a["id"].as_u64() == Some(id))
            .and_then(|a| num(&a["raw"]["value"]))
    };
    let temp = num(&d["temperature"]["current"])
        .or_else(|| num(&n["temperature"]))
        .or_else(|| attr(190))
        .or_else(|| attr(194))
        .filter(|v| (-50.0..=150.0).contains(v));
    let mut warnings = vec![];
    if !standby {
        if code & 16 != 0 {
            warnings.push("SMART prefail attribute below threshold")
        } else if code & (32 | 64 | 128) != 0 {
            warnings.push("SMART historical errors; inspect smartctl")
        };
        if num(&n["critical_warning"]).is_some_and(|v| v != 0.) {
            warnings.insert(0, "NVMe critical warning");
            health = Some("failed")
        };
        if num(&n["percentage_used"]).is_some_and(|v| v >= 100.) {
            warnings.push("NVMe endurance estimate exhausted")
        };
        if num(&n["media_errors"]).is_some_and(|v| v != 0.) {
            warnings.push("NVMe media errors recorded")
        }
    }
    let optional = |v: Option<f64>| if standby { None } else { v };
    json!({"name":text(name,64),"model":text(d["model_name"].as_str().or(d["product"].as_str()).unwrap_or(""),64),"temp_c":optional(temp),"health":health,"status":status,"read_bps":null,"write_bps":null,"warnings":warnings.into_iter().take(3).collect::<Vec<_>>(),"wear_pct":optional(num(&n["percentage_used"])),"media_errors":optional(num(&n["media_errors"])),"power_on_hours":optional(num(&d["power_on_time"]["hours"]).or_else(||num(&n["power_on_hours"]))),"spare_pct":optional(num(&n["available_spare"])),"reallocated_sectors":optional(attr(5)),"pending_sectors":optional(attr(197))})
}
pub fn read_smart(c: &Config) -> anyhow::Result<Value> {
    let devices = if c.smart_devices.is_empty() {
        let r = command("smartctl", &["--scan", "-j"], 5., false)?;
        serde_json::from_str::<Value>(&r.stdout)?["devices"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        c.smart_devices
            .iter()
            .map(|v| json!({"name":if v.starts_with("/dev/"){v.clone()}else{format!("/dev/{v}")}}))
            .collect()
    };
    let count = devices.len();
    let mut disks = vec![];
    let mut errors = vec![];
    let controller = regex::Regex::new(r"^nvme\d+$").unwrap();
    for d in devices.iter().take(16) {
        let Some(name) = d["name"].as_str() else {
            continue;
        };
        let base = Path::new(name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let kind = d["type"].as_str();
        let mut args = vec!["-j", "-a", name];
        if kind != Some("nvme") && !base.starts_with("nvme") {
            args.extend(["-n", "standby,3"])
        }
        if let Some(kind) = kind {
            args.extend(["-d", kind])
        }
        match command("smartctl", &args, 5., true)
            .and_then(|r| Ok((serde_json::from_str::<Value>(&r.stdout)?, r.status)))
        {
            Ok((data, code)) => {
                let disk = parse_smart(&data, &base, code);
                if disk["status"] == "error" {
                    errors.push(format!("{base}: SMART read exit {code}"))
                }
                let ns = if controller.is_match(&base) {
                    let re =
                        regex::Regex::new(&format!(r"^{}n\d+$", regex::escape(&base))).unwrap();
                    fs::read_dir("/sys/block")
                        .ok()
                        .into_iter()
                        .flatten()
                        .filter_map(Result::ok)
                        .filter_map(|e| {
                            let n = e.file_name().to_string_lossy().into_owned();
                            re.is_match(&n).then_some(n)
                        })
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                };
                if ns.is_empty() {
                    disks.push(disk)
                } else {
                    for name in ns {
                        let mut row = disk.clone();
                        row["name"] = json!(name);
                        disks.push(row)
                    }
                }
            }
            Err(_) => {
                errors.push(format!("{base}: SMART read unavailable"));
                disks.push(json!({"name":text(&base,64),"model":"","temp_c":null,"health":null,"status":"error","read_bps":null,"write_bps":null}))
            }
        }
    }
    Ok(
        json!({"disks_total":count.max(disks.len()),"disks":disks,"error":if errors.is_empty(){None}else{Some(text(&errors.join("; "),180))}}),
    )
}
pub fn parse_zfs(input: &str) -> Value {
    let pools=input.lines().filter_map(|line|{let f=line.split_whitespace().collect::<Vec<_>>();(f.len()>=4).then(||json!({"name":text(f[0],64),"total_bytes":num(&json!(f[1])),"used_bytes":num(&json!(f[2])),"status":text(f[3],24)}))}).collect::<Vec<_>>();
    json!({"pools":pools})
}
pub fn read_zfs() -> anyhow::Result<Value> {
    Ok(parse_zfs(
        &command(
            "zpool",
            &["list", "-H", "-p", "-o", "name,size,alloc,health"],
            5.,
            false,
        )?
        .stdout,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standby_unknown() {
        let v = parse_smart(
            &json!({"smart_status":{"passed":true},"temperature":{"current":30},"nvme_smart_health_information_log":{"percentage_used":100}}),
            "sda",
            3,
        );
        assert_eq!(v["status"], "standby");
        assert!(v["health"].is_null());
        assert!(v["temp_c"].is_null());
        assert!(v["wear_pct"].is_null())
    }
    #[test]
    fn critical_bit_and_temperature() {
        let v = parse_smart(
            &json!({"smart_status":{"passed":true},"temperature":{"current":65261},"nvme_smart_health_information_log":{"critical_warning":1}}),
            "nvme",
            0,
        );
        assert_eq!(v["health"], "failed");
        assert!(v["temp_c"].is_null());
        assert_eq!(v["warnings"][0], "NVMe critical warning")
    }
    #[test]
    fn pools_exact_bytes() {
        let v = parse_zfs("tank\t100000\t25000\tONLINE\n");
        assert_eq!(v["pools"][0]["total_bytes"], 100000.);
        assert_eq!(v["pools"][0]["used_bytes"], 25000.)
    }
}
