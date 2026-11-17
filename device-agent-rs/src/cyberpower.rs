//! Read-only CPS-MIB scalar interpretation for CyberPower UPS network cards.
//! Numeric OIDs/units are protocol facts; no NUT driver or control code is used.
use crate::protocol::source_status;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const ROOT: &str = ".1.3.6.1.4.1.3808.1.1.1.";
const METRICS: &[(&str, &str, &str, &str, f64, f64, f64)] = &[
    ("2.2.1.0", "Battery charge", "battery", "%", 1., 0., 100.),
    // Net-SNMP -Ot returns raw TimeTicks, each one hundredth of a second.
    (
        "2.2.4.0",
        "Estimated runtime",
        "runtime",
        "min",
        1. / 6000.,
        0.,
        4_294_967_295.,
    ),
    ("4.2.3.0", "UPS load", "load", "%", 1., 0., 200.),
    ("4.2.5.0", "Output power", "power", "W", 1., 0., 1_000_000.),
    (
        "3.2.1.0",
        "Input voltage",
        "voltage",
        "V",
        0.1,
        0.,
        100_000.,
    ),
    (
        "4.2.1.0",
        "Output voltage",
        "voltage",
        "V",
        0.1,
        0.,
        100_000.,
    ),
    (
        "3.2.4.0",
        "Input frequency",
        "frequency",
        "Hz",
        0.1,
        0.,
        10_000.,
    ),
    (
        "4.2.2.0",
        "Output frequency",
        "frequency",
        "Hz",
        0.1,
        0.,
        10_000.,
    ),
    (
        "4.2.4.0",
        "Output current",
        "current",
        "A",
        0.1,
        0.,
        1_000_000.,
    ),
    (
        "2.2.2.0",
        "Battery voltage",
        "voltage",
        "V",
        0.1,
        0.,
        100_000.,
    ),
    (
        "2.2.3.0",
        "Battery temperature",
        "temperature",
        "°C",
        1.,
        -100.,
        200.,
    ),
    (
        "10.2.0",
        "UPS temperature",
        "temperature",
        "°C",
        1.,
        -100.,
        200.,
    ),
];
const STATES: &[&str] = &["4.1.1.0", "2.1.1.0", "2.2.5.0", "10.1.0"];

pub fn oids() -> Vec<String> {
    METRICS
        .iter()
        .map(|m| m.0)
        .chain(STATES.iter().copied())
        .map(|suffix| format!("{ROOT}{suffix}"))
        .collect()
}

pub(crate) fn apply(snapshot: &mut Value, values: &BTreeMap<String, Value>, now: f64) {
    let numeric = |suffix: &str, low: f64, high: f64| {
        values
            .get(&format!("{ROOT}{suffix}"))
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite() && *n >= low && *n <= high)
    };
    let state = |suffix: &str| {
        values
            .get(&format!("{ROOT}{suffix}"))
            .and_then(Value::as_u64)
    };
    let mode = state("4.1.1.0");
    let label = match mode {
        Some(2) => Some("Online"),
        Some(3) => Some("On battery"),
        Some(4) => Some("Boost"),
        Some(5) => Some("Sleeping"),
        Some(6) => Some("Output off"),
        Some(7) => Some("Rebooting"),
        Some(8) => Some("Eco"),
        Some(9) => Some("Bypass"),
        Some(10) => Some("Buck"),
        Some(11) => Some("Overload"),
        _ => None,
    };
    let online = match mode {
        Some(2 | 4 | 8 | 9 | 10 | 11) => Some(true),
        Some(3 | 5 | 6) => Some(false),
        _ => None,
    };
    let charge = numeric("2.2.1.0", 0., 100.);
    let runtime = numeric("2.2.4.0", 0., 4_294_967_295.).map(|v| v / 100.);
    snapshot["platform"]["ups"] = json!({"manufacturer":"CyberPower","transport":"snmp",
        "status":label,"status_code":mode,"online":online,
        "on_battery":if label.is_some(){Some(mode==Some(3))}else{None},
        "charge_pct":charge,"runtime_s":runtime,"load_pct":numeric("4.2.3.0",0.,200.),
        "output_w":numeric("4.2.5.0",0.,1_000_000.),
        "battery_low":match state("2.1.1.0"){Some(2)=>Some(false),Some(3)=>Some(true),_=>None},
        "replace_battery":match state("2.2.5.0"){Some(1)=>Some(false),Some(2)=>Some(true),_=>None}});
    snapshot["platform"]["battery"] = json!({"percent":charge,"seconds_left":runtime,
        "power_plugged":online,"state":label.unwrap_or("Unknown")});
    snapshot["sources"]["ups_status"] = source_status(
        now,
        if label.is_some() {
            None
        } else {
            Some("UPS power status unavailable")
        },
        true,
    );
    for &(suffix, name, kind, unit, scale, low, high) in METRICS {
        if let Some(value) = numeric(suffix, low, high) {
            snapshot["sensors"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":format!("ups/{suffix}"),
                "name":name,"kind":kind,"unit":unit,"value":value*scale,
                "chip":"UPS","source":"snmp","updated_at":now,"age_s":0.}));
        }
    }
    if let Some(online) = online {
        snapshot["sensors"].as_array_mut().unwrap().push(
            json!({"id":"ups/mains","name":"Mains online",
            "kind":"state","unit":"","value":u8::from(online),"chip":"UPS","source":"snmp",
            "updated_at":now,"age_s":0.}),
        );
    }
    let mut alert = |id: &str, severity: &str, message: &str| {
        snapshot["alerts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":format!("ups/{id}"),"severity":severity,"message":message}));
    };
    match mode {
        Some(3) => alert("on-battery", "warning", "UPS is running on battery"),
        Some(5 | 6) => alert("output-off", "critical", "UPS output is off"),
        Some(7) => alert("rebooting", "warning", "UPS is rebooting"),
        Some(9) => alert("bypass", "warning", "UPS is in bypass mode"),
        Some(11) => alert("overload", "critical", "UPS is overloaded"),
        _ if label.is_none() => alert("status", "warning", "UPS power status unavailable"),
        _ => (),
    }
    if state("2.1.1.0") == Some(3) {
        alert("low-battery", "critical", "UPS battery is low");
    }
    if state("2.2.5.0") == Some(2) {
        alert(
            "replace-battery",
            "warning",
            "UPS battery needs replacement",
        );
    }
    match state("10.1.0") {
        Some(2) => alert("overheat", "critical", "UPS reports overheating"),
        Some(3) => alert("hardware", "critical", "UPS reports an internal fault"),
        _ => (),
    }
}
