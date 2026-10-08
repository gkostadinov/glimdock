//! Unprivileged HTTP snapshot reader and fixed-scope privileged Unix service.
use crate::{
    config::{self, ConfigManager, MAX_REQUEST, MAX_RESPONSE},
    runtime::{bounded_snapshot, prepare_directory},
    MAX_AGGREGATE, MAX_NODES, MAX_PAYLOAD,
};
use anyhow::{bail, Result};
use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::header,
    response::Response,
    Router,
};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    fs,
    net::SocketAddr,
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{Notify, Semaphore},
};

pub const FRESH_SECONDS: f64 = 15.;
pub fn valid_node_id(identity: &str) -> bool {
    identity.len() <= 63
        && Regex::new(r"^[a-z][a-z0-9_-]{0,31}:[A-Za-z0-9][A-Za-z0-9._-]{0,95}$")
            .unwrap()
            .is_match(identity)
}
pub fn node_status(snapshot: &Value, now: f64) -> &'static str {
    if !snapshot["host"].is_object() {
        return "unknown";
    }
    let Some(stamp) = snapshot["generated_at"].as_f64() else {
        return "unknown";
    };
    if stamp > now + 60. {
        return "unknown";
    }
    if now - stamp > FRESH_SECONDS {
        return "offline";
    }
    let sources = snapshot["sources"]
        .as_object()
        .map(|s| {
            s.values()
                .filter(|s| s.is_object() && s["enabled"] != false)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !sources.is_empty()
        && sources
            .iter()
            .all(|s| s["updated_at"].is_null() && s["error"] == "initializing")
    {
        return "unknown";
    }
    if !sources.is_empty() && sources.iter().all(|s| s["ok"] == false) {
        return "offline";
    }
    if snapshot["alerts"].as_array().is_some_and(|a| !a.is_empty())
        || sources.iter().any(|s| s["ok"] != true)
    {
        return "degraded";
    }
    if sources.is_empty() {
        "unknown"
    } else {
        "healthy"
    }
}
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("snapshot unavailable")]
    Unavailable,
    #[error("no nodes")]
    NoNodes,
    #[error("unknown node")]
    UnknownNode,
    #[error("invalid node selector")]
    InvalidSelector,
}
#[derive(Clone)]
pub struct SnapshotStore {
    pub path: PathBuf,
    pub demo: bool,
    began: f64,
}
impl SnapshotStore {
    pub fn new(path: PathBuf, demo: bool) -> Self {
        Self {
            path,
            demo,
            began: crate::epoch(),
        }
    }
    fn document(&self, now: f64) -> std::result::Result<Value, StoreError> {
        let raw =
            config::read_bounded(&self.path, MAX_AGGREGATE).map_err(|_| StoreError::Unavailable)?;
        let mut document = config::strict_json(&raw).map_err(|_| StoreError::Unavailable)?;
        let stamp = document["generated_at"]
            .as_f64()
            .ok_or(StoreError::Unavailable)?;
        if !self.demo && stamp > now + 60. {
            return Err(StoreError::Unavailable);
        }
        match document["schema"].as_u64() {
            Some(1) => {
                if raw.len() > MAX_PAYLOAD || !document["host"].is_object() {
                    return Err(StoreError::Unavailable);
                }
                let name = document["host"]["name"].as_str().unwrap_or("local");
                let id = config::proxmox_node_id(name);
                document = json!({"schema":2,"generated_at":stamp,"sequence":document["sequence"],"nodes":[{"id":id,"type":"proxmox","name":name,"address":document["host"]["ip"].as_str().unwrap_or(""),"snapshot":document}]});
            }
            Some(2) => {}
            _ => return Err(StoreError::Unavailable),
        }
        let nodes = document["nodes"]
            .as_array()
            .ok_or(StoreError::Unavailable)?;
        if nodes.len() > MAX_NODES {
            return Err(StoreError::Unavailable);
        }
        let mut identities = std::collections::HashSet::new();
        for node in nodes {
            let descriptor = descriptor(node)?;
            if !identities.insert(descriptor["id"].as_str().unwrap().to_string())
                || node["snapshot"]["schema"].as_u64() != Some(1)
                || !node["snapshot"]["host"].is_object()
            {
                return Err(StoreError::Unavailable);
            }
        }
        if self.demo {
            let seq = ((now - self.began) / 3.).floor() as u64 + 1;
            document["generated_at"] = json!(now);
            document["sequence"] = json!(seq);
            for node in document["nodes"].as_array_mut().unwrap() {
                let snapshot = &mut node["snapshot"];
                snapshot["demo"] = json!(true);
                snapshot["sequence"] = json!(seq);
                snapshot["generated_at"] = json!(now);
                if let Some(sources) = snapshot["sources"].as_object_mut() {
                    for source in sources.values_mut() {
                        source["updated_at"] = json!(now);
                        source["age_s"] = json!(0);
                    }
                }
                for (field, stamp, age) in [
                    ("guests", "mem_updated_at", "mem_age_s"),
                    ("gpus", "updated_at", "age_s"),
                ] {
                    if let Some(items) = snapshot[field].as_array_mut() {
                        for item in items {
                            item[stamp] = json!(now);
                            item[age] = json!(0);
                        }
                    }
                }
            }
        }
        Ok(document)
    }
    fn metadata(document: &Value, now: f64) -> std::result::Result<Vec<Value>, StoreError> {
        document["nodes"]
            .as_array()
            .ok_or(StoreError::Unavailable)?
            .iter()
            .map(|node| {
                let mut meta = descriptor(node)?;
                meta["status"] = json!(node_status(&node["snapshot"], now));
                Ok(meta)
            })
            .collect()
    }
    pub fn read(
        &self,
        selector: Option<&str>,
        now: f64,
    ) -> std::result::Result<(Value, f64), StoreError> {
        if selector.is_some_and(|id| !valid_node_id(id)) {
            return Err(StoreError::InvalidSelector);
        }
        let document = self.document(now)?;
        let nodes = document["nodes"].as_array().unwrap();
        if nodes.is_empty() {
            return Err(StoreError::NoNodes);
        }
        let selected = match selector {
            Some(id) => nodes
                .iter()
                .find(|n| n["id"] == id)
                .ok_or(StoreError::UnknownNode)?,
            None => nodes
                .iter()
                .find(|n| n["type"] == "proxmox")
                .unwrap_or(&nodes[0]),
        };
        let mut snapshot = selected["snapshot"].clone();
        let stamp = snapshot["generated_at"]
            .as_f64()
            .ok_or(StoreError::Unavailable)?;
        if !self.demo && stamp > now + 60. {
            return Err(StoreError::Unavailable);
        }
        let metadata = Self::metadata(&document, now)?;
        snapshot["node"] = metadata
            .iter()
            .find(|n| n["id"] == selected["id"])
            .unwrap()
            .clone();
        snapshot["nodes"] = metadata.into();
        snapshot = bounded_snapshot(snapshot);
        if serde_json::to_vec(&snapshot)
            .map_err(|_| StoreError::Unavailable)?
            .len()
            > MAX_PAYLOAD
        {
            return Err(StoreError::Unavailable);
        }
        Ok((snapshot, (now - stamp).max(0.)))
    }
    pub fn nodes(&self, now: f64) -> std::result::Result<Value, StoreError> {
        let document = self.document(now)?;
        Ok(
            json!({"schema":2,"generated_at":document["generated_at"],"sequence":document["sequence"],"nodes":Self::metadata(&document,now)?}),
        )
    }
}
fn descriptor(record: &Value) -> std::result::Result<Value, StoreError> {
    let id = record["id"].as_str().ok_or(StoreError::Unavailable)?;
    let kind = record["type"].as_str().ok_or(StoreError::Unavailable)?;
    let name = record["name"].as_str().ok_or(StoreError::Unavailable)?;
    let address = record["address"].as_str().ok_or(StoreError::Unavailable)?;
    if !valid_node_id(id)
        || !Regex::new(r"^[a-z][a-z0-9_-]{0,31}$")
            .unwrap()
            .is_match(kind)
        || name.is_empty()
        || name.chars().count() > 96
        || address.len() > 255
    {
        return Err(StoreError::Unavailable);
    }
    Ok(json!({"id":id,"type":kind,"name":name,"address":address}))
}

#[derive(Clone)]
pub struct HttpState {
    pub store: SnapshotStore,
    display_token: String,
    setup_token: Option<String>,
    config_socket: PathBuf,
    clients: Arc<Semaphore>,
}
impl HttpState {
    pub fn new(
        store: SnapshotStore,
        display_token: String,
        setup_token: Option<String>,
        config_socket: PathBuf,
    ) -> Result<Self> {
        validate_token(&display_token, false)?;
        if let Some(token) = &setup_token {
            validate_token(token, true)?;
            if token == &display_token {
                bail!("Setup and display tokens must differ");
            }
        }
        Ok(Self {
            store,
            display_token,
            setup_token,
            config_socket,
            clients: Arc::new(Semaphore::new(12)),
        })
    }
}
pub fn validate_token(token: &str, setup: bool) -> Result<()> {
    if token.len() < 32
        || (setup && token.len() > 256)
        || !token.is_ascii()
        || token.chars().any(char::is_whitespace)
    {
        bail!("Monitoring token must be at least 32 ASCII characters without whitespace");
    }
    Ok(())
}
pub fn load_token(path: Option<&Path>, setup: bool) -> Result<Option<String>> {
    let value = if let Some(path) = path {
        String::from_utf8(config::read_bounded(path, 512)?)
            .map_err(|_| anyhow::anyhow!("Invalid token file"))?
    } else {
        std::env::var(if setup {
            "HOMELAB_SETUP_TOKEN"
        } else {
            "HOMELAB_DISPLAY_TOKEN"
        })
        .unwrap_or_default()
    };
    let token = value.trim().to_string();
    if setup && token.is_empty() {
        return Ok(None);
    }
    validate_token(&token, setup)?;
    Ok(Some(token))
}
fn authorized(request: &Request, token: Option<&str>) -> bool {
    let Some(token) = token else {
        return false;
    };
    let mut headers = request.headers().get_all(header::AUTHORIZATION).iter();
    let Some(supplied) = headers.next() else {
        return false;
    };
    if headers.next().is_some() {
        return false;
    }
    let expected = format!("Bearer {token}");
    bool::from(supplied.as_bytes().ct_eq(expected.as_bytes()))
}
fn reply(code: u16, body: Value, head: bool, age: Option<f64>) -> Response {
    let bytes =
        serde_json::to_vec(&body).unwrap_or_else(|_| b"{\"error\":\"unavailable\"}".to_vec());
    let mut response = Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::CONTENT_LENGTH, bytes.len())
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONNECTION, "close")
        .header("X-Content-Type-Options", "nosniff")
        .header("Server", "Glimdock/1");
    if code == 401 {
        response = response.header(header::WWW_AUTHENTICATE, "Bearer realm=\"homelab-display\"");
    }
    if let Some(age) = age {
        response = response.header("X-Snapshot-Age", format!("{age:.1}"));
    }
    response
        .body(if head {
            Body::empty()
        } else {
            Body::from(bytes)
        })
        .unwrap()
}
fn error(code: u16, message: &str, head: bool) -> Response {
    reply(code, json!({"error":message}), head, None)
}
pub fn router(state: HttpState) -> Router {
    Router::new().fallback(handler).with_state(state)
}
async fn handler(State(state): State<HttpState>, request: Request) -> Response {
    let head = request.method() == axum::http::Method::HEAD;
    let Ok(_permit) = state.clients.try_acquire() else {
        return error(503, "busy", head);
    };
    let method = request.method().as_str();
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or("").to_string();
    if path == "/api/v1/config" && query.is_empty() {
        if !["GET", "HEAD", "POST"].contains(&method) {
            return error(404, "not found", head);
        }
        if !authorized(&request, state.setup_token.as_deref()) {
            return error(401, "unauthorized", head);
        }
        let mut body = None;
        if method == "POST" {
            if request.headers().contains_key(header::TRANSFER_ENCODING)
                || request
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.split(';').next().unwrap_or("").trim())
                    != Some("application/json")
            {
                return error(400, "invalid configuration request", head);
            }
            let lengths = request
                .headers()
                .get_all(header::CONTENT_LENGTH)
                .iter()
                .collect::<Vec<_>>();
            if lengths.len() != 1 {
                return error(400, "invalid configuration request", head);
            }
            let Some(length) = lengths[0].to_str().ok().and_then(|s| {
                if s.bytes().all(|b| b.is_ascii_digit()) {
                    s.parse::<usize>().ok()
                } else {
                    None
                }
            }) else {
                return error(400, "invalid configuration request", head);
            };
            if length == 0 || length > MAX_REQUEST - 128 {
                return error(400, "invalid configuration request", head);
            }
            let raw = match tokio::time::timeout(
                Duration::from_secs(5),
                to_bytes(request.into_body(), MAX_REQUEST - 128),
            )
            .await
            {
                Ok(Ok(v)) if v.len() == length => v,
                _ => return error(400, "invalid configuration request", head),
            };
            let value = match config::strict_json(&raw) {
                Ok(v) if v.is_object() => v,
                _ => return error(400, "invalid configuration request", head),
            };
            body = Some(value);
        }
        match forward_config(
            if body.is_some() { "POST" } else { "GET" },
            body,
            &state.config_socket,
        )
        .await
        {
            Ok((status, result)) => reply(status, result, head, None),
            Err(_) => error(503, "configuration service unavailable", head),
        }
    } else if ["GET", "HEAD"].contains(&method) && path == "/healthz" && query.is_empty() {
        let ok = state
            .store
            .read(None, crate::epoch())
            .is_ok_and(|(_, age)| age <= FRESH_SECONDS);
        reply(if ok { 200 } else { 503 }, json!({"ok":ok}), head, None)
    } else if ["GET", "HEAD"].contains(&method)
        && ["/api/v1/snapshot", "/api/v1/nodes"].contains(&path.as_str())
    {
        if path == "/api/v1/nodes" && !query.is_empty() {
            return error(404, "not found", head);
        }
        let pairs = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
        if pairs.iter().any(|(key, _)| key != "node") {
            return error(404, "not found", head);
        }
        if !authorized(&request, Some(&state.display_token)) {
            return error(401, "unauthorized", head);
        }
        let selector = if query.is_empty() {
            None
        } else if pairs.len() == 1 && valid_node_id(&pairs[0].1) {
            Some(pairs[0].1.as_ref())
        } else {
            return error(400, "invalid node selector", head);
        };
        let result = if path == "/api/v1/nodes" {
            state.store.nodes(crate::epoch()).map(|v| (v, None))
        } else {
            state
                .store
                .read(selector, crate::epoch())
                .map(|(v, a)| (v, Some(a)))
        };
        match result {
            Ok((v, a)) => reply(200, v, head, a),
            Err(StoreError::NoNodes) => error(404, "no nodes", head),
            Err(StoreError::UnknownNode) => error(404, "unknown node", head),
            Err(StoreError::InvalidSelector) => error(400, "invalid node selector", head),
            Err(_) => error(503, "snapshot unavailable", head),
        }
    } else {
        error(404, "not found", head)
    }
}
pub async fn serve(
    address: SocketAddr,
    state: HttpState,
    cert: Option<&Path>,
    key: Option<&Path>,
) -> Result<()> {
    if cert.is_some() != key.is_some() {
        bail!("Certificate and key must be supplied together");
    }
    let app = router(state);
    if let (Some(cert), Some(key)) = (cert, key) {
        let config = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
            .await
            .map_err(|_| anyhow::anyhow!("TLS certificate configuration failed"))?;
        axum_server::bind_rustls(address, config)
            .serve(app.into_make_service())
            .await?;
    } else {
        let listener = tokio::net::TcpListener::bind(address).await?;
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await?;
    }
    Ok(())
}
async fn read_line_bounded(stream: &mut UnixStream, bound: usize) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    loop {
        let mut buf = [0u8; 1024];
        let count = stream.read(&mut buf).await?;
        if count == 0 {
            bail!("Incomplete configuration message");
        }
        if let Some(position) = buf[..count].iter().position(|b| *b == b'\n') {
            result.extend_from_slice(&buf[..position]);
            if result.len() + 1 > bound {
                bail!("Configuration message exceeds capacity");
            }
            return Ok(result);
        }
        result.extend_from_slice(&buf[..count]);
        if result.len() >= bound {
            bail!("Configuration message exceeds capacity");
        }
    }
}
pub async fn forward_config(
    method: &str,
    body: Option<Value>,
    path: &Path,
) -> Result<(u16, Value)> {
    let mut request = json!({"method":method});
    if let Some(body) = body {
        request["body"] = body;
    }
    let mut raw = serde_json::to_vec(&request)?;
    raw.push(b'\n');
    if raw.len() > MAX_REQUEST {
        bail!("Configuration request exceeds capacity");
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = UnixStream::connect(path).await?;
        stream.write_all(&raw).await?;
        let raw = read_line_bounded(&mut stream, MAX_RESPONSE).await?;
        let document = config::strict_json(&raw)?;
        let status = document["status"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Invalid config status"))?;
        if ![200, 202, 400, 409, 503].contains(&status) || !document["body"].is_object() {
            bail!("Invalid config response");
        }
        Ok((status as u16, document["body"].clone()))
    })
    .await
    .map_err(|_| anyhow::anyhow!("Configuration service timed out"))?
}
pub async fn config_service(
    config_path: PathBuf,
    socket_path: PathBuf,
    group: Option<&str>,
    apply: bool,
) -> Result<()> {
    prepare_directory(&socket_path, group)?;
    if let Ok(metadata) = fs::symlink_metadata(&socket_path) {
        if !metadata.file_type().is_socket() {
            bail!("Configuration path is not a socket");
        }
        fs::remove_file(&socket_path)?;
    }
    let listener = UnixListener::bind(&socket_path)?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o660))?;
    if let Some(group) = group {
        let group = std::ffi::CString::new(group)?;
        let gid = unsafe {
            let item = libc::getgrnam(group.as_ptr());
            if item.is_null() {
                bail!("Socket group unavailable");
            }
            (*item).gr_gid
        };
        let path = std::ffi::CString::new(socket_path.as_os_str().as_encoded_bytes())?;
        if unsafe { libc::chown(path.as_ptr(), u32::MAX, gid) } != 0 {
            bail!("Socket ownership update failed");
        }
    }
    let manager = Arc::new(ConfigManager::new(config_path));
    let clients = Arc::new(Semaphore::new(8));
    let notify = Arc::new(Notify::new());
    if apply {
        let queue = notify.clone();
        tokio::spawn(async move {
            loop {
                queue.notified().await;
                tokio::time::sleep(Duration::from_millis(250)).await;
                let mut command = tokio::process::Command::new("/usr/bin/systemctl");
                command
                    .args(["restart", config::COLLECTOR_UNIT])
                    .kill_on_drop(true)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                if !matches!(tokio::time::timeout(Duration::from_secs(100),command.status()).await,Ok(Ok(status))if status.success())
                {
                    eprintln!("Collector apply failed; configuration is saved");
                }
            }
        });
    }
    loop {
        let (mut stream, _) = listener.accept().await?;
        let Ok(permit) = clients.clone().try_acquire_owned() else {
            continue;
        };
        let manager = manager.clone();
        let notify = notify.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let response = match tokio::time::timeout(
                Duration::from_secs(5),
                read_line_bounded(&mut stream, MAX_REQUEST),
            )
            .await
            {
                Ok(Ok(raw)) => {
                    let request = config::strict_json(&raw);
                    match request {
                        Ok(request)
                            if request.is_object()
                                && request
                                    .as_object()
                                    .unwrap()
                                    .keys()
                                    .all(|k| ["method", "body"].contains(&k.as_str())) =>
                        {
                            let is_post =
                                request["method"] == "POST" && request.get("body").is_some();
                            let valid = is_post
                                || (request["method"] == "GET" && request.get("body").is_none());
                            if !valid {
                                json!({"status":400,"body":{"error":"Only configuration GET/POST is supported"}})
                            } else {
                                let m = manager.clone();
                                let result = tokio::task::spawn_blocking(move || {
                                    if is_post {
                                        m.mutate(&request["body"])
                                    } else {
                                        m.get()
                                    }
                                })
                                .await;
                                match result {
                                    Ok(Ok(body)) => {
                                        if is_post && apply {
                                            notify.notify_one();
                                        }
                                        json!({"status":if is_post{202}else{200},"body":body})
                                    }
                                    Ok(Err(e)) => {
                                        json!({"status":e.status,"body":{"error":e.message}})
                                    }
                                    _ => {
                                        json!({"status":503,"body":{"error":"Configuration service unavailable"}})
                                    }
                                }
                            }
                        }
                        _ => json!({"status":400,"body":{"error":"Invalid configuration request"}}),
                    }
                }
                _ => {
                    json!({"status":400,"body":{"error":"Configuration request too large or incomplete"}})
                }
            };
            let mut raw = serde_json::to_vec(&response).unwrap();
            raw.push(b'\n');
            if raw.len() <= MAX_RESPONSE {
                let _ = tokio::time::timeout(Duration::from_secs(5), stream.write_all(&raw)).await;
            }
        });
    }
}
