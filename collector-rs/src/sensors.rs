//! hwmon and RAPL values retain physical units and component scope.
use crate::{
    config::Config,
    probes::{num, round, text},
};
use serde_json::{json, Value};
/// Linux exposes hwmon readings even without the optional lm-sensors tool.
pub fn read_hwmon(root: &std::path::Path) -> anyhow::Result<Vec<Value>> {
    let mut data = json!({});
    let re = regex::Regex::new(r"^(temp|fan|in|power|curr)(\d+)_(input|average|max|crit|alarm|crit_alarm|max_alarm|fault)$").unwrap();
    for chip in std::fs::read_dir(root)?.flatten().take(128) {
        let path = chip.path();
        let read = |name: &str| {
            crate::config::read_bounded(&path.join(name), 256)
                .ok()
                .and_then(|v| String::from_utf8(v).ok())
                .map(|s| s.trim().to_string())
        };
        let Some(name) = read("name") else { continue };
        let identity = format!("{}-{}", text(&name, 48), chip.file_name().to_string_lossy());
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for file in entries.flatten().take(512) {
            let filename = file.file_name().to_string_lossy().into_owned();
            let Some(m) = re.captures(&filename) else {
                continue;
            };
            let Some(value) = read(&filename)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|n| n.is_finite())
            else {
                continue;
            };
            let base = format!("{}{}", &m[1], &m[2]);
            let label = read(&format!("{base}_label"))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| base.clone());
            let scale = if matches!(&m[3], "input" | "average" | "max" | "crit") {
                match &m[1] {
                    "temp" | "in" | "curr" => 1000.,
                    "power" => 1_000_000.,
                    _ => 1.,
                }
            } else {
                1.
            };
            data[&identity][&label][filename] = json!(value / scale);
        }
    }
    Ok(parse_sensors(&data))
}
pub fn parse_sensors(data: &Value) -> Vec<Value> {
    let mut result = vec![];
    let re = regex::Regex::new(r"^(temp|fan|in|power|curr)(\d+)_(input|average)$").unwrap();
    if let Some(chips) = data.as_object() {
        for (chip, features) in chips {
            let Some(features) = features.as_object() else {
                continue;
            };
            for (label, readings) in features {
                let Some(readings) = readings.as_object() else {
                    continue;
                };
                for (field, raw) in readings {
                    let Some(m) = re.captures(field) else {
                        continue;
                    };
                    let base = format!("{}{}", &m[1], &m[2]);
                    if &m[3] == "average" && readings.contains_key(&format!("{base}_input")) {
                        continue;
                    };
                    let Some(value) = num(raw) else { continue };
                    let (kind, unit) = match &m[1] {
                        "temp" => ("temperature", "°C"),
                        "fan" => ("fan", "RPM"),
                        "in" => ("voltage", "V"),
                        "power" => ("power", "W"),
                        _ => ("current", "A"),
                    };
                    let mut critical = readings.get(&format!("{base}_crit")).and_then(num);
                    let mut high = if chip.starts_with("nct6687") {
                        None
                    } else {
                        readings.get(&format!("{base}_max")).and_then(num)
                    };
                    if kind == "temperature" {
                        critical = critical.filter(|v| (-40.0..=200.0).contains(v));
                        high = high.filter(|v| (-40.0..=200.0).contains(v));
                        if matches!((high,critical),(Some(a),Some(b)) if a>=b) {
                            high = None
                        }
                    }
                    let alarm = ["alarm", "crit_alarm", "max_alarm"]
                        .iter()
                        .any(|s| readings.get(&format!("{base}_{s}")).and_then(num) == Some(1.));
                    let fault = readings.get(&format!("{base}_fault")).and_then(num) == Some(1.);
                    result.push(json!({"id":text(&format!("{chip}/{base}"),96),"name":text(label,64),"chip":text(chip,64),"kind":kind,"value":round(value,3),"unit":unit,"crit":critical,"high":high,"alarm":alarm,"fault":fault}));
                }
            }
        }
    }
    result.sort_by_key(|s| {
        (
            s["kind"] != "temperature",
            s["chip"].as_str().unwrap_or("").to_owned(),
            s["name"].as_str().unwrap_or("").to_owned(),
        )
    });
    result
}
pub fn parse_turbostat(input: &str) -> anyhow::Result<Value> {
    let re = regex::Regex::new(r"^(?:CPU%c\d+|Pkg%pc\d+|Pk%pc\d+|C\d+[A-Za-z]*%)$").unwrap();
    let mut header = vec![];
    for line in input.lines() {
        let f = line.split_whitespace().collect::<Vec<_>>();
        if f.iter()
            .any(|v| ["Avg_MHz", "Bzy_MHz", "PkgWatt"].contains(v))
        {
            header = f;
            continue;
        }
        if !header.is_empty()
            && f.len() == header.len()
            && f.iter().any(|v| num(&json!(v)).is_some())
        {
            let row = header
                .iter()
                .zip(&f)
                .map(|(k, v)| ((*k).to_owned(), json!(v)))
                .collect::<serde_json::Map<_, _>>();
            let get = |k: &str| row.get(k).and_then(num);
            let c = row
                .iter()
                .filter(|(k, v)| re.is_match(k) && num(v).is_some())
                .map(|(k, v)| (k.clone(), json!(num(v))))
                .collect::<serde_json::Map<_, _>>();
            return Ok(
                json!({"package_w":get("PkgWatt"),"cores_w":get("CorWatt"),"graphics_w":get("GFXWatt"),"cpu_mhz":get("Avg_MHz"),"busy_mhz":get("Bzy_MHz"),"cpu_temp_c":get("PkgTmp"),"cstate_pct":c}),
            );
        }
    }
    anyhow::bail!("No turbostat summary row")
}
pub fn merge_power_sensors(snapshot: &Value, c: &Config) -> Vec<Value> {
    let mut rows = snapshot["sensors"].as_array().cloned().unwrap_or_default();
    rows.retain(|s| {
        !s["id"].as_str().unwrap_or("").starts_with("telemetry/cpu/")
            && !s["id"].as_str().unwrap_or("").starts_with("telemetry/gpu/")
    });
    let fresh = |state: &Value, ttl: f64| {
        state["ok"] == true
            && state["enabled"] != false
            && num(&state["age_s"]).is_some_and(|v| v >= 0. && v <= ttl)
    };
    let mut add = |id: String,
                   name: String,
                   chip: String,
                   value: &Value,
                   source: String,
                   state: &Value,
                   scope: &str| {
        if rows.iter().any(|r| r["id"] == id) {
            return;
        };
        if let Some(v) = num(value).filter(|v| *v >= 0.) {
            rows.push(json!({"id":text(&id,96),"name":text(&name,64),"chip":text(&chip,64),"kind":"power","value":round(v,3),"unit":"W","crit":null,"high":null,"alarm":false,"fault":false,"source":source,"updated_at":state["updated_at"],"age_s":state["age_s"],"error":null,"scope":scope}))
        }
    };
    let state = &snapshot["sources"]["turbostat"];
    if fresh(state, (c.turbostat_interval_s * 3.).max(20.)) {
        for (k, name, scope) in [
            ("package_w", "CPU package", "cpu-package"),
            ("cores_w", "CPU cores", "cpu-cores"),
            ("graphics_w", "CPU graphics domain", "cpu-graphics"),
        ] {
            add(
                format!("telemetry/cpu/{k}"),
                name.into(),
                "turbostat / RAPL".into(),
                &snapshot["power"][k],
                "turbostat".into(),
                state,
                scope,
            )
        }
    }
    if fresh(
        &snapshot["sources"]["gpus"],
        (c.gpu_interval_s * 3.).max(90.),
    ) {
        if let Some(gpus) = snapshot["gpus"].as_array() {
            let re = regex::Regex::new(r"\[([^\]]+)\]").unwrap();
            for gpu in gpus {
                if gpu["status"] != "active" || !gpu["error"].is_null() {
                    continue;
                };
                let owner = gpu["owner"].as_str().unwrap_or("host");
                let source = owner
                    .strip_prefix("vm:")
                    .map(|v| format!("guest_{v}"))
                    .unwrap_or_else(|| "gpus".into());
                let limit = if source == "gpus" {
                    (c.gpu_interval_s * 3.).max(90.)
                } else {
                    (c.guest_interval_s * 3.).max(45.)
                };
                let state = &snapshot["sources"][&source];
                if !fresh(state, limit)
                    || !num(&gpu["age_s"]).is_some_and(|v| v >= 0. && v <= limit)
                {
                    continue;
                };
                let raw = gpu["name"].as_str().unwrap_or("GPU");
                let name = re
                    .captures(raw)
                    .map(|m| m[1].to_owned())
                    .unwrap_or_else(|| raw.to_owned())
                    .replace("NVIDIA GeForce ", "")
                    .replace("GeForce ", "");
                add(
                    format!(
                        "telemetry/gpu/{}/power",
                        gpu["id"].as_str().unwrap_or("gpu")
                    ),
                    format!("{name} board"),
                    format!("{} / {owner}", gpu["vendor"].as_str().unwrap_or("GPU")),
                    &gpu["power_w"],
                    source,
                    gpu,
                    "gpu-board",
                )
            }
        }
    }
    rows
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sysfs_hwmon_preserves_physical_units_labels_limits_and_flags() {
        let dir = tempfile::tempdir().unwrap();
        let chip = dir.path().join("hwmon0");
        std::fs::create_dir(&chip).unwrap();
        for (name, value) in [
            ("name", "coretemp"),
            ("temp1_input", "42000"),
            ("temp1_label", "CPU Package"),
            ("temp1_max", "80000"),
            ("temp1_crit", "90000"),
            ("temp1_alarm", "1"),
            ("fan1_input", "1800"),
            ("in1_input", "12000"),
            ("curr1_input", "2100"),
            ("power1_input", "25000000"),
            ("power1_average", "20000000"),
            ("temp2_input", "NaN"),
        ] {
            std::fs::write(chip.join(name), value).unwrap();
        }
        let rows = read_hwmon(dir.path()).unwrap();
        assert_eq!(rows.len(), 5);
        let cpu = rows.iter().find(|r| r["kind"] == "temperature").unwrap();
        assert_eq!(cpu["name"], "CPU Package");
        assert_eq!(cpu["value"], 42.);
        assert_eq!(cpu["high"], 80.);
        assert_eq!(cpu["crit"], 90.);
        assert_eq!(cpu["alarm"], true);
        assert!(rows.iter().any(|r| r["unit"] == "V" && r["value"] == 12.));
        assert!(rows.iter().any(|r| r["unit"] == "A" && r["value"] == 2.1));
        assert!(rows.iter().any(|r| r["unit"] == "W" && r["value"] == 25.));
        assert!(rows
            .iter()
            .any(|r| r["unit"] == "RPM" && r["value"] == 1800.));
    }
    #[test]
    fn labels_thresholds_and_units() {
        let v = json!({"nct6687-isa":{"CPU":{"temp1_input":55,"temp1_max":55}},"nvme-pci":{"Composite":{"temp1_input":40,"temp1_max":65261,"temp1_crit":85}},"chip":{"Rail":{"curr1_input":2.1,"power1_average":16,"power1_input":15}}});
        let r = parse_sensors(&v);
        assert_eq!(r.len(), 4);
        assert!(r.iter().find(|v| v["chip"] == "nct6687-isa").unwrap()["high"].is_null());
        let nv = r.iter().find(|v| v["chip"] == "nvme-pci").unwrap();
        assert!(nv["high"].is_null());
        assert_eq!(nv["crit"], 85.);
        assert!(r.iter().any(|v| v["unit"] == "A"));
        assert!(r.iter().any(|v| v["value"] == 15.))
    }
    #[test]
    fn turbo_summary() {
        let v=parse_turbostat("noise\nAvg_MHz Bzy_MHz PkgWatt CorWatt GFXWatt PkgTmp CPU%c6\n1000 3000 25.5 20 2 55 84\n").unwrap();
        assert_eq!(v["package_w"], 25.5);
        assert_eq!(v["cstate_pct"]["CPU%c6"], 84.);
        assert!(parse_turbostat("unsupported").is_err())
    }
}

#[cfg(test)]
mod power_tests {
    use super::*;
    #[test]
    fn failure_clears_derived_power() {
        let c = Config::default();
        let mut s = json!({"sensors":[],"sources":{"turbostat":{"ok":true,"enabled":true,"age_s":1,"updated_at":10},"gpus":{"ok":false}},"power":{"package_w":25,"cores_w":20,"graphics_w":null},"gpus":[]});
        let rows = merge_power_sensors(&s, &c);
        assert_eq!(rows.len(), 2);
        assert!(rows
            .iter()
            .all(|r| r["unit"] == "W" && r["scope"].as_str().unwrap().starts_with("cpu-")));
        s["sensors"] = json!(rows);
        s["sources"]["turbostat"]["ok"] = json!(false);
        assert!(merge_power_sensors(&s, &c).is_empty());
        s["sources"]["turbostat"]["ok"] = json!(true);
        s["sources"]["turbostat"]["age_s"] = json!(30);
        assert!(merge_power_sensors(&s, &c).is_empty());
    }
    #[test]
    fn absent_current_never_derived_from_watts() {
        let c = Config::default();
        let s = json!({"sensors":[],"sources":{"turbostat":{"ok":true,"age_s":0}},"power":{"package_w":20},"gpus":[]});
        assert!(merge_power_sensors(&s, &c)
            .iter()
            .all(|r| r["kind"] != "current"));
    }
}
