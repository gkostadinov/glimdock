use axum::{body::Body, extract::ConnectInfo, http::Request};
use fs2::FileExt;
use glimdock_collector::{
    config::Config,
    hub::{Hub, RunOptions, State},
    runtime,
    server::{self, HttpState, SnapshotStore},
};
use serde_json::{json, Value};
use std::{
    fs,
    net::SocketAddr,
    os::unix::fs::{symlink, PermissionsExt},
    time::Duration,
};
use tokio::sync::{oneshot, watch};
use tower::ServiceExt;

fn private_write(path: &std::path::Path, value: Value) {
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn options(path: &std::path::Path) -> RunOptions {
    RunOptions {
        state_dir: path.into(),
        config: None,
        address: "127.0.0.1:0".parse().unwrap(),
        cert: None,
        key: None,
    }
}
async fn body(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 300_000)
            .await
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn fresh_state_is_generic_private_and_preserved_on_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let state = State::open(directory.path(), None).unwrap();
    let cfg = Config::read(&state.config).unwrap();
    assert_eq!(cfg.local_type, "server");
    assert!(cfg.enable_local);
    assert!(!cfg.local_node_id().starts_with("proxmox:"));
    let paths = [&state.config, &state.display_token, &state.setup_token];
    let saved = paths
        .iter()
        .map(|p| fs::read(p).unwrap())
        .collect::<Vec<_>>();
    assert_ne!(saved[1], saved[2]);
    assert_eq!(
        fs::metadata(directory.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for path in paths {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(State::open(directory.path(), None).is_err());
    drop(state);
    let reopened = State::open(directory.path(), None).unwrap();
    for (index, path) in [
        &reopened.config,
        &reopened.display_token,
        &reopened.setup_token,
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(fs::read(path).unwrap(), saved[index]);
    }
}

#[test]
fn legacy_configuration_and_existing_tokens_are_not_rewritten() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    private_write(
        &path,
        json!({"enable_proxmox":true,"node":"vm","host_ip":"192.0.2.2"}),
    );
    let saved = fs::read(&path).unwrap();
    let state = State::open(directory.path(), None).unwrap();
    assert_eq!(Config::read(&path).unwrap().local_node_id(), "proxmox:vm");
    assert_eq!(fs::read(&path).unwrap(), saved);
    let token = fs::read(&state.display_token).unwrap();
    drop(state);
    fs::remove_file(&path).unwrap();
    assert!(State::open(directory.path(), None).is_err());
    assert_eq!(
        fs::read(directory.path().join("display.token")).unwrap(),
        token
    );
    assert!(!path.exists());
}

#[test]
fn startup_rejects_unsafe_private_files_without_overwriting_them() {
    let parent = tempfile::tempdir().unwrap();
    let state = State::open(&parent.path().join("state"), None).unwrap();
    let setup = state.setup_token.clone();
    drop(state);
    fs::write(&setup, "short\n").unwrap();
    assert!(State::open(&parent.path().join("state"), None).is_err());
    assert_eq!(fs::read_to_string(&setup).unwrap(), "short\n");
    fs::remove_file(&setup).unwrap();
    let external = parent.path().join("external");
    fs::write(&external, "private untouched").unwrap();
    symlink(&external, &setup).unwrap();
    assert!(State::open(&parent.path().join("state"), None).is_err());
    assert_eq!(fs::read_to_string(&external).unwrap(), "private untouched");
}

fn local_request(peer: &str, host: &str, path: &str) -> Request<Body> {
    let mut request = Request::builder()
        .uri(path)
        .header("Host", host)
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    request
}

#[tokio::test]
async fn integrated_console_auth_requires_real_loopback_origin_and_preserves_roles() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("snapshot.json");
    let now = glimdock_collector::epoch();
    fs::write(
        &path,
        serde_json::to_vec(&json!({"schema":2,"sequence":1,"generated_at":now,"nodes":[]}))
            .unwrap(),
    )
    .unwrap();
    let state = HttpState::new(
        SnapshotStore::new(path, false),
        "d".repeat(43),
        Some("s".repeat(43)),
        directory.path().join("no.sock"),
    )
    .unwrap()
    .with_local_console();
    let app = server::router(state);
    assert_eq!(
        app.clone()
            .oneshot(local_request(
                "127.0.0.1:1234",
                "127.0.0.1:8765",
                "/api/v1/nodes"
            ))
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        app.clone()
            .oneshot(local_request("[::1]:1234", "[::1]:8765", "/api/v1/nodes"))
            .await
            .unwrap()
            .status(),
        200
    );
    for (peer, host) in [
        ("192.0.2.2:1234", "127.0.0.1:8765"),
        ("127.0.0.1:1234", "attacker.test:8765"),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(local_request(peer, host, "/api/v1/nodes"))
                .await
                .unwrap()
                .status(),
            401
        );
    }
    let mut cross = local_request("127.0.0.1:1234", "localhost:8765", "/api/v1/nodes");
    cross
        .headers_mut()
        .insert("Origin", "https://attacker.test".parse().unwrap());
    assert_eq!(app.clone().oneshot(cross).await.unwrap().status(), 401);
    let mut write = local_request("127.0.0.1:1234", "localhost:8765", "/api/v1/config");
    *write.method_mut() = axum::http::Method::POST;
    assert_eq!(app.clone().oneshot(write).await.unwrap().status(), 401);
    let mut wrong_role = local_request("127.0.0.1:1234", "localhost:8765", "/api/v1/config");
    wrong_role.headers_mut().insert(
        "Authorization",
        format!("Bearer {}", "d".repeat(43)).parse().unwrap(),
    );
    assert_eq!(app.clone().oneshot(wrong_role).await.unwrap().status(), 401);
    let mut missing_peer = Request::builder()
        .uri("/api/v1/nodes")
        .header("Host", "localhost:8765")
        .body(Body::empty())
        .unwrap();
    missing_peer
        .headers_mut()
        .insert("X-Forwarded-For", "127.0.0.1".parse().unwrap());
    assert_eq!(
        app.clone().oneshot(missing_peer).await.unwrap().status(),
        401
    );
    let info = body(
        app.oneshot(local_request(
            "192.0.2.2:1234",
            "localhost:8765",
            "/api/v1/web/info",
        ))
        .await
        .unwrap(),
    )
    .await;
    assert_eq!(info["local_bridge"], false);
}

async fn wait_nodes(
    client: &reqwest::Client,
    origin: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let document: Value = client
                .get(format!("{origin}/api/v1/snapshots"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            if predicate(&document) {
                return document;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("node edits did not reach the live collector")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_process_serves_console_live_feed_and_node_management_then_cleans_up() {
    let directory = tempfile::tempdir().unwrap();
    private_write(
        &directory.path().join("config.json"),
        json!({"enable_local":false,"local_type":"server","interval_s":1.0}),
    );
    let mut sample = runtime::empty_snapshot(
        "Windows workstation",
        "192.0.2.8",
        1,
        glimdock_collector::epoch(),
    );
    sample["host"]["cpu_pct"] = json!(37);
    sample["host"]["mem_total_bytes"] = json!(16_000_000_000u64);
    sample["host"]["mem_used_bytes"] = json!(8_000_000_000u64);
    sample["platform"] = json!({"os":"windows"});
    sample["node"] = json!({"id":"server:workstation","type":"server","platform":"windows","name":"Windows workstation","address":"192.0.2.8"});
    sample["sources"] = json!({"host":{"enabled":true,"ok":true,"updated_at":glimdock_collector::epoch(),"age_s":0,"error":null}});
    let fixture = sample.clone();
    let fixture_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fixture_origin = format!("http://{}", fixture_listener.local_addr().unwrap());
    let fixture_job = tokio::spawn(async move {
        axum::serve(
            fixture_listener,
            axum::Router::new().route(
                "/api/v1/snapshot",
                axum::routing::get(move || {
                    let mut fresh = fixture.clone();
                    fresh["generated_at"] = json!(glimdock_collector::epoch());
                    fresh["sources"]["host"]["updated_at"] = json!(glimdock_collector::epoch());
                    async move { axum::Json(fresh) }
                }),
            ),
        )
        .await
        .unwrap();
    });
    let hub = Hub::prepare(options(directory.path())).await.unwrap();
    let (stop, shutdown) = watch::channel(false);
    let (sent, ready) = oneshot::channel();
    let job = tokio::spawn(hub.run(shutdown, Some(sent)));
    let ready = tokio::time::timeout(Duration::from_secs(10), ready)
        .await
        .unwrap()
        .unwrap();
    assert!(ready.config_socket.as_os_str().len() < 104);
    assert_eq!(
        fs::metadata(&ready.config_socket)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let origin = ready.url.trim_end_matches('/');
    let client = reqwest::Client::new();
    let html = client.get(&ready.url).send().await.unwrap();
    assert_eq!(html.status(), 200);
    assert!(html.text().await.unwrap().contains("Glimdock"));
    let info: Value = client
        .get(format!("{origin}/api/v1/web/info"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(info["local_bridge"], true);
    let empty = wait_nodes(&client, origin, |v| {
        v["nodes"].as_array().is_some_and(Vec::is_empty)
    })
    .await;
    assert_eq!(empty["schema"], 2);
    let config: Value = client
        .get(format!("{origin}/api/v1/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let request = json!({"action":"upsert","version":config["version"],"node":{"type":"server","platform":"windows","name":"Workstation","url":fixture_origin,"poll_interval_s":2,"timeout_s":1,"ttl_s":6}});
    let mutation = client
        .post(format!("{origin}/api/v1/config"))
        .header("Origin", origin)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(mutation.status(), 202);
    let changed: Value = mutation.json().await.unwrap();
    let id = changed["config"]["nodes"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let snapshot = wait_nodes(&client, origin, |v| {
        v["nodes"][0]["snapshot"]["host"]["cpu_pct"] == 37
    })
    .await;
    assert_eq!(snapshot["nodes"][0]["platform"], "windows");
    assert_eq!(snapshot["nodes"][0]["type"], "server");
    let stale = client
        .post(format!("{origin}/api/v1/config"))
        .header("Origin", origin)
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), 409);
    let renamed = client.post(format!("{origin}/api/v1/config")).header("Origin", origin)
        .json(&json!({"action":"upsert","version":changed["config"]["version"],"node":{"id":id,"type":"server","name":"Renamed workstation","enabled":false}}))
        .send().await.unwrap();
    assert_eq!(renamed.status(), 202);
    let renamed: Value = renamed.json().await.unwrap();
    assert_eq!(renamed["config"]["nodes"][0]["id"], id);
    assert_eq!(renamed["config"]["nodes"][0]["enabled"], false);
    wait_nodes(&client, origin, |v| {
        v["nodes"].as_array().is_some_and(Vec::is_empty)
    })
    .await;
    let resumed = client.post(format!("{origin}/api/v1/config")).header("Origin", origin)
        .json(&json!({"action":"upsert","version":renamed["config"]["version"],"node":{"id":id,"type":"server","name":"Renamed workstation","enabled":true}}))
        .send().await.unwrap();
    assert_eq!(resumed.status(), 202);
    wait_nodes(&client, origin, |v| {
        v["nodes"][0]["name"] == "Renamed workstation"
            && v["nodes"][0]["snapshot"]["host"]["cpu_pct"] == 37
    })
    .await;
    let forbidden = client
        .post(format!("{origin}/api/v1/config"))
        .header("Origin", "https://attacker.test")
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(forbidden.status(), 401);
    let current: Value = client
        .get(format!("{origin}/api/v1/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let removed = client
        .post(format!("{origin}/api/v1/config"))
        .header("Origin", origin)
        .json(&json!({"action":"delete","version":current["version"],"id":id}))
        .send()
        .await
        .unwrap();
    let removed_status = removed.status();
    let removed_body = removed.text().await.unwrap();
    assert_eq!(
        removed_status, 202,
        "{removed_body}; id={id}; version={}",
        current["version"]
    );
    wait_nodes(&client, origin, |v| {
        v["nodes"].as_array().is_some_and(Vec::is_empty)
    })
    .await;
    let selected: Value = client
        .get(format!("{origin}/api/v1/snapshot"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(selected["nodes"], json!([]));
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(8), job)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!ready.config_socket.exists());
    assert!(!directory.path().join("run.json").exists());
    assert!(State::open(directory.path(), None).is_ok());
    assert!(std::net::TcpListener::bind(ready.address).is_ok());
    fixture_job.abort();
    let _ = fixture_job.await;
}

#[tokio::test]
async fn listener_startup_failure_releases_state_and_private_socket() {
    let directory = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut occupied = options(directory.path());
    occupied.address = listener.local_addr().unwrap();
    assert!(Hub::prepare(occupied).await.is_err());
    assert!(State::open(directory.path(), None).is_ok());
    let mut incomplete_tls = options(directory.path());
    incomplete_tls.cert = Some(directory.path().join("missing.crt"));
    assert!(Hub::prepare(incomplete_tls).await.is_err());
}

#[tokio::test]
async fn command_line_run_handles_sigterm_and_concurrent_instance() {
    let directory = tempfile::tempdir().unwrap();
    private_write(
        &directory.path().join("config.json"),
        json!({"enable_local":false,"interval_s":1.0}),
    );
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_glimdock-collector"));
    command
        .args(["run", "--state-dir"])
        .arg(directory.path())
        .args(["--port", "0"])
        .kill_on_drop(true)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().unwrap();
    let record = directory.path().join("run.json");
    let details: Value = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(raw) = fs::read(&record) {
                if let Ok(value) = serde_json::from_slice(&raw) {
                    return value;
                }
            }
            assert!(
                child.try_wait().unwrap().is_none(),
                "run exited before becoming ready"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let competing = tokio::process::Command::new(env!("CARGO_BIN_EXE_glimdock-collector"))
        .args(["run", "--state-dir"])
        .arg(directory.path())
        .args(["--port", "0"])
        .output()
        .await
        .unwrap();
    assert!(!competing.status.success());
    assert!(String::from_utf8(competing.stderr)
        .unwrap()
        .contains("Another hub is already running"));
    assert_eq!(
        unsafe { libc::kill(child.id().unwrap() as i32, libc::SIGTERM) },
        0
    );
    let status = tokio::time::timeout(Duration::from_secs(8), child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(status.success());
    assert!(!record.exists());
    assert!(!std::path::Path::new(details["config_socket"].as_str().unwrap()).exists());
    assert!(State::open(directory.path(), None).is_ok());
}

#[tokio::test]
async fn busy_configuration_lock_is_bounded_and_preserves_saved_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let state = State::open(directory.path(), None).unwrap();
    let manager = glimdock_collector::config::ConfigManager::new(state.config.clone());
    let configuration = manager.get().unwrap();
    let before = fs::read(&state.config).unwrap();
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.path().join(".config.lock"))
        .unwrap();
    lock.lock_exclusive().unwrap();
    let work = tokio::task::spawn_blocking(move || {
        manager.mutate(&json!({"action":"delete","version":configuration["version"],"id":configuration["local_node"]["id"]}))
    });
    let failure = tokio::time::timeout(Duration::from_secs(4), work)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(failure.status, 503);
    assert_eq!(fs::read(&state.config).unwrap(), before);
}
