use axum::{body::Body, http::Request};
use glimdock_collector::{
    config::{self, Config, ConfigManager, NodeConfig},
    printers,
    remote_feeds::{self, RemoteCollectorReader},
    runtime::{self, SourceStatus},
    server::{self, HttpState, SnapshotStore, StoreError},
    MAX_PAYLOAD,
};
use serde_json::{json, Value};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
use tower::ServiceExt;

fn source() -> NodeConfig {
    NodeConfig {
        id: "desk".into(),
        name: "Desk".into(),
        url: "http://printer:7125".into(),
        ..Default::default()
    }
}
fn state(now: f64) -> SourceStatus {
    SourceStatus {
        ok: true,
        updated_at: Some(now),
        age_s: Some(0.),
        error: None,
        ..Default::default()
    }
}
fn local(now: f64) -> Value {
    let mut sample = runtime::empty_snapshot("host", "192.0.2.2", 7, now);
    sample["host"]["cpu_pct"] = json!(23);
    sample["sources"] =
        json!({"proc":{"enabled":true,"ok":true,"updated_at":now,"age_s":0,"error":null}});
    sample
}
fn manager() -> (tempfile::TempDir, ConfigManager) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    fs::write(&path,br#"{"node":"vm","host_ip":"192.0.2.2","qga_guest_ids":[100],"truenas_ssh_key":"/private/nas-key"}"#).unwrap();
    (dir, ConfigManager::new(path))
}
fn upsert(manager: &ConfigManager, node: Value) -> Result<Value, config::ConfigError> {
    manager
        .mutate(&json!({"action":"upsert","version":manager.get().unwrap()["version"],"node":node}))
}

#[test]
fn strict_json_rejects_duplicate_fields_recursively() {
    for raw in [
        r#"{"node":"first","node":"second"}"#,
        r#"{"object":{"key":1,"key":2}}"#,
        r#"{"interval_s":NaN}"#,
        r#"{} {}"#,
    ] {
        assert!(config::strict_json(raw.as_bytes()).is_err());
    }
}
#[test]
fn config_validates_existing_fields_and_disallows_controls() {
    assert!(Config::from_value(
        json!({"node":"vm","host_ip":"192.0.2.2","qga_guest_ids":[100,100]})
    )
    .is_ok());
    for value in [
        json!({"unknown":true}),
        json!({"interval_s":false}),
        json!({"qga_guest_ids":[99]}),
        json!({"host_ip":"not-an-ip"}),
        json!({"temperature_warning_c":90,"temperature_critical_c":80}),
    ] {
        assert!(Config::from_value(value).is_err());
    }
}
#[test]
fn node_validation_requires_fixed_origin_and_supported_fields() {
    for url in [
        "http://user:key@printer",
        "file:///private",
        "http://printer/print/start",
        "http://printer?secret=x",
        "http://printer#x",
        "http://printer:0",
        "http://printer with spaces",
    ] {
        assert!(
            Config::from_value(json!({"printers":[{"id":"p","url":url}]})).is_err(),
            "{url}"
        );
    }
    assert!(Config::from_value(
        json!({"printers":[{"id":"p","url":"http://printer","token_file":"/secret"}]})
    )
    .is_err());
}
#[test]
fn remote_accepts_full_fixed_snapshot_and_preserves_four_node_cap() {
    let cfg = Config::from_value(
        json!({"remote_collectors":[{"id":"lab","url":"https://REMOTE:8765/api/v1/snapshot"}]}),
    )
    .unwrap();
    assert_eq!(cfg.remote_collectors[0].url, "https://remote:8765");
    let printers = (0..4)
        .map(|i| json!({"id":format!("p{i}"),"url":format!("http://printer{i}")}))
        .collect::<Vec<_>>();
    assert!(Config::from_value(json!({"enable_proxmox":false,"printers":printers})).is_ok());
    assert!(Config::from_value(json!({"printers":printers})).is_err());
}
#[test]
fn public_config_hides_secrets_and_preserves_unrelated_options() {
    let (dir, manager) = manager();
    let response=upsert(&manager,json!({"type":"klipper","name":"Workshop Printer","url":"http://printer","secret":"sample-private-key"})).unwrap();
    let public = serde_json::to_string(&response).unwrap();
    assert!(!public.contains("sample-private-key"));
    assert!(!public.contains("/private/nas-key"));
    assert!(response["applying"] == true);
    assert_eq!(
        response["config"]["nodes"][1]["id"],
        "klipper:Workshop-Printer"
    );
    let cfg = Config::read(&manager.path).unwrap();
    assert_eq!(cfg.qga_guest_ids, vec![100]);
    assert_eq!(cfg.truenas_ssh_key, "/private/nas-key");
    let key = &cfg.printers[0].api_key_file;
    assert_eq!(fs::read_to_string(key).unwrap(), "sample-private-key\n");
    assert_eq!(
        fs::metadata(key).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&manager.path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(dir.path().join("node-secrets"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}
#[test]
fn origin_change_clears_external_secret_without_deleting_user_file() {
    let (dir, manager) = manager();
    let external = dir.path().join("external.key");
    fs::write(&external, "sample-secret").unwrap();
    fs::write(&manager.path,serde_json::to_vec(&json!({"node":"vm","printers":[{"id":"p","url":"http://printer:7125","api_key_file":external}]})).unwrap()).unwrap();
    for change in [
        json!({"type":"klipper","id":"p","name":"Desk","url":"http://PRINTER:7125/"}),
        json!({"type":"klipper","id":"p","name":"Desk","secret":""}),
    ] {
        upsert(&manager, change).unwrap();
        assert_eq!(
            Config::read(&manager.path).unwrap().printers[0].api_key_file,
            external.to_string_lossy()
        );
    }
    upsert(
        &manager,
        json!({"type":"klipper","id":"p","name":"Desk","url":"http://another:7125"}),
    )
    .unwrap();
    assert!(Config::read(&manager.path).unwrap().printers[0]
        .api_key_file
        .is_empty());
    assert_eq!(fs::read_to_string(external).unwrap(), "sample-secret");
}
#[test]
fn managed_secret_is_removed_only_after_commit() {
    let (_dir, manager) = manager();
    upsert(&manager,json!({"type":"klipper","id":"p","name":"Desk","url":"http://printer","secret":"sample-managed-key"})).unwrap();
    let path = Config::read(&manager.path).unwrap().printers[0]
        .api_key_file
        .clone();
    assert!(PathBuf::from(&path).exists());
    upsert(
        &manager,
        json!({"type":"klipper","id":"p","name":"Desk","clear_secret":true}),
    )
    .unwrap();
    assert!(!PathBuf::from(path).exists());
}
#[test]
fn stale_version_cannot_overwrite_commit() {
    let (_dir, manager) = manager();
    let version = manager.get().unwrap()["version"].clone();
    upsert(
        &manager,
        json!({"type":"local-proxmox","id":"proxmox:vm","name":"Home Lab"}),
    )
    .unwrap();
    let error = manager
        .mutate(&json!({"action":"delete","version":version,"id":"proxmox:vm"}))
        .unwrap_err();
    assert_eq!(error.status, 409);
    assert_eq!(manager.get().unwrap()["local_node"]["name"], "Home Lab");
}
#[test]
fn invalid_requests_and_capacity_do_not_change_files_or_create_secrets() {
    let (dir, manager) = manager();
    for i in 0..3 {
        upsert(
            &manager,
            json!({"type":"klipper","id":format!("p{i}"),"name":"Printer","url":"http://printer"}),
        )
        .unwrap();
    }
    let before = fs::read(&manager.path).unwrap();
    for node in [
        json!({"type":"proxmox-feed","id":"remote","name":"Remote","url":"http://remote","secret":"r".repeat(43)}),
        json!({"type":"klipper","id":"../../p","name":"Desk","url":"http://printer"}),
        json!({"type":"klipper","id":"p0","name":"Desk","api_key_file":"/private"}),
        json!({"type":"klipper","id":"p0","name":"Desk","url":"http://printer","secret":"key\nHeader"}),
        json!({"type":"klipper","id":"p0","name":"Desk","url":"http://printer","secret":"key","clear_secret":true}),
    ] {
        assert_eq!(upsert(&manager, node).unwrap_err().status, 400);
        assert_eq!(fs::read(&manager.path).unwrap(), before);
    }
    assert!(!dir.path().join("node-secrets").exists());
}
#[test]
fn local_delete_restore_and_generated_ids_work() {
    let (_dir, manager) = manager();
    for _ in 0..2 {
        upsert(
            &manager,
            json!({"type":"klipper","name":"Printer","url":"http://printer"}),
        )
        .unwrap();
    }
    assert_eq!(
        manager.get().unwrap()["nodes"][2]["id"],
        "klipper:Printer-2"
    );
    manager.mutate(&json!({"action":"delete","version":manager.get().unwrap()["version"],"id":"proxmox:vm"})).unwrap();
    assert_eq!(manager.get().unwrap()["local_node"]["enabled"], false);
    upsert(&manager, json!({"type":"local-proxmox","name":"Home"})).unwrap();
    assert_eq!(manager.get().unwrap()["local_node"]["enabled"], true);
}
#[test]
fn private_atomic_replacement_publishes_complete_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    config::atomic_write(&path, b"old", 0o640).unwrap();
    config::atomic_write(&path, b"new", 0o640).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"new");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
#[test]
fn bounded_inventory_reports_omissions_and_stays_below_wire_cap() {
    let mut sample = local(1000.);
    sample["sensors"] = (0..100)
        .map(|i| json!({"id":i,"label":"temperature","value":42}))
        .collect::<Vec<_>>()
        .into();
    let bounded = runtime::bounded_snapshot(sample);
    assert_eq!(bounded["sensors"].as_array().unwrap().len(), 64);
    assert_eq!(bounded["limits"]["truncated"]["sensors"], 36);
    assert!(!bounded["alerts"].as_array().unwrap().is_empty());
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= MAX_PAYLOAD);
}
#[test]
fn oversized_base_cannot_exceed_wire_cap() {
    let mut sample = local(1000.);
    sample["power"]["malformed"] = json!("x".repeat(MAX_PAYLOAD * 2));
    let bounded = runtime::bounded_snapshot(sample);
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= MAX_PAYLOAD);
    assert_eq!(bounded["host"]["cpu_pct"], Value::Null);
}

#[test]
fn printer_progress_eta_unknown_pause_and_complete_are_honest() {
    let status = json!({"webhooks":{"state":"ready"},"print_stats":{"state":"printing","filename":"folder/part.gcode","print_duration":600},"display_status":{"progress":0.5},"extruder":{"temperature":220,"target":220,"power":0.4}});
    let printer = printers::normalize_printer("p", &status, &json!({}), "klipper", 1000.);
    assert_eq!(printer["filename"], "part.gcode");
    assert_eq!(printer["progress_pct"], 50.);
    assert_eq!(printer["remaining_s"], 600.);
    assert_eq!(printer["eta_at"], 1600.);
    assert_eq!(printer["heaters"][0]["duty_pct"], 40.);
    let mut paused = status.clone();
    paused["print_stats"]["state"] = json!("paused");
    assert!(printers::normalize_printer("p", &paused, &json!({}), "", 1000.)["eta_at"].is_null());
    let mut early = status.clone();
    early["display_status"]["progress"] = json!(0.02);
    assert!(
        printers::normalize_printer("p", &early, &json!({}), "", 1000.)["remaining_s"].is_null()
    );
    let mut complete = status;
    complete["print_stats"]["state"] = json!("complete");
    assert_eq!(
        printers::normalize_printer("p", &complete, &json!({}), "", 1000.)["remaining_s"],
        0
    );
}
#[test]
fn printer_shutdown_never_retains_old_print_values() {
    let status = json!({"webhooks":{"state":"shutdown"},"print_stats":{"state":"printing","print_duration":600},"display_status":{"progress":0.5}});
    let p = printers::normalize_printer("p", &status, &json!({}), "", 1000.);
    assert_eq!(p["state"], "unknown");
    assert!(p["progress_pct"].is_null());
    let data = json!({"printer":p});
    let s = printers::printer_snapshot(&source(), &state(1000.), Some(&data), 1, 1000.);
    assert_eq!(s["alerts"][0]["severity"], "critical");
}
#[test]
fn expired_printer_clears_values_and_cannot_inherit_host_sources() {
    let p = printers::normalize_printer(
        "p",
        &json!({"webhooks":{"state":"ready"},"print_stats":{"state":"printing"},"extruder":{"temperature":220}}),
        &json!({}),
        "",
        1000.,
    );
    let data = json!({"printer":p});
    let before = data.clone();
    let snapshot = printers::printer_snapshot(&source(), &state(1000.), Some(&data), 8, 1016.);
    assert!(snapshot["printer"]["heaters"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(snapshot["host"]["cpu_pct"].is_null());
    assert_eq!(snapshot["sources"].as_object().unwrap().len(), 1);
    assert_eq!(data, before);
}
#[test]
fn optional_printer_telemetry_is_bounded_and_missing_is_null() {
    let mut status =
        json!({"webhooks":{"state":"ready"},"print_stats":{"state":"standby"},"fan":{"speed":2}});
    for i in 0..30 {
        status[format!("temperature_sensor t{i}")] = json!({"temperature":42});
        status[format!("heater_generic h{i}")] = json!({"temperature":100,"power":0.5});
    }
    let p = printers::normalize_printer("p", &status, &json!({}), "", 1000.);
    assert_eq!(p["heaters"].as_array().unwrap().len(), 4);
    assert_eq!(p["temperatures"].as_array().unwrap().len(), 8);
    assert!(p["fan_pct"].is_null());
    assert!(p["filament_detected"].is_null());
}

#[test]
fn remote_validator_rejects_demo_printer_bad_schema_stale_and_future() {
    for modify in [
        json!({"demo":true}),
        json!({"printer":{}}),
        json!({"schema":2}),
        json!({"sequence":false}),
        json!({"generated_at":900}),
        json!({"generated_at":1200}),
        json!({"node":{"type":"klipper"}}),
        json!({"sources":[]}),
    ] {
        let mut sample = local(1000.);
        for (k, v) in modify.as_object().unwrap() {
            sample[k] = v.clone();
        }
        let mut reader = RemoteCollectorReader::new(source()).unwrap();
        assert!(reader.validate(sample, 1000.).is_err());
    }
}
#[test]
fn remote_timestamp_cannot_hide_frozen_sequence() {
    let mut reader = RemoteCollectorReader::new(source()).unwrap();
    reader.validate(local(1000.), 1000.).unwrap();
    reader.validate(local(1006.), 1006.).unwrap();
    assert!(reader.validate(local(1016.), 1016.).is_err());
}
#[test]
fn remote_rebases_publication_but_preserves_original_source_age() {
    let config = NodeConfig {
        poll_interval_s: 30.,
        ttl_s: 90.,
        ..source()
    };
    let data = json!({"snapshot":local(1000.),"generated_at":1000});
    let before = data.clone();
    let sample = remote_feeds::remote_snapshot(&config, &state(1000.), Some(&data), 9, 1025.);
    assert_eq!(sample["generated_at"], 1025.);
    assert_eq!(sample["sequence"], 9);
    assert_eq!(sample["feed"]["age_s"], 25.);
    assert_eq!(sample["sources"]["proc"]["age_s"], 25.);
    assert_eq!(sample["host"]["cpu_pct"], 23);
    let expired = remote_feeds::remote_snapshot(&config, &state(1000.), Some(&data), 10, 1091.);
    assert!(expired["host"]["cpu_pct"].is_null());
    assert_eq!(expired["sources"].as_object().unwrap().len(), 1);
    assert_eq!(data, before);
}
#[test]
fn remote_registry_and_unknown_top_level_extensions_are_dropped() {
    let mut upstream = local(1000.);
    upstream["nodes"] = json!([{"url":"http://other"}]);
    upstream["private_extension"] = json!("sample-secret");
    let mut reader = RemoteCollectorReader::new(source()).unwrap();
    let result = reader.validate(upstream, 1000.).unwrap();
    assert!(result["snapshot"].get("nodes").is_none());
    assert!(result["snapshot"].get("private_extension").is_none());
}

fn store(nodes: Vec<Value>, now: f64) -> (tempfile::TempDir, SnapshotStore) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("snapshot.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({"schema":2,"generated_at":now,"sequence":2,"nodes":nodes}))
            .unwrap(),
    )
    .unwrap();
    (dir, SnapshotStore::new(path, false))
}
fn record(id: &str, kind: &str, sample: Value) -> Value {
    json!({"id":id,"type":kind,"name":id,"address":"192.0.2.2","snapshot":sample})
}
#[test]
fn empty_registry_is_readable_for_setup_but_has_no_default_snapshot() {
    let (_dir, s) = store(vec![], 1000.);
    assert_eq!(s.nodes(1000.).unwrap()["nodes"], json!([]));
    assert!(matches!(s.read(None, 1000.), Err(StoreError::NoNodes)));
}
#[test]
fn selected_registry_retains_only_its_sources_and_default_is_proxmox() {
    let p = printers::printer_snapshot(&source(), &SourceStatus::default(), None, 1, 1000.);
    let (_dir, s) = store(
        vec![
            record("klipper:p", "klipper", p),
            record("proxmox:vm", "proxmox", local(1000.)),
        ],
        1000.,
    );
    assert_eq!(s.read(None, 1000.).unwrap().0["node"]["id"], "proxmox:vm");
    let selected = s.read(Some("klipper:p"), 1000.).unwrap().0;
    assert!(selected["sources"].get("proc").is_none());
    assert_eq!(selected["nodes"].as_array().unwrap().len(), 2);
}
#[test]
fn duplicate_registry_and_invalid_selector_are_rejected() {
    let n = record("proxmox:vm", "proxmox", local(1000.));
    let (_dir, s) = store(vec![n.clone(), n], 1000.);
    assert!(matches!(s.nodes(1000.), Err(StoreError::Unavailable)));
    assert!(matches!(
        s.read(Some("../../private"), 1000.),
        Err(StoreError::InvalidSelector)
    ));
}
#[test]
fn node_status_is_independent_and_stale_nodes_are_offline() {
    assert_eq!(server::node_status(&local(1000.), 1000.), "healthy");
    assert_eq!(server::node_status(&local(1000.), 1016.), "offline");
    let mut s = local(1000.);
    s["sources"]["proc"]["ok"] = json!(false);
    assert_eq!(server::node_status(&s, 1000.), "offline");
}

async fn http(
    app: axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (u16, Value) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    if let Some(body) = body {
        req = req
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len());
    }
    let response = app
        .oneshot(
            req.body(Body::from(body.unwrap_or("").to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status().as_u16();
    let raw = axum::body::to_bytes(response.into_body(), 200 * 1024)
        .await
        .unwrap();
    let body = if raw.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&raw).unwrap()
    };
    (status, body)
}
#[tokio::test]
async fn http_roles_are_separate_and_control_paths_do_not_exist() {
    let now = glimdock_collector::epoch();
    let (_dir, s) = store(vec![record("proxmox:vm", "proxmox", local(now))], now);
    let state = HttpState::new(
        s,
        "d".repeat(43),
        Some("s".repeat(43)),
        PathBuf::from("/missing/config.sock"),
    )
    .unwrap();
    let app = server::router(state);
    for (path, token, expected) in [
        ("/api/v1/config", "d", 401),
        ("/api/v1/snapshot", "s", 401),
        ("/api/v1/snapshot", "d", 200),
        ("/api/v1/nodes", "d", 200),
        ("/api/v1/config?path=/private", "s", 404),
    ] {
        assert_eq!(
            http(app.clone(), "GET", path, Some(&token.repeat(43)), None)
                .await
                .0,
            expected
        );
    }
    assert_eq!(
        http(
            app,
            "POST",
            "/printer/print/cancel",
            Some(&"s".repeat(43)),
            Some("{}")
        )
        .await
        .0,
        404
    );
}
#[tokio::test]
async fn http_selector_contract_handles_unknown_duplicate_and_malformed() {
    let now = glimdock_collector::epoch();
    let (_dir, s) = store(vec![record("proxmox:vm", "proxmox", local(now))], now);
    let app = server::router(HttpState::new(s, "d".repeat(43), None, PathBuf::new()).unwrap());
    for (path, expected) in [
        ("/api/v1/snapshot?node=proxmox%3Avm", 200),
        ("/api/v1/snapshot?node=proxmox%3Aother", 404),
        ("/api/v1/snapshot?node=x", 400),
        ("/api/v1/snapshot?node=proxmox%3Avm&node=proxmox%3Avm", 400),
        ("/api/v1/snapshot?token=abc", 404),
        ("/api/v1/nodes?node=proxmox%3Avm", 404),
    ] {
        assert_eq!(
            http(app.clone(), "GET", path, Some(&"d".repeat(43)), None)
                .await
                .0,
            expected
        );
    }
}
#[tokio::test]
async fn http_empty_nodes_health_and_setup_remain_distinct() {
    let now = glimdock_collector::epoch();
    let (_dir, s) = store(vec![], now);
    let app = server::router(HttpState::new(s, "d".repeat(43), None, PathBuf::new()).unwrap());
    assert_eq!(
        http(app.clone(), "GET", "/healthz", None, None).await,
        (503, json!({"ok":false}))
    );
    assert_eq!(
        http(
            app.clone(),
            "GET",
            "/api/v1/nodes",
            Some(&"d".repeat(43)),
            None
        )
        .await
        .1["nodes"],
        json!([])
    );
    assert_eq!(
        http(app, "GET", "/api/v1/snapshot", Some(&"d".repeat(43)), None).await,
        (404, json!({"error":"no nodes"}))
    );
}
#[tokio::test]
async fn config_socket_protocol_restores_local_and_reports_conflicts() {
    let (dir, manager) = manager();
    let socket = dir.path().join("config.sock");
    let path = manager.path.clone();
    let child = tokio::spawn(server::config_service(path, socket.clone(), None, false));
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let (status, public) = server::forward_config("GET", None, &socket).await.unwrap();
    assert_eq!(status, 200);
    assert!(!public.to_string().contains("/private/nas-key"));
    let body = json!({"action":"upsert","version":public["version"],"node":{"type":"local-proxmox","name":"Home"}});
    assert_eq!(
        server::forward_config("POST", Some(body.clone()), &socket)
            .await
            .unwrap()
            .0,
        202
    );
    assert_eq!(
        server::forward_config("POST", Some(body), &socket)
            .await
            .unwrap()
            .0,
        409
    );
    child.abort();
}
#[tokio::test]
async fn disabled_local_collector_never_launches_probes() {
    let mut collector = runtime::Collector::new(Config {
        enable_proxmox: false,
        ..Default::default()
    })
    .unwrap();
    let sample = collector.sample().await.unwrap();
    assert_eq!(sample["nodes"], json!([]));
    assert_eq!(sample["sequence"], 1);
    collector.initialize().await;
    assert_eq!(collector.sample().await.unwrap()["sequence"], 2);
}
#[test]
fn identical_tokens_and_invalid_secret_files_are_rejected() {
    let (_dir, s) = store(vec![], 1000.);
    assert!(HttpState::new(s, "d".repeat(43), Some("d".repeat(43)), PathBuf::new()).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("key");
    fs::write(&path, "key\nInjected: header").unwrap();
    assert!(runtime::read_secret(path.to_str().unwrap(), 1).is_err());
}

async fn mock_server(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let child = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (origin, child)
}
#[tokio::test]
async fn moonraker_reader_uses_fixed_get_endpoints_and_validates_bounded_status() {
    use std::sync::{Arc, Mutex};
    let calls = Arc::new(Mutex::new(Vec::<(String, String, bool)>::new()));
    let capture = calls.clone();
    let app=axum::Router::new().fallback(move|request:axum::extract::Request|{let capture=capture.clone();async move{
        let path=request.uri().path();capture.lock().unwrap().push((request.method().to_string(),path.to_string(),request.headers().get("X-Api-Key").is_some()));
        let result=match path {"/printer/objects/list"=>json!({"objects":["webhooks","print_stats","display_status","extruder","heater_bed","temperature_sensor MCU","fan"]}),"/printer/info"=>json!({"hostname":"klipper"}),"/printer/objects/query"=>json!({"eventtime":1000,"status":{"webhooks":{"state":"ready"},"print_stats":{"state":"printing","filename":"parts/job.gcode","print_duration":600},"display_status":{"progress":0.5},"extruder":{"temperature":220,"target":220,"power":0.4}}}),"/server/files/metadata"=>json!({"estimated_time":1200}),_=>json!({})};axum::Json(json!({"result":result}))
    }});
    let (origin, child) = mock_server(app).await;
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("printer.key");
    fs::write(&key, "sample-printer-key\n").unwrap();
    let mut config = source();
    config.url = origin;
    config.api_key_file = key.to_string_lossy().into_owned();
    let mut reader = printers::MoonrakerReader::new(config).unwrap();
    let sample = reader.read().await.unwrap();
    assert_eq!(sample["printer"]["filename"], "job.gcode");
    assert_eq!(sample["printer"]["progress_pct"], 50.);
    assert_eq!(sample["printer"]["heaters"][0]["duty_pct"], 40.);
    let captured = calls.lock().unwrap();
    assert_eq!(captured.len(), 4);
    assert!(captured.iter().all(|(method, path, key)| method == "GET"
        && [
            "/printer/objects/list",
            "/printer/info",
            "/printer/objects/query",
            "/server/files/metadata"
        ]
        .contains(&path.as_str())
        && *key));
    child.abort();
}
#[tokio::test]
async fn upstream_redirects_never_forward_credentials_to_another_origin() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let visits = Arc::new(AtomicUsize::new(0));
    let count = visits.clone();
    let (target, target_child) = mock_server(axum::Router::new().fallback(move || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            axum::Json(local(glimdock_collector::epoch()))
        }
    }))
    .await;
    let destination = format!("{target}/api/v1/snapshot");
    let (origin, child) = mock_server(axum::Router::new().fallback(move || {
        let destination = destination.clone();
        async move { axum::response::Redirect::temporary(&destination) }
    }))
    .await;
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("display.key");
    fs::write(&key, format!("{}\n", "r".repeat(43))).unwrap();
    let mut config = source();
    config.url = origin;
    config.token_file = key.to_string_lossy().into_owned();
    let mut reader = RemoteCollectorReader::new(config).unwrap();
    assert!(reader.read().await.is_err());
    assert_eq!(visits.load(Ordering::SeqCst), 0);
    child.abort();
    target_child.abort();
}
#[tokio::test]
async fn network_response_bound_is_enforced_before_json_is_accepted() {
    let (origin, child) = mock_server(
        axum::Router::new()
            .fallback(|| async { axum::Json(json!({"unexpected":"x".repeat(MAX_PAYLOAD*2)})) }),
    )
    .await;
    let client = runtime::http_client(2.).unwrap();
    assert!(runtime::bounded_http_json(client.get(origin), MAX_PAYLOAD)
        .await
        .is_err());
    child.abort();
}
#[tokio::test]
async fn authenticated_http_config_reports_success_conflict_and_sanitized_bad_json() {
    let (dir, manager) = manager();
    let socket = dir.path().join("config.sock");
    let child = tokio::spawn(server::config_service(
        manager.path.clone(),
        socket.clone(),
        None,
        false,
    ));
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let now = glimdock_collector::epoch();
    let (_store_dir, s) = store(vec![], now);
    let app =
        server::router(HttpState::new(s, "d".repeat(43), Some("s".repeat(43)), socket).unwrap());
    let token = "s".repeat(43);
    let (status, public) = http(app.clone(), "GET", "/api/v1/config", Some(&token), None).await;
    assert_eq!(status, 200);
    let body=json!({"action":"upsert","version":public["version"],"node":{"type":"klipper","name":"Printer","url":"http://printer"}}).to_string();
    assert_eq!(
        http(
            app.clone(),
            "POST",
            "/api/v1/config",
            Some(&token),
            Some(&body)
        )
        .await
        .0,
        202
    );
    assert_eq!(
        http(
            app.clone(),
            "POST",
            "/api/v1/config",
            Some(&token),
            Some(&body)
        )
        .await
        .0,
        409
    );
    for raw in [
        "{\"sample-private-key\":",
        "{\"action\":\"upsert\",\"action\":\"delete\"}",
        "[]",
    ] {
        let (status, body) = http(
            app.clone(),
            "POST",
            "/api/v1/config",
            Some(&token),
            Some(raw),
        )
        .await;
        assert_eq!(status, 400);
        assert!(!body.to_string().contains("sample-private-key"));
    }
    child.abort();
}
#[test]
fn extreme_intervals_are_rejected_before_duration_conversion() {
    assert!(Config::from_value(json!({"interval_s":1e300})).is_err());
}
#[test]
fn generated_timestamp_must_be_a_json_number() {
    let mut sample = local(1000.);
    sample["generated_at"] = json!("1000");
    let mut reader = RemoteCollectorReader::new(source()).unwrap();
    assert!(reader.validate(sample, 1000.).is_err());
}
