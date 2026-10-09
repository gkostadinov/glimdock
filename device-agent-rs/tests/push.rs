use axum::{
    body::Body, extract::State, http::HeaderMap, response::Response, routing::post, Json, Router,
};
use glimdock_agent::{
    epoch, protocol,
    push::{
        self, DeliveryError, Enrollment, Identity, Latest, Options, Sample, StateStore, Transport,
    },
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

fn descriptor() -> Value {
    json!({"id":"server:fixture","name":"Fixture","type":"server","platform":"other","address":""})
}
fn snapshot(now: f64) -> Value {
    let mut value = protocol::empty_snapshot(&descriptor(), 1, now);
    value["sources"]["host"] = protocol::source_status(now, None, true);
    protocol::finish_snapshot(value, &descriptor()).unwrap()
}
fn private_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}
fn options(url: &str, directory: PathBuf) -> Options {
    Options {
        collector_url: url.into(),
        state_dir: directory,
        enrollment_key_file: None,
        re_enroll: false,
        allow_insecure_http: true,
        collector_ca_cert: None,
    }
}
fn enrolled(directory: &std::path::Path, url: &str) -> StateStore {
    let mut store = StateStore::open(directory, url).unwrap();
    store
        .enroll(Enrollment {
            schema: 1,
            node_id: "agent:fixture".into(),
            agent_id: store.identity.agent_id.clone(),
            interval_s: 3.,
            ttl_s: 15.,
        })
        .unwrap();
    store
}

#[test]
fn enrollment_identity_survives_retry_restart_and_is_collector_bound() {
    let directory = private_dir();
    let path = directory.path().canonicalize().unwrap();
    let initial = StateStore::open(&path, "https://collector.example/").unwrap();
    let agent = initial.identity.agent_id.clone();
    let token = initial.identity.agent_token.clone();
    assert!(push::is_hex_identity(&agent));
    assert!(push::is_hex_identity(&token));
    assert_ne!(agent, token);
    assert!(StateStore::open(&path, "https://collector.example/").is_err());
    drop(initial);
    let mut retried = StateStore::open(&path, "https://collector.example/").unwrap();
    assert_eq!(agent, retried.identity.agent_id);
    assert_eq!(token, retried.identity.agent_token);
    retried
        .enroll(Enrollment {
            schema: 1,
            node_id: "agent:assigned".into(),
            agent_id: agent.clone(),
            interval_s: 10.,
            ttl_s: 30.,
        })
        .unwrap();
    drop(retried);
    let saved = std::fs::read(path.join("agent.json")).unwrap();
    assert!(StateStore::open(&path, "https://different.example/").is_err());
    assert_eq!(saved, std::fs::read(path.join("agent.json")).unwrap());
    let restored = StateStore::open(&path, "https://collector.example/").unwrap();
    assert_eq!(restored.identity.node_id.as_deref(), Some("agent:assigned"));
    assert_eq!(restored.identity.agent_id, agent);
    assert_eq!(restored.identity.agent_token, token);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(path.join("agent.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn corrupted_or_unsafe_state_is_not_replaced() {
    let directory = private_dir();
    let path = directory.path().canonicalize().unwrap();
    drop(StateStore::open(&path, "https://collector.example/").unwrap());
    std::fs::write(path.join("agent.json"), br#"{"schema":1,"schema":1}"#).unwrap();
    assert!(StateStore::open(&path, "https://collector.example/").is_err());
    assert_eq!(
        std::fs::read(path.join("agent.json")).unwrap(),
        br#"{"schema":1,"schema":1}"#
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        std::fs::remove_file(path.join("agent.json")).unwrap();
        let target = path.join("outside");
        std::fs::write(&target, b"untouched").unwrap();
        symlink(&target, path.join("agent.json")).unwrap();
        assert!(StateStore::open(&path, "https://collector.example/").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
        std::fs::remove_file(path.join("agent.json")).unwrap();
        drop(StateStore::open(&path, "https://collector.example/").unwrap());
        std::fs::set_permissions(
            path.join("agent.json"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(StateStore::open(&path, "https://collector.example/").is_err());
    }
}

#[test]
fn enrollment_reply_cannot_change_identity_or_return_unsafe_cadence() {
    let directory = private_dir();
    let path = directory.path().canonicalize().unwrap();
    let mut store = StateStore::open(&path, "https://collector.example/").unwrap();
    for (agent, interval, ttl) in [
        ("wrong".to_string(), 3., 15.),
        (store.identity.agent_id.clone(), 10., 15.),
        (store.identity.agent_id.clone(), f64::NAN, 15.),
        (store.identity.agent_id.clone(), 3., 1000.),
    ] {
        assert!(store
            .enroll(Enrollment {
                schema: 1,
                node_id: "agent:a".into(),
                agent_id: agent,
                interval_s: interval,
                ttl_s: ttl
            })
            .is_err());
    }
    assert!(!store.identity.enrolled());
}

#[test]
fn explicit_reenrollment_rotates_access_keeps_identity_and_retries_durably() {
    let directory = private_dir();
    let path = directory.path().canonicalize().unwrap();
    let mut store = enrolled(&path, "https://collector.example/");
    let agent_id = store.identity.agent_id.clone();
    let old_token = store.identity.agent_token.clone();
    store.prepare_reenrollment().unwrap();
    assert_eq!(store.identity.agent_id, agent_id);
    assert_ne!(store.identity.agent_token, old_token);
    assert!(!store.identity.enrolled());
    let new_token = store.identity.agent_token.clone();
    drop(store);
    let mut resumed = StateStore::open(&path, "https://collector.example/").unwrap();
    resumed.prepare_reenrollment().unwrap();
    assert_eq!(resumed.identity.agent_id, agent_id);
    assert_eq!(resumed.identity.agent_token, new_token);
    resumed
        .enroll(Enrollment {
            schema: 1,
            node_id: "agent:fixture".into(),
            agent_id,
            interval_s: 3.,
            ttl_s: 15.,
        })
        .unwrap();
    assert_eq!(resumed.identity.node_id.as_deref(), Some("agent:fixture"));
}

#[test]
fn urls_require_verified_https_or_explicit_http_without_embedded_secrets() {
    assert_eq!(
        push::collector_base("https://collector.example", false)
            .unwrap()
            .as_str(),
        "https://collector.example/"
    );
    assert!(push::collector_base("http://127.0.0.1:8765", true).is_ok());
    for url in [
        "http://collector.example",
        "https://user:secret@collector.example",
        "https://collector.example/api/",
        "https://collector.example?token=secret",
        "https://collector.example/#secret",
        "file:///tmp/state",
    ] {
        assert!(push::collector_base(url, false).is_err(), "{url}");
    }
    let directory = private_dir();
    let cert = directory.path().join("bad.pem");
    std::fs::write(&cert, b"not a certificate").unwrap();
    let mut opts = options("https://collector.example", directory.path().into());
    opts.collector_ca_cert = Some(cert);
    assert!(Transport::new(&opts).is_err());
}

#[test]
fn latest_buffer_discards_out_of_order_and_expired_readings() {
    let latest = Latest::default();
    let now = epoch();
    latest.publish(Sample::new(5, now, snapshot(now)).unwrap());
    latest.publish(Sample::new(3, now, snapshot(now)).unwrap());
    let sample = latest.get().unwrap();
    assert_eq!(sample.sequence, 5);
    assert!(sample.fresh(now + 1., 15.));
    assert!(!sample.fresh(now + 16., 15.));
    assert!(!sample.fresh(now - 1., 15.));
    latest.publish(Sample::new(6, now + 2., snapshot(now + 2.)).unwrap());
    assert_eq!(latest.get().unwrap().sequence, 6);
    for attempt in 0..100 {
        for entropy in [0, 127, 255] {
            let delay = push::retry_delay(attempt, entropy);
            assert!(delay >= Duration::from_millis(500));
            assert!(delay <= Duration::from_secs(32));
        }
    }
}

#[derive(Default)]
struct Mock {
    requests: Mutex<Vec<(String, HeaderMap, Value)>>,
    response: Mutex<Option<(u16, Value)>>,
}
async fn enroll_mock(
    State(mock): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Json<Value> {
    mock.requests
        .lock()
        .unwrap()
        .push(("enroll".into(), headers, body.clone()));
    Json(
        json!({"schema":1,"agent_id":body["agent_id"],"node_id":"agent:fixture","interval_s":3,"ttl_s":15}),
    )
}
async fn push_mock(
    State(mock): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    mock.requests
        .lock()
        .unwrap()
        .push(("push".into(), headers, body.clone()));
    let (code,response) = mock.response.lock().unwrap().take().unwrap_or((200,json!({"schema":1,"accepted":true,"duplicate":false,"node_id":body["node_id"],"sequence":body["sequence"]})));
    Response::builder()
        .status(code)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&response).unwrap()))
        .unwrap()
}
async fn fixture() -> (String, Arc<Mock>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(Mock::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/api/v1/agents/enroll", post(enroll_mock))
        .route("/api/v1/agents/push", post(push_mock))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, state, task)
}

#[tokio::test]
async fn enrollment_and_delivery_use_separate_scoped_credentials_and_exact_metadata() {
    let (url, mock, server) = fixture().await;
    let directory = private_dir();
    let path = directory.path().canonicalize().unwrap();
    let mut store = StateStore::open(&path, &url).unwrap();
    let client = Transport::new(&options(&url, path.clone())).unwrap();
    let boot = push::random_identity().unwrap();
    let pairing = "p".repeat(64);
    let reply = client
        .enroll(&store.identity, &boot, &descriptor(), 3., &pairing)
        .await
        .unwrap();
    store.enroll(reply).unwrap();
    let now = epoch();
    let sample = Sample::new(1, now, snapshot(now)).unwrap();
    client.push(&store.identity, &boot, &sample).await.unwrap();
    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].1["authorization"], format!("Bearer {pairing}"));
    assert_eq!(
        requests[1].1["authorization"],
        format!("Bearer {}", store.identity.agent_token)
    );
    assert_eq!(requests[0].2["agent_token"], store.identity.agent_token);
    assert_eq!(requests[0].2["kind"], "server");
    assert_eq!(requests[0].2["interval_s"], 3.);
    let body = &requests[1].2;
    assert_eq!(body["boot_id"], boot);
    assert_eq!(body["sequence"], 1);
    assert_eq!(body["sampled_at"], body["snapshot"]["generated_at"]);
    assert_eq!(body["sequence"], body["snapshot"]["sequence"]);
    assert_eq!(body["snapshot"]["node"]["id"], "agent:fixture");
    assert!(!serde_json::to_string(&body)
        .unwrap()
        .contains(&store.identity.agent_token));
    assert!(!serde_json::to_string(&body).unwrap().contains(&pairing));
    server.abort();
}

#[tokio::test]
async fn native_snmp_and_json_adapters_deliver_platform_readings_over_push() {
    use glimdock_agent::{
        json_device::{JsonDeviceCollector, JsonDeviceConfig},
        snmp::{SnmpCollector, SnmpConfig, HOST_UPTIME, SYS_UPTIME},
        Collector,
    };
    let (url, mock, server) = fixture().await;
    let directory = private_dir();
    let path = directory.path().canonicalize().unwrap();
    let fixture_path = path.clone();
    let samples = tokio::task::spawn_blocking(move || {
        let path = fixture_path;
    let credentials = path.join("snmp.credentials.json");
    std::fs::write(
        &credentials,
        br#"{"version":"2c","community":"fixture-community"}"#,
    )
    .unwrap();
    let cfg: SnmpConfig = serde_json::from_value(json!({"id":"router","name":"Gateway","platform":"router","address":"192.0.2.1","credential_file":credentials,"cpu_oids":["1.3.6.1.2.1.25.3.3.1.2.1"]})).unwrap();
    let mut snmp = SnmpCollector::with_runner(cfg, |request| {
        Ok(request
            .args
            .iter()
            .skip(7)
            .map(|oid| {
                format!(
                    "{oid} {}",
                    if matches!(oid.as_str(), SYS_UPTIME | HOST_UPTIME) {
                        10000
                    } else {
                        42
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    })
    .unwrap();
    let now = epoch();
    let snmp_sample = snmp.sample(now, 1.).unwrap();
    assert_eq!(snmp_sample["host"]["cpu_pct"], 42.);
    let cfg: JsonDeviceConfig = serde_json::from_value(json!({"id":"json-router","name":"JSON gateway","platform":"router","url":"http://127.0.0.1/metrics","host":[{"field":"cpu_pct","path":["cpu"]}],"sensors":[{"id":"board","kind":"temperature","unit":"C","path":["temperature"]}]})).unwrap();
    let mut json_adapter = JsonDeviceCollector::new(cfg).unwrap();
    let parsed = json_adapter.validate_document(br#"{"cpu":31,"temperature":48}"#, now);
    let json_sample = json_adapter.sample_document(parsed, now).unwrap();
    assert_eq!(json_sample["sensors"][0]["value"], 48.);
        [(0, snmp.descriptor(), snmp_sample), (1, json_adapter.descriptor(), json_sample)]
    }).await.unwrap();
    for (index, descriptor, snapshot) in samples {
        let state_path = path.join(format!("agent-{index}"));
        let mut store = StateStore::open(&state_path, &url).unwrap();
        let client = Transport::new(&options(&url, state_path)).unwrap();
        let boot = push::random_identity().unwrap();
        let result = client
            .enroll(&store.identity, &boot, &descriptor, 3., &"p".repeat(64))
            .await
            .unwrap();
        store.enroll(result).unwrap();
        client
            .push(
                &store.identity,
                &boot,
                &Sample::new(1, snapshot["generated_at"].as_f64().unwrap(), snapshot).unwrap(),
            )
            .await
            .unwrap();
    }
    let requests = mock.requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    for request in requests.iter().filter(|(route, _, _)| route == "push") {
        assert_eq!(request.2["snapshot"]["node"]["platform"], "router");
        assert_eq!(request.2["snapshot"]["node"]["type"], "server");
        assert!(!serde_json::to_string(&request.2)
            .unwrap()
            .contains("fixture-community"));
    }
    server.abort();
}

#[tokio::test]
async fn delivery_retries_transient_errors_and_rejects_auth_redirect_or_wrong_ack() {
    let (url, mock, server) = fixture().await;
    let directory = private_dir();
    let path = directory.path().canonicalize().unwrap();
    let mut store = enrolled(&path, &url);
    let client = Transport::new(&options(&url, path.clone())).unwrap();
    let boot = push::random_identity().unwrap();
    let now = epoch();
    let sample = Sample::new(8, now, snapshot(now)).unwrap();
    for (code, expected) in [
        (503, DeliveryError::Retry),
        (429, DeliveryError::Retry),
        (401, DeliveryError::Unauthorized),
        (403, DeliveryError::Unauthorized),
        (409, DeliveryError::Rejected),
        (302, DeliveryError::Rejected),
    ] {
        *mock.response.lock().unwrap() = Some((code, json!({"error":"private remote failure"})));
        assert_eq!(
            client
                .push(&store.identity, &boot, &sample)
                .await
                .unwrap_err(),
            expected
        );
    }
    *mock.response.lock().unwrap() = Some((
        200,
        json!({"schema":1,"accepted":true,"node_id":"agent:wrong","sequence":8}),
    ));
    assert_eq!(
        client
            .push(&store.identity, &boot, &sample)
            .await
            .unwrap_err(),
        DeliveryError::Rejected
    );
    *mock.response.lock().unwrap() = Some((200, json!({"padding":"x".repeat(5000)})));
    assert_eq!(
        client
            .push(&store.identity, &boot, &sample)
            .await
            .unwrap_err(),
        DeliveryError::Rejected
    );
    client.push(&store.identity, &boot, &sample).await.unwrap();
    *mock.response.lock().unwrap() = Some((
        200,
        json!({"schema":1,"accepted":true,"node_id":"agent:fixture","sequence":8,"interval_s":10,"ttl_s":30}),
    ));
    let ack = client.push(&store.identity, &boot, &sample).await.unwrap();
    store
        .update_cadence(ack.interval_s.unwrap(), ack.ttl_s.unwrap())
        .unwrap();
    drop(store);
    let restored = StateStore::open(&path, &url).unwrap();
    assert_eq!(restored.identity.interval_s, Some(10.));
    assert_eq!(restored.identity.ttl_s, Some(30.));
    server.abort();
}

#[tokio::test]
async fn https_verification_rejects_plaintext_endpoint() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        use tokio::io::AsyncWriteExt;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
            .await
            .unwrap();
    });
    let directory = private_dir();
    let path = directory.path().canonicalize().unwrap();
    let store = enrolled(&path, &url);
    let client = Transport::new(&options(&url, path)).unwrap();
    let now = epoch();
    assert_eq!(
        client
            .push(
                &store.identity,
                &push::random_identity().unwrap(),
                &Sample::new(1, now, snapshot(now)).unwrap()
            )
            .await
            .unwrap_err(),
        DeliveryError::Retry
    );
    server.await.unwrap();
}

#[cfg(unix)]
#[test]
fn process_keeps_sampling_during_outage_resumes_identity_without_listener_and_stops_on_sigterm() {
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        process::{Command, Stdio},
        thread,
        time::Instant,
    };
    struct Process(std::process::Child);
    impl Drop for Process {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn request(socket: &mut TcpStream) -> Value {
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut header = Vec::new();
        let mut byte = [0u8; 1];
        while !header.ends_with(b"\r\n\r\n") {
            assert!(header.len() < 8192);
            socket.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
        }
        let header = String::from_utf8(header).unwrap();
        let size = header
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .and_then(|n| n.parse::<usize>().ok())
            })
            .unwrap();
        assert!(size <= push::MAX_PUSH_BODY);
        let mut body = vec![0u8; size];
        socket.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }
    fn reply(socket: &mut TcpStream, code: u16, body: Value) {
        let raw = serde_json::to_string(&body).unwrap();
        write!(socket,"HTTP/1.1 {code} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{raw}",raw.len()).unwrap();
    }
    fn terminate(process: &mut Process) {
        assert_eq!(
            unsafe { libc::kill(process.0.id() as i32, libc::SIGTERM) },
            0
        );
        let began = Instant::now();
        loop {
            if let Some(status) = process.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(began.elapsed() < Duration::from_secs(3));
            thread::sleep(Duration::from_millis(25));
        }
    }
    let directory = private_dir();
    let path = directory.path().canonicalize().unwrap();
    let state = path.join("state");
    let pairing = path.join("pairing.key");
    std::fs::write(&pairing, "p".repeat(64)).unwrap();
    let config = path.join("host.json");
    std::fs::write(
        &config,
        br#"{"id":"fixture","name":"Fixture","interval_s":1}"#,
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let unused = TcpListener::bind("127.0.0.1:0").unwrap();
    let unused_address = unused.local_addr().unwrap();
    drop(unused);
    let spawn = |pair: bool, re_enroll: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_glimdock-agent"));
        cmd.args([
            "--collector-url",
            &url,
            "--allow-insecure-http",
            "--state-dir",
            state.to_str().unwrap(),
            "--config",
            config.to_str().unwrap(),
            "--port",
            &unused_address.port().to_string(),
        ]);
        if pair {
            cmd.args(["--enrollment-key-file", pairing.to_str().unwrap()]);
        }
        if re_enroll {
            cmd.arg("--re-enroll");
        }
        Process(
            cmd.stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    };
    let mut process = spawn(true, false);
    let began = Instant::now();
    let mut enrolled_agent = None;
    let mut boot = Value::Null;
    let mut first_sequence = None;
    let mut attempts = 0;
    loop {
        let (mut socket, _) = match listener.accept() {
            Ok(socket) => socket,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    began.elapsed() < Duration::from_secs(15),
                    "Agent failed to reconnect"
                );
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(error) => panic!("{error}"),
        };
        let body = request(&mut socket);
        if body.get("snapshot").is_none() {
            enrolled_agent = Some(body["agent_id"].clone());
            reply(
                &mut socket,
                200,
                json!({"schema":1,"agent_id":body["agent_id"],"node_id":"agent:process","interval_s":1,"ttl_s":15}),
            );
            continue;
        }
        assert_eq!(body["agent_id"], enrolled_agent.clone().unwrap());
        if boot.is_null() {
            boot = body["boot_id"].clone();
        }
        let sequence = body["sequence"].as_u64().unwrap();
        let initial = *first_sequence.get_or_insert(sequence);
        attempts += 1;
        assert_eq!(body["sampled_at"], body["snapshot"]["generated_at"]);
        assert!(epoch() - body["sampled_at"].as_f64().unwrap() < 2.);
        if attempts < 4 {
            reply(
                &mut socket,
                if attempts == 2 { 403 } else { 503 },
                json!({"error":"outage"}),
            );
            continue;
        }
        assert!(
            sequence > initial,
            "Sampling stalled while network retries ran"
        );
        reply(
            &mut socket,
            200,
            json!({"schema":1,"accepted":true,"node_id":"agent:process","sequence":sequence}),
        );
        break;
    }
    assert!(
        TcpStream::connect(unused_address).is_err(),
        "Push mode opened a listener"
    );
    terminate(&mut process);
    std::fs::remove_file(&pairing).unwrap();
    let saved: Identity =
        serde_json::from_slice(&std::fs::read(state.join("agent.json")).unwrap()).unwrap();
    assert_eq!(saved.agent_id, enrolled_agent.unwrap().as_str().unwrap());
    let mut process = spawn(false, false);
    let began = Instant::now();
    loop {
        let (mut socket, _) = match listener.accept() {
            Ok(socket) => socket,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(began.elapsed() < Duration::from_secs(5));
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(error) => panic!("{error}"),
        };
        let body = request(&mut socket);
        assert!(
            body.get("snapshot").is_some(),
            "Restart unexpectedly required enrollment"
        );
        assert_eq!(body["agent_id"], saved.agent_id);
        assert_ne!(body["boot_id"], boot);
        assert_eq!(body["sequence"], 1);
        reply(
            &mut socket,
            200,
            json!({"schema":1,"accepted":true,"node_id":"agent:process","sequence":1}),
        );
        break;
    }
    terminate(&mut process);
    std::fs::write(&pairing, "q".repeat(64)).unwrap();
    let mut process = spawn(true, true);
    let began = Instant::now();
    let mut enrollment_seen = false;
    loop {
        let (mut socket, _) = match listener.accept() {
            Ok(socket) => socket,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(began.elapsed() < Duration::from_secs(5));
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            Err(error) => panic!("{error}"),
        };
        let body = request(&mut socket);
        assert_eq!(body["agent_id"], saved.agent_id);
        if body.get("snapshot").is_none() {
            assert_ne!(body["agent_token"], saved.agent_token);
            enrollment_seen = true;
            reply(
                &mut socket,
                200,
                json!({"schema":1,"agent_id":body["agent_id"],"node_id":"agent:process","interval_s":1,"ttl_s":15}),
            );
            continue;
        }
        assert!(enrollment_seen, "Explicit recovery skipped re-enrollment");
        assert_eq!(body["node_id"].as_str(), saved.node_id.as_deref());
        reply(
            &mut socket,
            200,
            json!({"schema":1,"accepted":true,"node_id":"agent:process","sequence":body["sequence"]}),
        );
        break;
    }
    terminate(&mut process);
}
