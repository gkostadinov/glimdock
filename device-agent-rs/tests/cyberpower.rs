use glimdock_agent::{
    cyberpower::ROOT,
    snmp::{SnmpCollector, SnmpConfig, SYS_UPTIME},
    Collector,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    sync::{Arc, Mutex},
};

struct Fixture {
    _dir: tempfile::TempDir,
    config: SnmpConfig,
    values: Arc<Mutex<BTreeMap<String, Value>>>,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let credentials = dir.path().join("credentials.json");
        fs::write(&credentials, r#"{"version":"2c","community":"synthetic"}"#).unwrap();
        let config: SnmpConfig =
            serde_json::from_value(json!({"id":"ups","name":"UPS","platform":"other",
            "address":"192.0.2.210","credential_file":credentials,"ups_profile":"cyberpower"}))
            .unwrap();
        let mut values = BTreeMap::from([(SYS_UPTIME.into(), json!(123456))]);
        for (oid, value) in [
            ("4.1.1.0", 8),
            ("2.1.1.0", 2),
            ("2.2.5.0", 1),
            ("10.1.0", 1),
            ("2.2.1.0", 97),
            ("2.2.4.0", 174000),
            ("4.2.3.0", 28),
            ("4.2.5.0", 510),
            ("3.2.1.0", 2290),
            ("4.2.4.0", 22),
            ("2.2.2.0", 547),
        ] {
            values.insert(format!("{ROOT}{oid}"), json!(value));
        }
        Self {
            _dir: dir,
            config,
            values: Arc::new(Mutex::new(values)),
        }
    }
    fn collector(&self) -> SnmpCollector {
        let values = self.values.clone();
        SnmpCollector::with_runner(self.config.clone(), move |request| {
            assert_eq!(&request.args[..2], &["-Cf", "-OnqteU"]);
            assert_eq!(request.args[6], "udp:192.0.2.210:161");
            assert!(!request.args.join(" ").contains("synthetic"));
            let values = values.lock().unwrap();
            Ok(request
                .args
                .iter()
                .skip(7)
                .map(|oid| format!("{oid} {}", values.get(oid).unwrap_or(&Value::Null)))
                .collect::<Vec<_>>()
                .join("\n"))
        })
        .unwrap()
    }
    fn set(&self, suffix: &str, value: Value) {
        self.values
            .lock()
            .unwrap()
            .insert(format!("{ROOT}{suffix}"), value);
    }
}

#[test]
fn direct_ups_units_eco_mode_and_optional_metrics_are_correct() {
    let fixture = Fixture::new();
    let mut collector = fixture.collector();
    let s = collector.sample(1000., 1.).unwrap();
    assert_eq!(s["platform"]["ups"]["transport"], "snmp");
    assert_eq!(s["platform"]["ups"]["status"], "Eco");
    assert_eq!(s["platform"]["ups"]["online"], true);
    assert_eq!(s["platform"]["ups"]["runtime_s"], 1740.);
    assert_eq!(s["platform"]["ups"]["output_w"], 510.);
    let sensor = |name: &str| {
        s["sensors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == name)
            .unwrap()
    };
    assert_eq!(sensor("Estimated runtime")["value"], 29.);
    assert_eq!(sensor("Input voltage")["value"], 229.);
    assert_eq!(sensor("Battery voltage")["value"], 54.7);
    assert_eq!(sensor("Output current")["value"], 2.2);
    assert!(s["sensors"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["name"] != "UPS temperature"));
    assert_eq!(s["node"]["status"], "healthy");
    assert!(s["host"]["cpu_pct"].is_null());
    assert!(s["power"]["package_w"].is_null());
    assert!(!s.to_string().contains("synthetic"));
}

#[test]
fn battery_bypass_off_and_fault_flags_raise_only_monitoring_alerts() {
    let fixture = Fixture::new();
    let mut collector = fixture.collector();
    for (mode, id) in [
        (3, "on-battery"),
        (5, "output-off"),
        (6, "output-off"),
        (7, "rebooting"),
        (9, "bypass"),
        (11, "overload"),
    ] {
        fixture.set("4.1.1.0", json!(mode));
        let s = collector.sample(1000., 1.).unwrap();
        assert_eq!(s["node"]["status"], "degraded");
        assert!(s["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == format!("ups/{id}")));
    }
    fixture.set("4.1.1.0", json!(2));
    fixture.set("2.1.1.0", json!(3));
    fixture.set("2.2.5.0", json!(2));
    fixture.set("10.1.0", json!(3));
    let s = collector.sample(1000., 1.).unwrap();
    for id in ["low-battery", "replace-battery", "hardware"] {
        assert!(s["alerts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == format!("ups/{id}")));
    }
}

#[test]
fn invalid_and_unsupported_fields_are_unknown_and_do_not_invent_power() {
    let fixture = Fixture::new();
    let mut collector = fixture.collector();
    collector.sample(1000., 1.).unwrap();
    fixture.set("4.1.1.0", json!(1));
    fixture.set("2.2.1.0", json!(101));
    fixture.set("2.2.4.0", json!(-1));
    fixture.set("4.2.5.0", Value::Null);
    let s = collector.sample(1010., 11.).unwrap();
    for field in [
        "status",
        "online",
        "on_battery",
        "charge_pct",
        "runtime_s",
        "output_w",
    ] {
        assert!(s["platform"]["ups"][field].is_null(), "{field}");
    }
    assert_eq!(s["node"]["status"], "degraded");
    assert!(s["sensors"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["name"] != "Output power"));
    fixture.values.lock().unwrap().clear();
    let lost = collector.sample(1020., 21.).unwrap();
    assert_eq!(lost["node"]["status"], "offline");
    assert!(lost["platform"]["ups"]["output_w"].is_null());
}

#[test]
fn ups_profile_is_opt_in_and_rejects_unknown_profiles() {
    let mut fixture = Fixture::new();
    fixture.config.ups_profile = None;
    let mut generic = fixture.collector();
    assert!(generic.sample(1000., 1.).unwrap()["platform"]["ups"].is_null());
    fixture.config.ups_profile = Some("arbitrary-script".into());
    assert!(fixture.config.validate().is_err());
}
