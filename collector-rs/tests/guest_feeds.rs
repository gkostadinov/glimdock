use glimdock_collector::{
    config::Config,
    guest_feeds::{apply, GuestMemorySource},
    push::AgentConfig,
};
use serde_json::{json, Value};

fn config() -> Config {
    Config {
        local_type: "proxmox".into(),
        stable_local_id: "proxmox:test".into(),
        push_agents: vec![AgentConfig {
            id: "agent:guest".into(),
            name: "Guest".into(),
            platform: "macos".into(),
            ..Default::default()
        }],
        guest_memory_sources: vec![GuestMemorySource {
            node_id: "proxmox:test".into(),
            guest_id: 103,
            agent_node_id: "agent:guest".into(),
        }],
        ..Default::default()
    }
}
fn nodes() -> Vec<Value> {
    vec![
        json!({"id":"proxmox:test","snapshot":{"sources":{"proxmox":{"ok":true}},"guests":[{"id":103,"status":"running","mem_used_bytes":2000,"mem_total_bytes":2000,"pve_mem_used_bytes":2000,"mem_assigned_bytes":2000}]}}),
        json!({"id":"agent:guest","platform":"macos","snapshot":{"sources":{"memory":{"ok":true,"updated_at":1000},"agent_push":{"ok":true}},"host":{"mem_total_bytes":2000,"mem_used_bytes":900,"memory_basis":"macos-native","mem_app_bytes":600,"mem_wired_bytes":200,"mem_compressed_bytes":100,"mem_cache_bytes":500,"memory_pressure":"normal","swap_total_bytes":0,"swap_used_bytes":0}}}),
    ]
}
#[test]
fn trusted_native_memory_replaces_only_display_accounting() {
    let mut n = nodes();
    apply(&mut n, &config(), 1003.);
    let g = &n[0]["snapshot"]["guests"][0];
    assert_eq!(g["mem_used_bytes"], 900);
    assert_eq!(g["memory_basis"], "guest-os-macos");
    assert_eq!(g["pve_mem_used_bytes"], 2000);
    assert_eq!(g["mem_assigned_bytes"], 2000);
    assert_eq!(g["mem_compressed_bytes"], 100);
    assert_eq!(g["swap_used_bytes"], 0);
    assert_eq!(g["mem_age_s"], 3.);
    assert!(g["mem_error"].is_null());
}
#[test]
fn unavailable_stale_future_stopped_and_corrupt_samples_never_show_full_host_ram() {
    for case in 0..8 {
        let mut n = nodes();
        match case {
            0 => {
                n.pop();
            }
            1 => n[1]["snapshot"]["sources"]["memory"]["updated_at"] = json!(900),
            2 => n[1]["snapshot"]["sources"]["memory"]["updated_at"] = json!(1100),
            3 => n[1]["snapshot"]["sources"]["agent_push"]["ok"] = json!(false),
            4 => n[0]["snapshot"]["guests"][0]["status"] = json!("stopped"),
            5 => n[0]["snapshot"]["sources"]["proxmox"]["ok"] = json!(false),
            6 => n[1]["snapshot"]["host"]["mem_used_bytes"] = json!(2001),
            _ => n[1]["snapshot"]["host"]["mem_compressed_bytes"] = json!(999),
        }
        apply(&mut n, &config(), 1003.);
        let g = &n[0]["snapshot"]["guests"][0];
        assert!(g["mem_used_bytes"].is_null(), "case {case}");
        assert!(g["mem_total_bytes"].is_null());
        assert!(g["memory_pressure"].is_null());
        assert!(g["mem_error"].is_string());
        assert_eq!(g["pve_mem_used_bytes"], 2000);
    }
}
#[test]
fn configuration_rejects_ambiguous_or_unbound_publishers() {
    config().validate().unwrap();
    for case in 0..5 {
        let mut c = config();
        match case {
            0 => c
                .guest_memory_sources
                .push(c.guest_memory_sources[0].clone()),
            1 => c.guest_memory_sources[0].agent_node_id = "agent:unknown".into(),
            2 => c.guest_memory_sources[0].node_id = "proxmox:unknown".into(),
            3 => c.qga_guest_ids.push(103),
            _ => c.guest_memory_sources[0].guest_id = 0,
        }
        assert!(c.validate().is_err(), "case {case}");
    }
}
