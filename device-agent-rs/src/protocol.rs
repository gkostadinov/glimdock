//! The display contract and counter deltas shared by all native adapters.
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn text(value: &str, cap: usize) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(cap)
        .collect()
}
pub fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| n.is_finite())
}
pub fn source_status(now: f64, error: Option<&str>, enabled: bool) -> Value {
    json!({"enabled":enabled,"ok":enabled && error.is_none(),"updated_at":if enabled && error.is_none(){Some(now)}else{None},"age_s":if enabled && error.is_none(){Some(0.)}else{None},"error":error})
}
pub fn empty_snapshot(descriptor: &Value, sequence: u64, now: f64) -> Value {
    let mut host =
        json!({"name":descriptor["name"],"ip":descriptor["address"],"cpu_cores":[],"load":[]});
    for field in crate::HOST_FIELDS {
        host[*field] = Value::Null;
    }
    let mut power = json!({"cstate_pct":{}});
    for field in crate::POWER_FIELDS {
        power[*field] = Value::Null;
    }
    json!({"schema":1,"sequence":sequence,"generated_at":now,"host":host,"power":power,"guests":[],"storage":[],"disks":[],"sensors":[],"gpus":[],"alerts":[],"sources":{},"platform":{},"limits":{"counts":{},"truncated":{},"network_interfaces":[],"disk_devices":[]},"faults":{"lookback_days":7,"segfault_count_24h":null,"event_count_24h":null,"last_event_at":null,"events":[]}})
}
pub fn snapshot_status(snapshot: &Value) -> &'static str {
    let sources = snapshot["sources"]
        .as_object()
        .map(|v| {
            v.values()
                .filter(|s| s["enabled"] != false)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if sources.is_empty() || !sources.iter().any(|s| s["ok"] == true) {
        return if sources.iter().any(|s| number(&s["updated_at"]).is_some()) {
            "degraded"
        } else {
            "offline"
        };
    }
    if snapshot["alerts"].as_array().is_some_and(|a| !a.is_empty())
        || sources.iter().any(|s| s["ok"] != true)
    {
        "degraded"
    } else {
        "healthy"
    }
}
fn capacity_alert(snapshot: &mut Value) {
    let alerts = snapshot["alerts"].as_array_mut().unwrap();
    if !alerts.iter().any(|a| a["id"] == "display/truncated") {
        alerts.insert(0, json!({"id":"display/truncated","severity":"warning","message":"Display capacity reached; some items omitted"}));
        alerts.truncate(24);
    }
}
pub fn finish_snapshot(mut snapshot: Value, descriptor: &Value) -> Result<Value> {
    const CAPS: &[(&str, usize)] = &[
        ("sensors", 64),
        ("storage", 16),
        ("disks", 16),
        ("gpus", 8),
        ("guests", 48),
    ];
    for field in ["limits", "sources", "platform"] {
        if !snapshot[field].is_object() {
            snapshot[field] = json!({});
        }
    }
    for field in ["counts", "truncated"] {
        if !snapshot["limits"][field].is_object() {
            snapshot["limits"][field] = json!({});
        }
    }
    if !snapshot["alerts"].is_array() {
        snapshot["alerts"] = json!([]);
    }
    snapshot["alerts"].as_array_mut().unwrap().truncate(24);
    for (field, cap) in CAPS {
        if !snapshot[*field].is_array() {
            snapshot[*field] = json!([]);
        }
        let actual = snapshot[*field].as_array().unwrap().len();
        let count = snapshot["limits"]["counts"][*field]
            .as_u64()
            .unwrap_or(0)
            .max(actual as u64);
        snapshot[*field].as_array_mut().unwrap().truncate(*cap);
        snapshot["limits"]["counts"][*field] = json!(count);
        snapshot["limits"]["truncated"][*field] =
            json!(count.saturating_sub(actual.min(*cap) as u64));
        snapshot["limits"][format!("max_{field}")] = json!(cap);
    }
    for (parent, field, count_key, cap) in [
        ("host", "cpu_cores", "cpu_cores", 64),
        ("platform", "interfaces", "network_interfaces", 16),
    ] {
        let actual = snapshot[parent][field].as_array().map_or(0, Vec::len);
        let count = snapshot["limits"]["counts"][count_key]
            .as_u64()
            .unwrap_or(0)
            .max(actual as u64);
        if let Some(items) = snapshot[parent][field].as_array_mut() {
            items.truncate(cap);
        }
        snapshot["limits"]["counts"][count_key] = json!(count);
        snapshot["limits"]["truncated"][count_key] =
            json!(count.saturating_sub(actual.min(cap) as u64));
    }
    if let Some(names) = snapshot["limits"]["network_interfaces"].as_array_mut() {
        names.truncate(16);
    }
    if let Some(events) = snapshot["faults"]["events"].as_array_mut() {
        events.truncate(16);
    }
    snapshot["limits"]["max_payload_bytes"] = json!(crate::MAX_PAYLOAD);
    if snapshot["limits"]["truncated"]
        .as_object()
        .unwrap()
        .values()
        .any(|v| v.as_u64().unwrap_or(0) > 0)
    {
        capacity_alert(&mut snapshot);
    }
    snapshot["node"] = descriptor.clone();
    snapshot["node"]["status"] = json!(snapshot_status(&snapshot));
    snapshot["nodes"] = json!([snapshot["node"].clone()]);
    while serde_json::to_vec(&snapshot)?.len() > crate::MAX_PAYLOAD {
        let largest = CAPS
            .iter()
            .filter(|(f, _)| snapshot[*f].as_array().is_some_and(|a| !a.is_empty()))
            .max_by_key(|(f, _)| serde_json::to_vec(&snapshot[*f]).map_or(0, |v| v.len()))
            .map(|(f, _)| *f);
        if let Some(field) = largest {
            snapshot[field].as_array_mut().unwrap().pop();
            let count = snapshot["limits"]["truncated"][field].as_u64().unwrap_or(0);
            snapshot["limits"]["truncated"][field] = json!(count + 1);
            capacity_alert(&mut snapshot);
            snapshot["node"]["status"] = json!(snapshot_status(&snapshot));
            snapshot["nodes"] = json!([snapshot["node"].clone()]);
        } else {
            bail!("Base telemetry exceeds display capacity");
        }
    }
    Ok(snapshot)
}

#[derive(Default)]
pub struct Rates {
    previous: BTreeMap<String, (u64, f64)>,
}
impl Rates {
    pub fn rate(&mut self, key: &str, counter: Option<u64>, now: f64) -> Option<f64> {
        let Some(counter) = counter else {
            self.previous.remove(key);
            return None;
        };
        let old = self.previous.insert(key.into(), (counter, now));
        old.and_then(|(before, t)| {
            counter
                .checked_sub(before)
                .filter(|_| now > t)
                .map(|delta| delta as f64 / (now - t))
        })
        .filter(|v| v.is_finite())
    }
    pub fn clear(&mut self) {
        self.previous.clear();
    }
    pub fn clear_prefix(&mut self, prefix: &str) {
        self.previous.retain(|k, _| !k.starts_with(prefix));
    }
}
