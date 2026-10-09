use glimdock_agent::host::{platform_name, HostCollector, HostConfig};
use glimdock_agent::{Collector, MAX_PAYLOAD};
use serde_json::{json, Value};

#[test]
fn configuration_rejects_invalid_or_unexpected_fields() {
    for document in [
        json!({"command":"reboot"}),
        json!({"id":"-bad"}),
        json!({"id":"a".repeat(33)}),
        json!({"platform":"proxmox"}),
        json!({"platform":[]}),
        json!({"interval_s":"3"}),
        json!({"interval_s":true}),
        json!({"interval_s":0}),
        json!({"interval_s":301}),
        json!({"storage_mounts":vec!["x";17]}),
        json!({"network_interfaces":"en0"}),
        json!({"name":"bad\nname"}),
        json!({"network_interfaces":[""]}),
    ] {
        assert!(
            serde_json::from_value::<HostConfig>(document.clone())
                .and_then(|config| config
                    .validate()
                    .map(|_| config)
                    .map_err(serde::de::Error::custom))
                .is_err(),
            "accepted {document}"
        );
    }
    for platform in ["auto", "macos", "linux", "windows", "router", "other"] {
        let config: HostConfig = serde_json::from_value(json!({"platform":platform})).unwrap();
        config.validate().unwrap();
    }
}

#[test]
fn local_native_sample_has_real_memory_and_os_metadata_and_unknown_first_rates() {
    let mut collector = HostCollector::new(HostConfig {
        id: "native-test".into(),
        name: "Native test".into(),
        ..HostConfig::default()
    })
    .unwrap();
    let first = collector.sample(1000.0, 20.0).unwrap();
    assert_eq!(first["node"]["id"], "server:native-test");
    assert_eq!(first["node"]["type"], "server");
    assert_eq!(first["node"]["platform"], platform_name());
    assert_eq!(first["host"]["os"], platform_name());
    assert!(first["host"]["mem_total_bytes"].as_u64().unwrap() > 0);
    assert!(
        first["host"]["mem_used_bytes"].as_u64().unwrap()
            <= first["host"]["mem_total_bytes"].as_u64().unwrap()
    );
    assert!(first["host"]["uptime_s"].as_u64().unwrap() > 0);
    assert!(first["platform"]["cpu_count"].as_u64().unwrap() > 0);
    assert!(!first["platform"]["architecture"]
        .as_str()
        .unwrap()
        .is_empty());
    for field in [
        "cpu_pct",
        "disk_read_bps",
        "disk_write_bps",
        "net_rx_bps",
        "net_tx_bps",
    ] {
        assert!(first["host"][field].is_null(), "first {field} is not null");
    }
    assert!(serde_json::to_vec(&first).unwrap().len() <= MAX_PAYLOAD);
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    let second = collector.sample(1003.0, 23.0).unwrap();
    assert_eq!(second["sequence"], 2);
    let cpu = second["host"]["cpu_pct"].as_f64().unwrap();
    assert!((0.0..=100.0).contains(&cpu));
    for row in second["storage"].as_array().unwrap() {
        if row["status"] == "online" {
            assert!(row["used_bytes"].as_u64().unwrap() <= row["total_bytes"].as_u64().unwrap());
        }
    }
    assert!(serde_json::to_vec(&second).unwrap().len() <= MAX_PAYLOAD);
}

#[test]
fn missing_selected_interface_and_filesystem_remain_unknown() {
    let config: HostConfig = serde_json::from_value(json!({
        "id":"missing-test", "network_interfaces":["glimdock-nonexistent"],
        "storage_mounts":["glimdock-nonexistent"],
    }))
    .unwrap();
    let mut collector = HostCollector::new(config).unwrap();
    for tick in 1..=2 {
        let snapshot = collector
            .sample(1000.0 + tick as f64 * 3.0, 20.0 + tick as f64 * 3.0)
            .unwrap();
        assert!(snapshot["host"]["net_rx_bps"].is_null());
        assert_eq!(snapshot["platform"]["interfaces"][0]["status"], "unknown");
        assert_eq!(snapshot["storage"][0]["status"], "unknown");
        assert!(snapshot["storage"][0]["used_bytes"].is_null());
        assert_eq!(snapshot["sources"]["storage"]["error"], "Probe unavailable");
        assert_eq!(snapshot["node"]["status"], "degraded");
    }
}

#[test]
fn native_collector_is_send_and_duplicate_selections_are_removed() {
    fn is_send<T: Send>() {}
    is_send::<HostCollector>();
    let collector = HostCollector::new(HostConfig {
        network_interfaces: vec!["a".into(), "a".into()],
        storage_mounts: vec!["b".into(), "b".into()],
        ..HostConfig::default()
    })
    .unwrap();
    assert_eq!(collector.config.network_interfaces, vec!["a"]);
    assert_eq!(collector.config.storage_mounts, vec!["b"]);
}

#[test]
fn collection_rejects_nonfinite_timestamps() {
    let mut collector = HostCollector::new(HostConfig::default()).unwrap();
    assert!(collector.sample(f64::NAN, 20.0).is_err());
    assert!(collector.sample(1000.0, f64::INFINITY).is_err());
    let _: Value = collector.descriptor();
}
