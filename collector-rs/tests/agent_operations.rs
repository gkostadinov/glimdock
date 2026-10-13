use axum::{body::Body, http::Request};
use glimdock_collector::{
    operations::{self, OperationsConfig},
    server::{self, HttpState, SnapshotStore},
};
use serde_json::{json, Value};
use tower::ServiceExt;
fn fixture() -> Value {
    serde_json::from_str(include_str!("../../examples/agent-operations/demo.json")).unwrap()
}
#[test]
fn authored_contract_is_strict_and_preserves_unavailable_measurements() {
    let doc = fixture();
    operations::validate(&doc).unwrap();
    let minimal: Value =
        serde_json::from_str(include_str!("../../examples/agent-operations/minimal.json")).unwrap();
    operations::validate(&minimal).unwrap();
    assert!(minimal["systems"][0]["usage"]["cost"].is_null());
    for (pointer, value) in [
        ("/agents/0/usage/input_tokens", json!(false)),
        ("/systems/0/usage/cost", json!(-1)),
        ("/systems/0/usage/currency", Value::Null),
        ("/tasks/0/progress_pct", json!(101)),
        ("/agents/0/source_id", json!("missing")),
        ("/limits/counts/agents", json!(200)),
    ] {
        let mut bad = doc.clone();
        *bad.pointer_mut(pointer).unwrap() = value;
        assert!(operations::validate(&bad).is_err(), "{pointer}");
    }
    let mut private = doc.clone();
    private["agents"][0]["prompt"] = json!("do not export");
    assert!(operations::validate(&private).is_err());
    let mut duplicate = doc.clone();
    duplicate["agents"][1] = duplicate["agents"][0].clone();
    assert!(operations::validate(&duplicate).is_err());
}
#[test]
fn operations_configuration_is_explicit_and_transport_safe() {
    let base = OperationsConfig {
        url: "https://coordinator.example/v1/snapshot".into(),
        ..Default::default()
    };
    base.validate().unwrap();
    for url in [
        "file:///private/state",
        "https://key:password@coordinator.example/",
        "http://coordinator.example/v1/snapshot",
        "https://coordinator.example/?token=secret",
    ] {
        let config = OperationsConfig {
            url: url.into(),
            ..base.clone()
        };
        assert!(config.validate().is_err());
    }
    OperationsConfig {
        url: "http://127.0.0.1:8784/v1/snapshot".into(),
        ..base.clone()
    }
    .validate()
    .unwrap();
    OperationsConfig {
        url: "http://coordinator.example/v1/snapshot".into(),
        allow_insecure_http: true,
        ..base.clone()
    }
    .validate()
    .unwrap();
    let file = OperationsConfig {
        snapshot_file: "/PRIVATE/STATE/operations.json".into(),
        ..Default::default()
    };
    file.validate().unwrap();
    assert!(OperationsConfig {
        url: base.url,
        ..file
    }
    .validate()
    .is_err());
}
#[tokio::test]
async fn operations_endpoint_uses_reader_role_and_rejects_controls() {
    let folder = tempfile::tempdir().unwrap();
    let hardware = folder.path().join("snapshot.json");
    let doc = fixture();
    std::fs::write(
        operations::snapshot_path(&hardware),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();
    let display = "d".repeat(64);
    let setup = "s".repeat(64);
    let state = HttpState::new(
        SnapshotStore::new(hardware, false),
        display.clone(),
        Some(setup.clone()),
        folder.path().join("config.sock"),
    )
    .unwrap();
    let app = server::router(state);
    for (method, key, status) in [
        ("GET", None, 401),
        ("GET", Some(setup.as_str()), 401),
        ("GET", Some(display.as_str()), 200),
        ("POST", Some(display.as_str()), 405),
    ] {
        let mut request = Request::builder()
            .method(method)
            .uri("/api/v1/agent-operations");
        if let Some(key) = key {
            request = request.header("Authorization", format!("Bearer {key}"));
        }
        let response = app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        if status == 200 {
            let bytes = axum::body::to_bytes(response.into_body(), 49152)
                .await
                .unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), doc);
        }
    }
}
