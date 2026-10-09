use axum::{body::Body, http::Request};
use glimdock_collector::{
    config::{self, Config, ConfigManager},
    push, runtime,
    server::{self, ConfigurationService, HttpState, SnapshotStore},
    MAX_NODES, MAX_PAYLOAD,
};
use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt};
use tower::ServiceExt;
const AGENT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const KEY: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BOOT: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const BOOT2: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
fn manager() -> (tempfile::TempDir, ConfigManager) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    config::atomic_write(
        &path,
        br#"{"enable_local":false,"remote_collectors":[],"printers":[]}"#,
        0o600,
    )
    .unwrap();
    (dir, ConfigManager::new(path))
}
fn mutate(m: &ConfigManager, action: &str, node: Value) -> Value {
    m.mutate(&json!({"action":action,"version":m.get().unwrap()["version"],"node":node}))
        .unwrap()
}
fn pair(m: &ConfigManager) -> Value {
    mutate(
        m,
        "pair-agent",
        json!({"name":"My Mac","platform":"macos","kind":"server"}),
    )
}
fn enrollment() -> Value {
    json!({"agent_id":AGENT,"agent_token":KEY,"boot_id":BOOT,"kind":"server","name":"Actual Mac","platform":"macos","interval_s":3.})
}
fn sample(node: &str, boot: &str, seq: u64, stamp: f64) -> Value {
    let mut snapshot = runtime::empty_snapshot("Actual Mac", "192.0.2.1", seq, stamp);
    snapshot["node"] = json!({"type":"server","platform":"macos","id":"server:local"});
    snapshot["sources"] =
        json!({"host":{"enabled":true,"ok":true,"updated_at":stamp,"age_s":0.,"error":null}});
    snapshot["host"]["cpu_pct"] = json!(25.);
    snapshot["host"]["mem_used_bytes"] = json!(1024);
    snapshot["host"]["mem_total_bytes"] = json!(4096);
    json!({"agent_id":AGENT,"node_id":node,"boot_id":boot,"sequence":seq,"sampled_at":stamp,"snapshot":snapshot})
}
fn registered(m: &ConfigManager, now: f64) -> String {
    let p = pair(m);
    push::enroll(
        m,
        p["pairing"]["token"].as_str().unwrap(),
        &enrollment(),
        now,
    )
    .unwrap()["node_id"]
        .as_str()
        .unwrap()
        .to_string()
}
#[test]
fn pairing_projection_redacts_all_credentials_and_expires() {
    let (_d, m) = manager();
    let p = pair(&m);
    let token = p["pairing"]["token"].as_str().unwrap();
    let text = m.get().unwrap().to_string();
    let private = fs::read_to_string(&m.path).unwrap();
    assert!(!text.contains(token));
    assert!(!text.contains("token_hash"));
    assert!(!text.contains("pairing_hash"));
    assert!(!private.contains(token));
    assert_eq!(m.get().unwrap()["nodes"][0]["pairing_pending"], true);
    assert_eq!(
        push::enroll(
            &m,
            token,
            &enrollment(),
            p["pairing"]["expires_at"].as_f64().unwrap() + 1.
        )
        .unwrap_err()
        .status,
        401
    );
}
#[test]
fn enrollment_retry_is_exact_and_credential_is_scoped() {
    let (_d, m) = manager();
    let p = pair(&m);
    let token = p["pairing"]["token"].as_str().unwrap();
    let now = glimdock_collector::epoch();
    let response = push::enroll(&m, token, &enrollment(), now).unwrap();
    assert_eq!(response["duplicate"], false);
    let reopened = ConfigManager::new(m.path.clone());
    assert_eq!(
        push::enroll(&reopened, token, &enrollment(), now + 1.).unwrap()["duplicate"],
        true
    );
    let mut different = enrollment();
    different["agent_id"] = json!(BOOT2);
    assert_eq!(
        push::enroll(&m, token, &different, now).unwrap_err().status,
        409
    );
    different = enrollment();
    different["agent_token"] = json!(BOOT2);
    assert_eq!(
        push::enroll(&m, token, &different, now).unwrap_err().status,
        409
    );
    let id = response["node_id"].as_str().unwrap();
    assert_eq!(
        push::ingest(&m, token, &sample(id, BOOT, 1, now), now)
            .unwrap_err()
            .status,
        401
    );
    assert!(push::ingest(&m, KEY, &sample(id, BOOT, 1, now), now).is_ok());
}
#[test]
fn publishers_cannot_reuse_pairing_keys_or_other_publisher_secrets() {
    let (_d, m) = manager();
    let p = pair(&m);
    let token = p["pairing"]["token"].as_str().unwrap();
    let mut e = enrollment();
    e["agent_token"] = json!(token);
    assert_eq!(
        push::enroll(&m, token, &e, glimdock_collector::epoch())
            .unwrap_err()
            .status,
        400
    );
    push::enroll(&m, token, &enrollment(), glimdock_collector::epoch()).unwrap();
    let p2 = mutate(
        &m,
        "pair-agent",
        json!({"name":"Second","platform":"macos"}),
    );
    e = enrollment();
    e["agent_id"] = json!(BOOT2);
    assert_eq!(
        push::enroll(
            &m,
            p2["pairing"]["token"].as_str().unwrap(),
            &e,
            glimdock_collector::epoch()
        )
        .unwrap_err()
        .status,
        409
    );
}
#[test]
fn migration_preserves_existing_node_identity_and_removes_polling() {
    let (_d, m) = manager();
    let mut doc = serde_json::from_slice::<Value>(&fs::read(&m.path).unwrap()).unwrap();
    doc["remote_collectors"] = json!([{"id":"This-Mac","type":"server","name":"This Mac","platform":"macos","url":"http://192.0.2.2:8768"}]);
    config::atomic_write(&m.path, &serde_json::to_vec(&doc).unwrap(), 0o600).unwrap();
    let p = mutate(
        &m,
        "pair-agent",
        json!({"id":"remote:This-Mac","name":"This Mac","platform":"macos"}),
    );
    assert_eq!(p["pairing"]["node_id"], "remote:This-Mac");
    let cfg = Config::read(&m.path).unwrap();
    assert!(cfg.remote_collectors.is_empty());
    assert_eq!(cfg.push_agents[0].id, "remote:This-Mac");
    assert_eq!(cfg.enabled_node_count(), 1);
}
#[test]
fn samples_replay_and_session_rules_survive_manager_restart() {
    let (_d, m) = manager();
    let now = glimdock_collector::epoch();
    let id = registered(&m, now);
    let first = sample(&id, BOOT, 1, now);
    assert_eq!(
        push::ingest(&m, KEY, &first, now).unwrap()["duplicate"],
        false
    );
    let restarted = ConfigManager::new(m.path.clone());
    assert_eq!(
        push::ingest(&restarted, KEY, &first, now + 1.).unwrap()["duplicate"],
        true
    );
    let mut changed = first.clone();
    changed["snapshot"]["host"]["cpu_pct"] = json!(30);
    assert_eq!(
        push::ingest(&m, KEY, &changed, now + 1.)
            .unwrap_err()
            .status,
        409
    );
    push::ingest(&m, KEY, &sample(&id, BOOT, 2, now + 2.), now + 2.).unwrap();
    assert_eq!(
        push::ingest(&m, KEY, &first, now + 3.).unwrap_err().status,
        409
    );
    push::ingest(&m, KEY, &sample(&id, BOOT2, 1, now + 4.), now + 4.).unwrap();
    assert_eq!(
        push::ingest(&m, KEY, &sample(&id, BOOT, 3, now + 5.), now + 5.)
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(
        push::ingest(&m, KEY, &sample(&id, BOOT2, 2, now + 4.), now + 5.)
            .unwrap_err()
            .status,
        409
    );
}
#[test]
fn retry_heartbeat_never_refreshes_measurement_or_retains_expired_numbers() {
    let (_d, m) = manager();
    let now = glimdock_collector::epoch();
    let id = registered(&m, now);
    let data = sample(&id, BOOT, 1, now);
    push::ingest(&m, KEY, &data, now).unwrap();
    push::ingest(&m, KEY, &data, now + 10.).unwrap();
    let cfg = Config::read(&m.path).unwrap();
    let fresh = push::node_snapshot(&cfg.push_agents[0], Some(&m.path), 1, now + 11.);
    assert_eq!(fresh["last_seen"], now + 10.);
    assert_eq!(fresh["sample_at"], now);
    assert_eq!(fresh["snapshot"]["host"]["cpu_pct"], 25.);
    assert_eq!(
        server::node_summary(&fresh["snapshot"], now + 11.)["age_s"],
        11.
    );
    assert!(server::node_summary(&fresh["snapshot"], now + 16.)["cpu_percent"].is_null());
    assert_eq!(
        server::node_status(&fresh["snapshot"], now + 16.),
        "offline"
    );
    let stale = push::node_snapshot(&cfg.push_agents[0], Some(&m.path), 2, now + 16.);
    assert!(stale["snapshot"]["host"]["cpu_pct"].is_null());
    assert_eq!(stale["snapshot"]["agent"]["age_s"], 16.);
    assert_eq!(
        server::node_status(&stale["snapshot"], now + 16.),
        "offline"
    );
    assert_eq!(
        push::ingest(&m, KEY, &data, now + 16.).unwrap_err().status,
        409
    );
}
#[test]
fn pause_revocation_and_delete_block_ingestion() {
    let (_d, m) = manager();
    let now = glimdock_collector::epoch();
    let id = registered(&m, now);
    let data = sample(&id, BOOT, 1, now);
    push::ingest(&m, KEY, &data, now).unwrap();
    mutate(
        &m,
        "upsert",
        json!({"id":id,"type":"server","origin":"agent","name":"Renamed","enabled":false}),
    );
    assert_eq!(
        push::ingest(&m, KEY, &data, now + 1.).unwrap_err().status,
        403
    );
    mutate(
        &m,
        "upsert",
        json!({"id":id,"type":"server","origin":"agent","name":"Renamed","enabled":true}),
    );
    assert!(push::ingest(&m, KEY, &data, now + 1.).is_ok());
    let r = m
        .mutate(&json!({"action":"revoke-agent","version":m.get().unwrap()["version"],"id":id}))
        .unwrap();
    assert_eq!(r["config"]["nodes"][0]["revoked"], true);
    assert_eq!(
        push::ingest(&m, KEY, &data, now + 2.).unwrap_err().status,
        401
    );
    m.mutate(&json!({"action":"delete","version":m.get().unwrap()["version"],"id":id}))
        .unwrap();
    assert!(Config::read(&m.path).unwrap().push_agents.is_empty());
    assert_eq!(
        fs::read_dir(m.path.parent().unwrap().join("agent-inbox"))
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn pairing_rotation_revokes_previous_publisher_and_erases_latest_sample() {
    let (_d, m) = manager();
    let now = glimdock_collector::epoch();
    let id = registered(&m, now);
    push::ingest(&m, KEY, &sample(&id, BOOT, 1, now), now).unwrap();
    let p = mutate(
        &m,
        "pair-agent",
        json!({"id":id,"name":"New owner","platform":"macos"}),
    );
    assert_eq!(
        push::ingest(&m, KEY, &sample(&id, BOOT, 2, now + 1.), now + 1.)
            .unwrap_err()
            .status,
        401
    );
    let cfg = Config::read(&m.path).unwrap();
    assert!(push::node_snapshot(&cfg.push_agents[0], Some(&m.path), 3, now)["sample_at"].is_null());
    assert!(push::enroll(
        &m,
        p["pairing"]["token"].as_str().unwrap(),
        &enrollment(),
        now
    )
    .is_ok());
}
#[test]
fn timestamps_platform_capacity_and_schema_are_checked() {
    let (_d, m) = manager();
    let now = glimdock_collector::epoch();
    let id = registered(&m, now);
    for stamp in [now - 16., now + 31.] {
        assert_eq!(
            push::ingest(&m, KEY, &sample(&id, BOOT, 1, stamp), now)
                .unwrap_err()
                .status,
            409
        );
    }
    let mut invalid = sample(&id, BOOT, 1, now);
    invalid["snapshot"]["node"]["platform"] = json!("windows");
    assert_eq!(
        push::ingest(&m, KEY, &invalid, now).unwrap_err().status,
        400
    );
    invalid = sample(&id, BOOT, 1, now);
    invalid["snapshot"]["schema"] = json!(2);
    assert_eq!(
        push::ingest(&m, KEY, &invalid, now).unwrap_err().status,
        400
    );
    for i in 1..MAX_NODES {
        mutate(&m, "pair-agent", json!({"name":format!("Other {i}")}));
    }
    assert_eq!(m.mutate(&json!({"action":"pair-agent","version":m.get().unwrap()["version"],"node":{"name":"Fifth"}})).unwrap_err().status,400);
}
#[test]
fn adapter_cadence_is_saved_and_returned_in_ingestion_ack() {
    let (_d, m) = manager();
    let now = glimdock_collector::epoch();
    let p = pair(&m);
    let mut e = enrollment();
    e["interval_s"] = json!(300.);
    let r = push::enroll(&m, p["pairing"]["token"].as_str().unwrap(), &e, now).unwrap();
    assert_eq!(r["interval_s"], 300.);
    assert_eq!(r["ttl_s"], 900.);
    let id = r["node_id"].as_str().unwrap();
    mutate(
        &m,
        "upsert",
        json!({"id":id,"origin":"agent","type":"server","name":"Adapter","poll_interval_s":5.,"ttl_s":15.}),
    );
    let ack = push::ingest(&m, KEY, &sample(id, BOOT, 1, now), now).unwrap();
    assert_eq!(ack["interval_s"], 5.);
    assert_eq!(ack["ttl_s"], 15.);
}
#[tokio::test]
async fn privileged_socket_and_http_enforce_roles_and_shutdown() {
    let (d, m) = manager();
    let now = glimdock_collector::epoch();
    let p = pair(&m);
    let socket = d.path().join("config.sock");
    let service = ConfigurationService::bind(m.path.clone(), &socket, None, false).unwrap();
    let (stop, receiver) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(service.run(receiver));
    let display = "1".repeat(64);
    let setup = "2".repeat(64);
    let state = HttpState::new(
        SnapshotStore::new(d.path().join("snapshot.json"), false),
        display.clone(),
        Some(setup.clone()),
        socket,
    )
    .unwrap()
    .with_local_console();
    let app = server::router(state);
    let make = |path: &str, token: &str, body: &Value| {
        let bytes = serde_json::to_vec(body).unwrap();
        Request::builder()
            .method("POST")
            .uri(path)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", bytes.len())
            .body(Body::from(bytes))
            .unwrap()
    };
    let response = app
        .clone()
        .oneshot(make(
            "/api/v1/agents/enroll",
            p["pairing"]["token"].as_str().unwrap(),
            &enrollment(),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let raw = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .unwrap();
    let node = serde_json::from_slice::<Value>(&raw).unwrap()["node_id"]
        .as_str()
        .unwrap()
        .to_string();
    for key in [&display, &setup] {
        let r = app
            .clone()
            .oneshot(make(
                "/api/v1/agents/push",
                key,
                &sample(&node, BOOT, 1, now),
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), 401);
    }
    let r = app
        .clone()
        .oneshot(make(
            "/api/v1/agents/push",
            KEY,
            &sample(&node, BOOT, 1, now),
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let bytes = vec![b'x'; MAX_PAYLOAD + 4097];
    let r = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/agents/push")
                .header("Authorization", format!("Bearer {KEY}"))
                .header("Content-Type", "application/json")
                .header("Content-Length", bytes.len())
                .body(Body::from(bytes))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), 413);
    stop.send(true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let perms = fs::metadata(m.path.parent().unwrap().join("agent-inbox"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(perms & 0o777, 0o700);
}
#[tokio::test]
async fn runtime_and_snapshot_reader_preserve_push_node_identity_and_metadata() {
    let (d, m) = manager();
    let now = glimdock_collector::epoch();
    let id = registered(&m, now);
    push::ingest(&m, KEY, &sample(&id, BOOT, 1, now), now).unwrap();
    let mut collector = runtime::Collector::new(Config::read(&m.path).unwrap()).unwrap();
    let aggregate = collector.sample().await.unwrap();
    assert_eq!(aggregate["nodes"][0]["id"], id);
    assert_eq!(aggregate["nodes"][0]["status"], "healthy");
    let path = d.path().join("snapshot.json");
    config::atomic_write(&path, &serde_json::to_vec(&aggregate).unwrap(), 0o600).unwrap();
    let store = SnapshotStore::new(path, false);
    let nodes = store.nodes(now + 1.).unwrap();
    assert_eq!(nodes["nodes"][0]["last_seen"], now);
    assert_eq!(nodes["nodes"][0]["origin"], "agent");
    let (read, _) = store.read(Some(&id), now + 1.).unwrap();
    assert_eq!(read["node"]["id"], id);
    assert_eq!(read["agent"]["sampled_at"], now);
}

#[test]
fn one_publisher_cannot_write_another_node_and_corrupt_inbox_is_fail_closed() {
    let (_d, m) = manager();
    let now = glimdock_collector::epoch();
    let first = registered(&m, now);
    let p = mutate(
        &m,
        "pair-agent",
        json!({"name":"Second","platform":"macos"}),
    );
    let second_id = "e".repeat(64);
    let second_key = "f".repeat(64);
    let mut e = enrollment();
    e["agent_id"] = json!(second_id);
    e["agent_token"] = json!(second_key);
    let second = push::enroll(&m, p["pairing"]["token"].as_str().unwrap(), &e, now).unwrap()
        ["node_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        push::ingest(&m, KEY, &sample(&second, BOOT, 1, now), now)
            .unwrap_err()
            .status,
        401
    );
    assert_eq!(
        push::ingest(&m, &second_key, &sample(&first, BOOT, 1, now), now)
            .unwrap_err()
            .status,
        401
    );
    push::ingest(&m, KEY, &sample(&first, BOOT, 1, now), now).unwrap();
    let inbox = m
        .path
        .parent()
        .unwrap()
        .join("agent-inbox")
        .join(format!("{}.json", config::sha256(first.as_bytes())));
    config::atomic_write(&inbox, b"{broken", 0o600).unwrap();
    assert_eq!(
        push::ingest(&m, KEY, &sample(&first, BOOT, 2, now + 1.), now + 1.)
            .unwrap_err()
            .status,
        503
    );
    let cfg = Config::read(&m.path).unwrap();
    assert!(
        push::node_snapshot(&cfg.push_agents[0], Some(&m.path), 2, now + 1.)["snapshot"]["host"]
            ["cpu_pct"]
            .is_null()
    );
}
#[test]
fn unsafe_host_identity_cannot_escape_selected_snapshot_capacity() {
    let (_d, m) = manager();
    let now = glimdock_collector::epoch();
    let id = registered(&m, now);
    let mut data = sample(&id, BOOT, 1, now);
    data["snapshot"]["host"]["ip"] = json!("a".repeat(MAX_PAYLOAD));
    assert_eq!(push::ingest(&m, KEY, &data, now).unwrap_err().status, 400);
    data = sample(&id, BOOT, 1, now);
    data["snapshot"]["host"]["name"] = json!("a".repeat(65));
    assert_eq!(push::ingest(&m, KEY, &data, now).unwrap_err().status, 400);
}

#[test]
fn expired_pairing_recovers_only_its_exact_durable_binding_after_lost_response() {
    let (_d, m) = manager();
    let now = glimdock_collector::epoch();
    let p = pair(&m);
    let key = p["pairing"]["token"].as_str().unwrap();
    let enrolled = push::enroll(&m, key, &enrollment(), now).unwrap();
    let expired = p["pairing"]["expires_at"].as_f64().unwrap() + 3600.;
    let reopened = ConfigManager::new(m.path.clone());
    let recovered = push::enroll(&reopened, key, &enrollment(), expired).unwrap();
    assert_eq!(recovered["node_id"], enrolled["node_id"]);
    assert_eq!(recovered["duplicate"], true);
    let mut wrong = enrollment();
    wrong["agent_id"] = json!(BOOT2);
    assert_eq!(
        push::enroll(&m, key, &wrong, expired).unwrap_err().status,
        401
    );
    wrong = enrollment();
    wrong["agent_token"] = json!(BOOT2);
    assert_eq!(
        push::enroll(&m, key, &wrong, expired).unwrap_err().status,
        401
    );
    let id = enrolled["node_id"].as_str().unwrap();
    m.mutate(&json!({"action":"revoke-agent","version":m.get().unwrap()["version"],"id":id}))
        .unwrap();
    assert_eq!(
        push::enroll(&m, key, &enrollment(), expired)
            .unwrap_err()
            .status,
        401
    );
    mutate(
        &m,
        "pair-agent",
        json!({"id":id,"name":"Fresh owner","platform":"macos"}),
    );
    assert_eq!(
        push::enroll(&m, key, &enrollment(), expired)
            .unwrap_err()
            .status,
        401
    );
}
