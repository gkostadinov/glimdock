// Router appliances remain first-class telemetry nodes, grouped only by the
// collector owner's authoritative registry, never by publisher metadata.
use glimdock_collector::{
    config::{self, Config, ConfigManager},
    push, runtime,
    server::{self, SnapshotStore},
    MAX_NODES, MAX_PAYLOAD, MAX_ROOT_NODES,
};
use serde_json::{json, Value};
use std::fs;
const AGENT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const KEY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BOOT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
fn manager() -> (tempfile::TempDir, ConfigManager) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    config::atomic_write(&path, br#"{"enable_local":false}"#, 0o600).unwrap();
    (dir, ConfigManager::new(path))
}
fn mutate(m: &ConfigManager, action: &str, node: Value) -> Value {
    m.mutate(&json!({"action":action,"version":m.get().unwrap()["version"],"node":node}))
        .unwrap()
}
fn request(m: &ConfigManager, action: &str, node: Value) -> Value {
    json!({"action":action,"version":m.get().unwrap()["version"],"node":node})
}
fn registry() -> Value {
    let roots = (0..MAX_ROOT_NODES)
        .map(|i| json!({"id":format!("agent:root{i}"),"name":format!("Root {i}")}))
        .collect::<Vec<_>>();
    let children = (MAX_ROOT_NODES..MAX_NODES).map(|i| json!({"id":format!("agent:child{i}"),"parent_id":"agent:root0","name":format!("Child {i}")})).collect::<Vec<_>>();
    json!({"enable_local":false,"push_agents":roots.into_iter().chain(children).collect::<Vec<_>>()})
}
#[test]
fn capacity_is_four_roots_eight_total_and_legacy_flat_configs_are_unchanged() {
    let cfg = Config::from_value(registry()).unwrap();
    assert_eq!(cfg.enabled_node_count(), 8);
    assert_eq!(cfg.enabled_root_node_count(), 4);
    let p = ConfigManager::projection(&cfg, "version");
    assert_eq!(p["max_nodes"], 8);
    assert_eq!(p["max_root_nodes"], 4);
    assert_eq!(p["nodes"][4]["parent_id"], "agent:root0");
    let legacy = Config::from_value(
        json!({"enable_local":false,"push_agents":[{"id":"agent:Router","name":"Router"}]}),
    )
    .unwrap();
    assert_eq!(legacy.push_agents[0].parent_id, "");
    assert_eq!(legacy.enabled_root_node_count(), 1);
    let mut ninth = registry();
    ninth["push_agents"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"agent:extra","name":"Extra","parent_id":"agent:root0"}));
    assert!(Config::from_value(ninth).is_err());
    let mut fifth_root = registry();
    fifth_root["push_agents"][4]["parent_id"] = json!("");
    assert!(Config::from_value(fifth_root).is_err());
}
#[test]
fn grouping_rejects_missing_self_nested_cyclic_and_disabled_parents() {
    for parent in ["agent:missing", "agent:child4", "agent:child5", ""] {
        let mut doc = registry();
        doc["push_agents"][4]["parent_id"] = json!(parent);
        assert!(Config::from_value(doc).is_err(), "parent {parent}");
    }
    let mut cycle = registry();
    cycle["push_agents"][0]["parent_id"] = json!("agent:child4");
    assert!(Config::from_value(cycle).is_err());
    let mut paused = registry();
    paused["push_agents"][0]["enabled"] = json!(false);
    assert!(Config::from_value(paused.clone()).is_err());
    for n in paused["push_agents"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .skip(4)
    {
        n["enabled"] = json!(false);
    }
    assert!(Config::from_value(paused).is_ok()); // identities survive pause
    let mut malformed = registry();
    malformed["push_agents"][4]["parent_id"] = json!("not-an-id");
    assert!(Config::from_value(malformed).is_err());
}
#[test]
fn management_preserves_parent_when_omitted_and_rejects_orphans_atomically() {
    let (_dir, m) = manager();
    mutate(
        &m,
        "pair-agent",
        json!({"name":"Router","platform":"router"}),
    );
    let child = mutate(
        &m,
        "pair-agent",
        json!({"name":"MikroTik","platform":"router","parent_id":"agent:Router"}),
    );
    let id = child["pairing"]["node_id"].as_str().unwrap();
    mutate(
        &m,
        "upsert",
        json!({"id":id,"name":"Switch","origin":"agent"}),
    );
    let reopened = ConfigManager::new(m.path.clone());
    assert_eq!(
        reopened.get().unwrap()["nodes"][1]["parent_id"],
        "agent:Router"
    );
    let saved = fs::read(&m.path).unwrap();
    let bad = request(
        &m,
        "upsert",
        json!({"id":"agent:Router","name":"Router","enabled":false,"origin":"agent"}),
    );
    assert_eq!(m.mutate(&bad).unwrap_err().status, 400);
    assert_eq!(fs::read(&m.path).unwrap(), saved);
    assert_eq!(
        m.mutate(
            &json!({"action":"delete","id":"agent:Router","version":m.get().unwrap()["version"]})
        )
        .unwrap_err()
        .status,
        400
    );
    assert_eq!(fs::read(&m.path).unwrap(), saved);
    mutate(
        &m,
        "upsert",
        json!({"id":id,"name":"Switch","enabled":false,"origin":"agent"}),
    );
    mutate(
        &m,
        "upsert",
        json!({"id":"agent:Router","name":"Router","enabled":false,"origin":"agent"}),
    );
    assert_eq!(Config::read(&m.path).unwrap().enabled_node_count(), 0);
    mutate(
        &m,
        "upsert",
        json!({"id":id,"name":"Switch","parent_id":"","enabled":true,"origin":"agent"}),
    );
    assert_eq!(Config::read(&m.path).unwrap().enabled_root_node_count(), 1);
}
#[test]
fn polling_feed_hierarchy_survives_edits_and_migration_to_push() {
    let (_dir, m) = manager();
    mutate(
        &m,
        "pair-agent",
        json!({"name":"Router","platform":"router"}),
    );
    mutate(
        &m,
        "upsert",
        json!({"name":"Switch","type":"server","platform":"router","url":"http://192.0.2.2","parent_id":"agent:Router"}),
    );
    mutate(
        &m,
        "upsert",
        json!({"id":"remote:Switch","name":"Renamed","type":"server","url":"http://192.0.2.2"}),
    );
    assert_eq!(m.get().unwrap()["nodes"][0]["parent_id"], "agent:Router");
    let migrated = mutate(
        &m,
        "pair-agent",
        json!({"id":"remote:Switch","name":"Renamed","platform":"router"}),
    );
    assert_eq!(migrated["pairing"]["node_id"], "remote:Switch");
    let cfg = Config::read(&m.path).unwrap();
    assert!(cfg.remote_collectors.is_empty());
    assert_eq!(cfg.push_agents[1].parent_id, "agent:Router");
}
#[tokio::test]
async fn child_is_fresh_selectable_and_bounded_when_parent_is_offline_and_push_cannot_spoof_group()
{
    let (dir, m) = manager();
    let now = glimdock_collector::epoch();
    mutate(
        &m,
        "pair-agent",
        json!({"name":"Router","platform":"router"}),
    );
    let p = mutate(
        &m,
        "pair-agent",
        json!({"name":"MikroTik","platform":"router","parent_id":"agent:Router"}),
    );
    let id = p["pairing"]["node_id"].as_str().unwrap();
    push::enroll(&m, p["pairing"]["token"].as_str().unwrap(), &json!({"agent_id":AGENT,"agent_token":KEY,"boot_id":BOOT,"platform":"router","kind":"server","name":"MikroTik","interval_s":3.}), now).unwrap();
    let mut sample = runtime::empty_snapshot("MikroTik", "192.0.2.2", 1, now);
    sample["node"] =
        json!({"id":"agent:spoof","platform":"router","type":"server","parent_id":"agent:spoof"});
    sample["parent_id"] = json!("agent:spoof");
    sample["host"]["cpu_pct"] = json!(12.);
    sample["sources"] =
        json!({"host":{"enabled":true,"ok":true,"updated_at":now,"age_s":0.,"error":null}});
    let data = json!({"agent_id":AGENT,"node_id":id,"boot_id":BOOT,"sequence":1,"sampled_at":now,"snapshot":sample});
    push::ingest(&m, KEY, &data, now).unwrap();
    let cfg = Config::read(&m.path).unwrap();
    let child = push::node_snapshot(&cfg.push_agents[1], Some(&m.path), 2, now);
    assert_eq!(child["parent_id"], "agent:Router");
    assert_eq!(child["snapshot"]["node"]["parent_id"], "agent:Router");
    assert!(child["snapshot"].get("parent_id").is_none());
    assert_eq!(server::node_status(&child["snapshot"], now), "healthy");
    let mut collector = runtime::Collector::new(cfg).unwrap();
    let aggregate = collector.sample().await.unwrap();
    let path = dir.path().join("snapshot.json");
    fs::write(&path, serde_json::to_vec(&aggregate).unwrap()).unwrap();
    let store = SnapshotStore::new(path, false);
    let (selected, _) = store.read(Some(id), now).unwrap();
    assert_eq!(selected["node"]["id"], id);
    assert_eq!(selected["node"]["parent_id"], "agent:Router");
    assert_eq!(selected["nodes"][0]["status"], "offline");
    assert_eq!(selected["nodes"][1]["status"], "healthy");
    assert_eq!(selected["host"]["cpu_pct"], 12.);
    assert!(serde_json::to_vec(&selected).unwrap().len() <= MAX_PAYLOAD);
    assert_eq!(
        store.read(None, now).unwrap().0["node"]["id"],
        "agent:Router"
    );
    assert_eq!(
        store.nodes(now).unwrap()["nodes"][1]["parent_id"],
        "agent:Router"
    );
    assert_eq!(
        store.snapshots(now).unwrap()["nodes"][1]["parent_id"],
        "agent:Router"
    );
}
#[test]
fn reader_rejects_orphan_and_nested_aggregates_and_defaults_to_root() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("snapshot.json");
    let now = glimdock_collector::epoch();
    let record = |id: &str, parent: &str| json!({"id":id,"type":"server","platform":"router","name":id,"address":"","parent_id":parent,"snapshot":runtime::empty_snapshot(id,"",1,now)});
    let write = |nodes: Vec<Value>| {
        fs::write(
            &path,
            serde_json::to_vec(&json!({"schema":2,"generated_at":now,"sequence":1,"nodes":nodes}))
                .unwrap(),
        )
        .unwrap()
    };
    let store = SnapshotStore::new(path.clone(), false);
    write(vec![
        record("agent:Switch", "agent:Router"),
        record("agent:Router", ""),
    ]);
    assert_eq!(
        store.read(None, now).unwrap().0["node"]["id"],
        "agent:Router"
    );
    write(vec![
        record("agent:Switch", "agent:missing"),
        record("agent:Router", ""),
    ]);
    assert!(store.nodes(now).is_err());
    write(vec![
        record("agent:Switch", "agent:Router"),
        record("agent:Router", "agent:Host"),
        record("agent:Host", ""),
    ]);
    assert!(store.nodes(now).is_err());
}
