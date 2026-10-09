use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use glimdock_agent::{
    config, epoch,
    json_device::{JsonDeviceCollector, JsonDeviceConfig},
    protocol::{self, Rates},
    server::{self, ServerState},
    Collector, MAX_PAYLOAD,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
use tower::ServiceExt;

fn descriptor() -> Value {
    json!({"id":"server:router","name":"Router","address":"192.0.2.1","type":"server","platform":"router"})
}
fn snapshot(now: f64) -> Value {
    let mut s = protocol::empty_snapshot(&descriptor(), 1, now);
    s["sources"]["device_api"] = protocol::source_status(now, None, true);
    protocol::finish_snapshot(s, &descriptor()).unwrap()
}
fn adapter(extra: Value) -> JsonDeviceCollector {
    let mut cfg = json!({"id":"router","platform":"router","url":"http://127.0.0.1/metrics","host":[{"field":"cpu_pct","path":["cpu"]}],"sensors":[{"id":"board","name":"Board","kind":"temperature","unit":"C","path":["temp"],"scale":0.1,"high":70,"crit":90}]});
    for (key, value) in extra.as_object().unwrap() {
        cfg[key] = value.clone();
    }
    JsonDeviceCollector::new(serde_json::from_value(cfg).unwrap()).unwrap()
}
fn mapped_sample(adapter: &mut JsonDeviceCollector, raw: &[u8], now: f64) -> Value {
    let parsed = adapter.validate_document(raw, now);
    adapter.sample_document(parsed, now).unwrap()
}

#[test]
fn strict_config_and_response_json() {
    for raw in [
        br#"{"x":1,"x":2}"#.as_slice(),
        br#"{"x":{"y":1,"y":2}}"#,
        br#"{"x":NaN}"#,
        br#"{"x":1e999}"#,
        br#"{} trailing"#,
    ] {
        assert!(config::strict_json(raw).is_err());
    }
    let deep = format!("{}0{}", "[".repeat(140), "]".repeat(140));
    assert!(config::strict_json(deep.as_bytes()).is_err());
    assert!(config::strict_json(br#"{"x":18446744073709551615}"#).is_ok());
}

#[test]
fn published_examples_use_native_typed_configs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let host = config::read_config::<glimdock_agent::host::HostConfig>(
        &root.join("examples/agents/host.json"),
    )
    .unwrap();
    host.validate().unwrap();
    let mut json =
        config::read_config::<JsonDeviceConfig>(&root.join("examples/agents/json.json"))
            .unwrap();
    json.validate().unwrap();
    let snmp = config::read_config::<glimdock_agent::snmp::SnmpConfig>(
        &root.join("examples/agents/snmp.json"),
    )
    .unwrap();
    snmp.validate().unwrap();
}

#[test]
fn mapping_types_conversions_and_thresholds() {
    let mut c = adapter(json!({}));
    let s = mapped_sample(&mut c, br#"{"cpu":42,"temp":925}"#, 100.);
    assert_eq!(s["host"]["cpu_pct"], 42.);
    assert_eq!(s["sensors"][0]["value"], 92.5);
    assert_eq!(s["alerts"][0]["severity"], "critical");
    assert_eq!(s["node"]["type"], "server");
    assert_eq!(s["node"]["platform"], "router");
    assert_eq!(s["node"]["status"], "degraded");
    let s = mapped_sample(&mut c, br#"{"cpu":"42","temp":true}"#, 103.);
    assert!(s["host"]["cpu_pct"].is_null());
    assert!(s["sensors"][0]["value"].is_null());
    assert_eq!(s["node"]["status"], "offline");
    let s = mapped_sample(&mut c, br#"{"cpu":101,"temp":650}"#, 106.);
    assert!(s["host"]["cpu_pct"].is_null());
    assert_eq!(s["node"]["status"], "degraded");
}

#[test]
fn numeric_strings_are_opt_in_and_paths_are_literal() {
    let mut c = adapter(
        json!({"host":[{"field":"cpu_pct","path":["cpu"],"parse":"numeric-string"}],"sensors":[{"id":"voltage","path":["a.b",0,"v"],"parse":"numeric-string","scale":0.001}]}),
    );
    let s = mapped_sample(&mut c, br#"{"cpu":" 4.2e1 ","a.b":[{"v":"12000"}]}"#, 100.);
    assert_eq!(s["host"]["cpu_pct"], 42.);
    assert_eq!(s["sensors"][0]["value"], 12.);
    for bad in ["NaN", "Infinity", "42%", "0x20", "1e999"] {
        let raw = serde_json::to_vec(&json!({"cpu":bad})).unwrap();
        let s = mapped_sample(&mut c, &raw, 103.);
        assert!(s["host"]["cpu_pct"].is_null());
    }
}

#[test]
fn inconsistent_memory_and_overflow_are_unknown() {
    let mut c = adapter(
        json!({"host":[{"field":"mem_used_bytes","path":["used"]},{"field":"mem_total_bytes","path":["total"]}],"sensors":[{"id":"scaled","path":["x"],"scale":1e308}]}),
    );
    let s = mapped_sample(&mut c, br#"{"used":200,"total":100,"x":100}"#, 100.);
    assert!(s["host"]["mem_used_bytes"].is_null());
    assert_eq!(s["host"]["mem_total_bytes"], 100.);
    assert!(s["sensors"][0]["value"].is_null());
}

#[test]
fn frozen_expired_and_invalid_documents_clear_readings() {
    let mut c = adapter(json!({"timestamp_path":["time"],"sequence_path":["seq"]}));
    let raw = br#"{"cpu":42,"temp":500,"time":100,"seq":1}"#;
    let s = mapped_sample(&mut c, raw, 100.);
    assert_eq!(s["node"]["status"], "healthy");
    assert!(c.validate_document(raw, 116.).is_err());
    let fresh_stamp = br#"{"cpu":42,"temp":500,"time":116,"seq":1}"#;
    assert!(c.validate_document(fresh_stamp, 116.).is_err());
    let s = mapped_sample(&mut c, fresh_stamp, 116.);
    assert!(s["host"]["cpu_pct"].is_null());
    assert!(s["sensors"].as_array().unwrap().is_empty());
    assert_eq!(s["node"]["status"], "offline");
    for raw in [
        br#"{"cpu":42,"cpu":43}"#.as_slice(),
        br#"{"time":100,"seq":true}"#,
        br#"{"time":200,"seq":2}"#,
        br#"42"#,
    ] {
        assert!(c.validate_document(raw, 100.).is_err());
    }
    assert!(c
        .validate_document(&vec![b' '; MAX_PAYLOAD + 1], 100.)
        .is_err());
}

#[test]
fn clock_rollback_cannot_refresh_a_frozen_sequence() {
    let mut c = adapter(json!({"sequence_path":["seq"]}));
    let raw = br#"{"cpu":42,"temp":500,"seq":1}"#;
    assert!(c.validate_document(raw, 100.).is_ok());
    assert!(c.validate_document(raw, 50.).is_err());
    let s = mapped_sample(&mut c, raw, 50.);
    assert_eq!(s["node"]["status"], "offline");
    assert!(s["host"]["cpu_pct"].is_null());
    assert!(c.sample_document(Ok((json!({}), 100.)), f64::NAN).is_err());
}

#[test]
fn config_rejects_scripts_inline_credentials_and_unbounded_paths() {
    for extra in [
        json!({"url":"http://user:secret@example.com/"}),
        json!({"url":"file:///tmp/metrics"}),
        json!({"token_file":"relative"}),
        json!({"host":[{"field":"arbitrary","path":["x"]}]}),
        json!({"host":[{"field":"cpu_pct","path":[]}]}),
        json!({"host":[{"field":"cpu_pct","path":["x"],"parse":"eval"}]}),
        json!({"sensors":[{"id":"x","path":["x"]},{"id":"x","path":["y"]}]}),
    ] {
        let mut cfg = json!({"url":"http://localhost/","host":[{"field":"cpu_pct","path":["x"]}]});
        for (key, value) in extra.as_object().unwrap() {
            cfg[key] = value.clone();
        }
        let mut cfg: JsonDeviceConfig = serde_json::from_value(cfg).unwrap();
        assert!(cfg.validate().is_err());
    }
    assert!(serde_json::from_value::<JsonDeviceConfig>(json!({"command":"anything"})).is_err());
}

fn http_fixture(response: String) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while request.len() < 8192 {
            if socket.read(&mut byte).unwrap() == 0 {
                break;
            }
            request.push(byte[0]);
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        socket.write_all(response.as_bytes()).unwrap();
        String::from_utf8(request).unwrap()
    });
    (format!("http://{address}/metrics"), thread)
}

#[test]
fn native_json_get_uses_private_bearer_and_rejects_redirect() {
    let token = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(token.path(), "private-test-token").unwrap();
    let body = r#"{"cpu":42,"temp":500}"#;
    let(url,request)=http_fixture(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()));
    let mut c = adapter(json!({"url":url,"token_file":token.path()}));
    let s = c.sample(100., 1.).unwrap();
    assert_eq!(s["host"]["cpu_pct"], 42.);
    let request = request.join().unwrap().to_ascii_lowercase();
    assert!(request.starts_with("get /metrics http/1.1"));
    assert!(request.contains("authorization: bearer private-test-token"));
    assert!(!serde_json::to_string(&s)
        .unwrap()
        .contains("private-test-token"));
    let(url,request)=http_fixture("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into());
    let mut c = adapter(json!({"url":url}));
    let s = c.sample(100., 1.).unwrap();
    assert_eq!(s["node"]["status"], "offline");
    request.join().unwrap();
}

#[test]
fn counter_precision_reset_and_missing_values() {
    let mut rates = Rates::default();
    let base = (1u64 << 60) + 5;
    assert_eq!(rates.rate("wan", Some(base), 1.), None);
    assert_eq!(rates.rate("wan", Some(base + 9), 4.), Some(3.));
    assert_eq!(rates.rate("wan", Some(1), 5.), None);
    assert_eq!(rates.rate("wan", None, 6.), None);
    assert_eq!(rates.rate("wan", Some(3), 7.), None);
}

#[test]
fn inventory_caps_preserve_counts_and_payload_bound() {
    let mut s = snapshot(100.);
    s["sensors"] = json!((0..100)
        .map(|i| json!({"id":format!("s{i}"),"name":"x".repeat(1000),"value":42}))
        .collect::<Vec<_>>());
    let s = protocol::finish_snapshot(s, &descriptor()).unwrap();
    assert_eq!(s["limits"]["counts"]["sensors"], 100);
    assert!(s["sensors"].as_array().unwrap().len() < 64);
    assert!(s["limits"]["truncated"]["sensors"].as_u64().unwrap() > 36);
    assert!(serde_json::to_vec(&s).unwrap().len() <= MAX_PAYLOAD);
    assert_eq!(s["node"]["status"], "degraded");
}

fn state() -> ServerState {
    ServerState::new(snapshot(epoch()), "t".repeat(32), 3.).unwrap()
}
async fn fetch(
    state: ServerState,
    method: &str,
    path: &str,
    auth: bool,
) -> (StatusCode, Value, axum::http::HeaderMap) {
    let mut request = Request::builder().method(method).uri(path);
    if auth {
        request = request.header("Authorization", format!("Bearer {}", "t".repeat(32)));
    }
    let response = server::router(state)
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let raw = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        if raw.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&raw).unwrap()
        },
        headers,
    )
}

#[tokio::test]
async fn endpoint_requires_auth_and_is_read_only() {
    let s = state();
    assert_eq!(
        fetch(s.clone(), "GET", "/api/v1/snapshot", false).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, body, headers) = fetch(s.clone(), "GET", "/api/v1/snapshot", true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["node"]["platform"], "router");
    assert_eq!(headers["cache-control"], "no-store");
    for path in ["/api/v1/setup", "/api/v1/control", "/api/v1/actions"] {
        assert_eq!(
            fetch(s.clone(), "GET", path, true).await.0,
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        fetch(s.clone(), "POST", "/api/v1/snapshot", true).await.0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    let (status, body, _) = fetch(s, "GET", "/healthz", false).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"ok":true}));
}

#[tokio::test]
async fn endpoint_selector_head_and_metadata_contract() {
    let s = state();
    assert_eq!(
        fetch(
            s.clone(),
            "GET",
            "/api/v1/snapshot?node=server%3Arouter",
            true
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        fetch(s.clone(), "GET", "/api/v1/snapshot?node=server:other", true)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    for path in [
        "/api/v1/snapshot?node=server:router&node=server:router",
        "/api/v1/snapshot?node=../../etc/passwd",
    ] {
        assert_eq!(
            fetch(s.clone(), "GET", path, true).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    for path in [
        "/healthz?x=1",
        "/api/v1/nodes?node=server:router",
        "/api/v1/snapshot?command=reboot",
    ] {
        assert_eq!(
            fetch(s.clone(), "GET", path, true).await.0,
            StatusCode::NOT_FOUND
        );
    }
    let (status, body, headers) = fetch(s.clone(), "HEAD", "/api/v1/snapshot", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_null());
    assert!(
        headers["content-length"]
            .to_str()
            .unwrap()
            .parse::<usize>()
            .unwrap()
            > 0
    );
    let (_, body, _) = fetch(s, "GET", "/api/v1/nodes", true).await;
    assert_eq!(body["schema"], 2);
    assert_eq!(body["nodes"][0]["id"], "server:router");
    assert!(body.get("host").is_none());
}

#[tokio::test]
async fn endpoint_duplicate_credentials_and_unavailable_samples() {
    let s = state();
    let request = Request::builder()
        .uri("/api/v1/snapshot")
        .header("Authorization", format!("Bearer {}", "t".repeat(32)))
        .header("Authorization", format!("Bearer {}", "t".repeat(32)))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        server::router(s.clone())
            .oneshot(request)
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    for sample in [
        snapshot(epoch() - 100.),
        protocol::finish_snapshot(
            protocol::empty_snapshot(&descriptor(), 2, epoch()),
            &descriptor(),
        )
        .unwrap(),
        snapshot(epoch() + 100.),
    ] {
        s.publish(sample).unwrap();
        let (status, body, _) = fetch(s.clone(), "GET", "/api/v1/snapshot", true).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(body.get("host").is_none());
        assert_eq!(
            fetch(s.clone(), "GET", "/healthz", false).await.1,
            json!({"ok":false})
        );
    }
}

#[test]
fn native_http_listener_expires_incomplete_headers() {
    use std::{
        net::TcpStream,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    struct Process(std::process::Child);
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let token = directory.path().join("display.token");
    std::fs::write(&token, "t".repeat(32)).unwrap();
    let cfg = directory.path().join("host.json");
    std::fs::write(
        &cfg,
        br#"{"id":"fixture","name":"Fixture","address":"192.0.2.20"}"#,
    )
    .unwrap();
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let _process = Process(
        Command::new(env!("CARGO_BIN_EXE_glimdock-agent"))
            .args([
                "--config",
                cfg.to_str().unwrap(),
                "--token-file",
                token.to_str().unwrap(),
                "--port",
                &address.port().to_string(),
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    let mut socket = loop {
        if let Ok(socket) = TcpStream::connect(address) {
            break socket;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "Native HTTP service did not start"
        );
        thread::sleep(Duration::from_millis(50));
    };
    socket
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    socket
        .write_all(b"GET /healthz HTTP/1.1\r\nHost: fixture\r\n")
        .unwrap();
    let began = Instant::now();
    let mut byte = [0u8; 1];
    let result = socket.read(&mut byte);
    assert!(
        matches!(result, Ok(0))
            || result.as_ref().is_err_and(|error| !matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            )),
        "Incomplete headers were not closed: {result:?}"
    );
    assert!(began.elapsed() < Duration::from_secs(8));
}
