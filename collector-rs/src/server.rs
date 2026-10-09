//! Unprivileged HTTP snapshot reader and fixed-scope privileged Unix service.
use crate::{
    config::{self, ConfigManager, MANAGEMENT_REQUEST, MAX_REQUEST, MAX_RESPONSE},
    runtime::{bounded_snapshot, prepare_directory},
    MAX_AGGREGATE, MAX_NODES, MAX_PAYLOAD,
};
use anyhow::{bail, Result};
use axum::{
    body::{to_bytes, Body},
    extract::{ConnectInfo, Request, State},
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
    sync::{watch, Notify, Semaphore},
    task::JoinSet,
};

mod web_assets {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

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
    if snapshot["agent"].is_object() {
        let stamp = snapshot["agent"]["sampled_at"].as_f64();
        let ttl = crate::finite(&snapshot["agent"]["ttl_s"], 5., 900.);
        if !stamp
            .zip(ttl)
            .is_some_and(|(stamp, ttl)| stamp <= now + 30. && now - stamp <= ttl)
        {
            return "offline";
        }
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
                let node = if document.get("node").is_some() {
                    descriptor(&document["node"])?
                } else {
                    let pve = (document["sources"]["proxmox"].is_object()
                        && document["sources"]["proxmox"]["enabled"] != false)
                        || document["guests"].as_array().is_some_and(|g| !g.is_empty());
                    let kind = if pve { "proxmox" } else { "server" };
                    let platform = if pve {
                        "linux"
                    } else {
                        document["platform"]["os"]
                            .as_str()
                            .filter(|p| config::valid_platform(p) && !p.is_empty())
                            .unwrap_or(if document["sources"]["proc"].is_object() {
                                "linux"
                            } else {
                                config::native_platform()
                            })
                    };
                    json!({"id":config::native_node_id(kind,name),"type":kind,"platform":platform,"name":name,"address":document["host"]["ip"].as_str().unwrap_or("")})
                };
                let mut record = node;
                record["snapshot"] = document;
                document = json!({"schema":2,"generated_at":stamp,"sequence":record["snapshot"]["sequence"],"nodes":[record]});
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
                meta["summary"] = node_summary(&node["snapshot"], now);
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
            if selector.is_some() {
                return Err(StoreError::UnknownNode);
            }
            let stamp = document["generated_at"]
                .as_f64()
                .ok_or(StoreError::Unavailable)?;
            let mut empty = crate::runtime::empty_snapshot(
                "No nodes configured",
                "",
                document["sequence"].as_u64().unwrap_or(0),
                stamp,
            );
            empty["nodes"] = json!([]);
            empty["sources"] = json!({"collector":{"enabled":false,"ok":false,"updated_at":stamp,"age_s":(now-stamp).max(0.),"error":"No nodes configured"}});
            empty["alerts"] = json!([{"id":"setup/no-nodes","severity":"warning","message":"No nodes configured"}]);
            return Ok((empty, (now - stamp).max(0.)));
        }
        let selected = match selector {
            Some(id) => nodes
                .iter()
                .find(|n| n["id"] == id)
                .ok_or(StoreError::UnknownNode)?,
            None => &nodes[0],
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
    /// All bounded node snapshots for the web emulator. This has the same
    /// display-token role as a selected snapshot and never contains configuration.
    pub fn snapshots(&self, now: f64) -> std::result::Result<Value, StoreError> {
        let mut document = self.document(now)?;
        let metadata = Self::metadata(&document, now)?;
        for (record, meta) in document["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(metadata)
        {
            record["status"] = meta["status"].clone();
            record["summary"] = meta["summary"].clone();
            record["snapshot"] = bounded_snapshot(record["snapshot"].take());
        }
        if serde_json::to_vec(&document)
            .map_err(|_| StoreError::Unavailable)?
            .len()
            > MAX_AGGREGATE
        {
            return Err(StoreError::Unavailable);
        }
        Ok(document)
    }
    pub fn fresh(&self, now: f64) -> bool {
        self.document(now).is_ok_and(|d| {
            d["generated_at"]
                .as_f64()
                .is_some_and(|stamp| now - stamp <= FRESH_SECONDS)
        })
    }
    pub fn nodes(&self, now: f64) -> std::result::Result<Value, StoreError> {
        let document = self.document(now)?;
        Ok(
            json!({"schema":2,"generated_at":document["generated_at"],"sequence":document["sequence"],"nodes":Self::metadata(&document,now)?}),
        )
    }
}
/// Small, finite summaries share one policy between the physical display,
/// node registry, and browser. Expired/offline samples never show old metrics.
pub fn node_summary(snapshot: &Value, now: f64) -> Value {
    let status = node_status(snapshot, now);
    let stamp = snapshot["agent"]["sampled_at"]
        .as_f64()
        .or_else(|| snapshot["feed"]["updated_at"].as_f64())
        .or_else(|| snapshot["printer"]["updated_at"].as_f64())
        .or_else(|| snapshot["generated_at"].as_f64());
    let ttl = crate::finite(&snapshot["agent"]["ttl_s"], 5., 900.)
        .or_else(|| crate::finite(&snapshot["feed"]["ttl_s"], 5., 900.))
        .or_else(|| crate::finite(&snapshot["printer"]["ttl_s"], 5., 900.))
        .unwrap_or(FRESH_SECONDS);
    let age = stamp.map(|t| (now - t).max(0.));
    let live = matches!(status, "healthy" | "degraded") && age.is_some_and(|a| a <= ttl);
    let cpu = live
        .then(|| crate::finite(&snapshot["host"]["cpu_pct"], 0., 100.))
        .flatten();
    let memory = if live {
        crate::number(&snapshot["host"]["mem_used_bytes"])
            .zip(crate::number(&snapshot["host"]["mem_total_bytes"]).filter(|t| *t > 0.))
            .and_then(|(used, total)| (used >= 0. && used <= total).then_some(used * 100. / total))
    } else {
        None
    };
    let temperature = if live {
        crate::finite(&snapshot["power"]["cpu_temp_c"], -50., 500.)
            .or_else(|| {
                snapshot["printer"]["heaters"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find_map(|h| crate::finite(&h["temp_c"], -50., 500.))
            })
            .or_else(|| {
                snapshot["sensors"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|s| s["kind"] == "temperature")
                    .filter_map(|s| crate::finite(&s["value"], -50., 500.))
                    .reduce(f64::max)
            })
    } else {
        None
    };
    let count = |field: &str| {
        if !live {
            return None;
        }
        snapshot["limits"]["counts"][field]
            .as_u64()
            .or_else(|| snapshot[field].as_array().map(|a| a.len() as u64))
    };
    let print_state = if live {
        snapshot["printer"]["state"].as_str().filter(|s| {
            [
                "standby",
                "printing",
                "paused",
                "complete",
                "cancelled",
                "error",
                "unknown",
            ]
            .contains(s)
        })
    } else {
        None
    };
    json!({"status":status,"cpu_percent":cpu,"memory_percent":memory,
        "temperature_c":temperature,"progress_percent":if live {crate::finite(&snapshot["printer"]["progress_pct"],0.,100.)}else{None},
        "print_state":print_state,"sensors":count("sensors"),"guests":count("guests"),
        "generated_at":stamp,"age_s":age,"ttl_s":ttl})
}
fn descriptor(record: &Value) -> std::result::Result<Value, StoreError> {
    let id = record["id"].as_str().ok_or(StoreError::Unavailable)?;
    let kind = record["type"].as_str().ok_or(StoreError::Unavailable)?;
    let name = record["name"].as_str().ok_or(StoreError::Unavailable)?;
    let address = record["address"].as_str().ok_or(StoreError::Unavailable)?;
    let platform = record
        .get("platform")
        .map(|p| p.as_str().ok_or(StoreError::Unavailable))
        .transpose()?
        .unwrap_or("");
    if !valid_node_id(id)
        || !Regex::new(r"^[a-z][a-z0-9_-]{0,31}$")
            .unwrap()
            .is_match(kind)
        || name.is_empty()
        || name.chars().count() > 96
        || address.len() > 255
        || !config::valid_platform(platform)
    {
        return Err(StoreError::Unavailable);
    }
    let mut own = json!({"id":id,"type":kind,"platform":platform,"name":name,"address":address});
    if record["origin"] == "agent" {
        own["origin"] = json!("agent");
        for key in ["last_seen", "sample_at"] {
            if record[key].is_null() || record[key].as_f64().is_some_and(|n| n.is_finite()) {
                own[key] = record[key].clone();
            }
        }
    }
    Ok(own)
}

#[derive(Clone)]
pub struct HttpState {
    pub store: SnapshotStore,
    display_token: String,
    setup_token: Option<String>,
    config_socket: PathBuf,
    clients: Arc<Semaphore>,
    upstream: Option<(String, reqwest::Client)>,
    bridge_https: bool,
    local_console: bool,
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
            upstream: None,
            bridge_https: false,
            local_console: false,
        })
    }
    /// A single-server hub trusts only genuine loopback clients using a local
    /// Host and same-origin browser writes. LAN requests still use bearer roles.
    pub fn with_local_console(mut self) -> Self {
        self.local_console = true;
        self
    }
    /// A loopback-only companion serves the identical console and injects
    /// private credentials only into the fixed, configured collector origin.
    pub fn with_upstream(mut self, origin: &str) -> Result<Self> {
        let parsed = url::Url::parse(origin)?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !matches!(parsed.path(), "" | "/")
            || parsed.port() == Some(0)
        {
            bail!("Upstream must be a fixed HTTP(S) origin without credentials");
        }
        self.upstream = Some((
            parsed.origin().ascii_serialization(),
            crate::runtime::http_client(5.)?,
        ));
        Ok(self)
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
fn asset_reply(bytes: &'static [u8], content_type: &'static str, head: bool) -> Response {
    Response::builder().status(200)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, bytes.len())
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
        .header("Permissions-Policy", "serial=(self)")
        .header("Content-Security-Policy", "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; worker-src 'self' blob:; object-src 'none'; base-uri 'none'; frame-ancestors 'self'")
        .body(if head { Body::empty() } else { Body::from(bytes) }).unwrap()
}
fn local_bridge_request(request: &Request, write: bool, https: bool) -> bool {
    let Some(authority) = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
    else {
        return false;
    };
    let Ok(url) = url::Url::parse(&format!(
        "{}://{authority}",
        if https { "https" } else { "http" }
    )) else {
        return false;
    };
    if !matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
    {
        return false;
    }
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok());
    if write && origin.is_none() {
        return false;
    }
    if let Some(origin) = origin {
        if origin != url.origin().ascii_serialization() {
            return false;
        }
    }
    if request
        .headers()
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
    {
        return false;
    }
    true
}
async fn proxy_api(
    state: &HttpState,
    request: Request,
    path: &str,
    query: &str,
    head: bool,
) -> Response {
    let method = request.method().as_str();
    let config = path == "/api/v1/config";
    if !(matches!(method, "GET" | "HEAD") || (config && method == "POST"))
        || (config && !query.is_empty())
    {
        return error(404, "not found", head);
    }
    let token = if config {
        state.setup_token.as_deref()
    } else {
        Some(state.display_token.as_str())
    };
    let Some(token) = token else {
        return error(401, "management key unavailable", head);
    };
    let (origin, client) = state.upstream.as_ref().unwrap();
    let mut endpoint = format!("{origin}{path}");
    if !query.is_empty() {
        endpoint.push('?');
        endpoint.push_str(query);
    }
    // Incoming Authorization, cookies and arbitrary headers never cross the bridge.
    let mut outgoing = client
        .request(
            if head {
                reqwest::Method::GET
            } else {
                request.method().clone()
            },
            endpoint,
        )
        .bearer_auth(token);
    if method == "POST" {
        if request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or("").trim())
            != Some("application/json")
            || request.headers().contains_key(header::TRANSFER_ENCODING)
        {
            return error(400, "invalid configuration request", head);
        }
        let raw = match tokio::time::timeout(
            Duration::from_secs(5),
            to_bytes(request.into_body(), MANAGEMENT_REQUEST - 128),
        )
        .await
        {
            Ok(Ok(v)) if !v.is_empty() => v,
            _ => return error(400, "invalid configuration request", head),
        };
        if config::strict_json(&raw).is_err() {
            return error(400, "invalid configuration request", head);
        }
        outgoing = outgoing
            .header(header::CONTENT_TYPE, "application/json")
            .body(raw.to_vec());
    }
    let Ok(mut result) = outgoing.send().await else {
        return error(503, "collector unavailable", head);
    };
    let status = result.status().as_u16();
    let limit = if config { MAX_RESPONSE } else { MAX_AGGREGATE };
    let mut bytes = Vec::new();
    loop {
        match result.chunk().await {
            Ok(Some(chunk)) if bytes.len() + chunk.len() <= limit => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(None) => break,
            _ => return error(503, "collector response unavailable", head),
        }
    }
    let Ok(value) = config::strict_json(&bytes) else {
        return error(503, "collector response unavailable", head);
    };
    reply(status, value, head, None)
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
    let local_console = state.local_console
        && request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|ConnectInfo(peer)| peer.ip().is_loopback())
        && local_bridge_request(&request, method == "POST", state.bridge_https);
    if state.upstream.is_some()
        && !local_bridge_request(&request, method == "POST", state.bridge_https)
    {
        return error(
            403,
            "local bridge requires a same-origin loopback request",
            head,
        );
    }
    if ["GET", "HEAD"].contains(&method) && path == "/api/v1/web/info" && query.is_empty() {
        return reply(
            200,
            json!({"schema":1,"local_bridge":state.upstream.is_some() || local_console,"management_available":state.setup_token.is_some(),"collector_url":state.upstream.as_ref().map(|(origin,_)|origin)}),
            head,
            None,
        );
    }
    if ["GET", "HEAD"].contains(&method) {
        if let Some((bytes, mime)) = web_assets::asset(&path) {
            return asset_reply(bytes, mime, head);
        }
    }
    if state.upstream.is_some()
        && [
            "/api/v1/config",
            "/api/v1/snapshot",
            "/api/v1/snapshots",
            "/api/v1/nodes",
            "/healthz",
        ]
        .contains(&path.as_str())
    {
        return proxy_api(&state, request, &path, &query, head).await;
    }
    if ["/api/v1/agents/enroll", "/api/v1/agents/push"].contains(&path.as_str()) && query.is_empty()
    {
        if method != "POST" || state.upstream.is_some() || state.store.demo {
            return error(404, "not found", head);
        }
        // Publishing always requires a scoped credential, including on loopback.
        let mut auth = request.headers().get_all(header::AUTHORIZATION).iter();
        let Some(token) = auth
            .next()
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        else {
            return error(401, "agent credential required", head);
        };
        if auth.next().is_some() || token.len() != 64 || !crate::push::valid_identity(token) {
            return error(401, "invalid agent credential", head);
        }
        let token = token.to_string();
        if request.headers().contains_key(header::TRANSFER_ENCODING)
            || request
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.split(';').next().unwrap_or("").trim())
                != Some("application/json")
        {
            return error(400, "invalid agent request", head);
        }
        let mut lengths = request.headers().get_all(header::CONTENT_LENGTH).iter();
        let length = lengths
            .next()
            .and_then(|v| v.to_str().ok())
            .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<usize>().ok());
        let Some(length) = length.filter(|n| *n > 0 && *n <= MAX_REQUEST - 512) else {
            return error(413, "agent request exceeds capacity", head);
        };
        if lengths.next().is_some() {
            return error(400, "invalid agent request", head);
        }
        let raw = match tokio::time::timeout(
            Duration::from_secs(5),
            to_bytes(request.into_body(), MAX_REQUEST - 512),
        )
        .await
        {
            Ok(Ok(v)) if v.len() == length => v,
            _ => return error(400, "invalid agent request", head),
        };
        let body = match config::strict_json(&raw) {
            Ok(v) if v.is_object() => v,
            _ => return error(400, "invalid agent request", head),
        };
        if path.ends_with("/enroll")
            && (body["agent_token"].as_str() == Some(state.display_token.as_str())
                || body["agent_token"]
                    .as_str()
                    .is_some_and(|t| Some(t) == state.setup_token.as_deref()))
        {
            return error(
                400,
                "publisher credential must be separate from console credentials",
                head,
            );
        }
        match forward_config(
            if path.ends_with("/enroll") {
                "AGENT_ENROLL"
            } else {
                "AGENT_PUSH"
            },
            Some(json!({"token":token,"request":body})),
            &state.config_socket,
        )
        .await
        {
            Ok((code, body)) => reply(code, body, head, None),
            Err(_) => error(503, "agent service unavailable", head),
        }
    } else if path == "/api/v1/config" && query.is_empty() {
        if !["GET", "HEAD", "POST"].contains(&method) {
            return error(404, "not found", head);
        }
        if !(authorized(&request, state.setup_token.as_deref())
            || (local_console && !request.headers().contains_key(header::AUTHORIZATION)))
        {
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
            if length == 0 || length > MANAGEMENT_REQUEST - 128 {
                return error(400, "invalid configuration request", head);
            }
            let raw = match tokio::time::timeout(
                Duration::from_secs(5),
                to_bytes(request.into_body(), MANAGEMENT_REQUEST - 128),
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
        let ok = state.store.fresh(crate::epoch());
        reply(if ok { 200 } else { 503 }, json!({"ok":ok}), head, None)
    } else if ["GET", "HEAD"].contains(&method)
        && ["/api/v1/snapshot", "/api/v1/nodes", "/api/v1/snapshots"].contains(&path.as_str())
    {
        if path != "/api/v1/snapshot" && !query.is_empty() {
            return error(404, "not found", head);
        }
        let pairs = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
        if pairs.iter().any(|(key, _)| key != "node") {
            return error(404, "not found", head);
        }
        if !(authorized(&request, Some(&state.display_token))
            || (local_console && !request.headers().contains_key(header::AUTHORIZATION)))
        {
            return error(401, "unauthorized", head);
        }
        let selector = if query.is_empty() {
            None
        } else if pairs.len() == 1 && valid_node_id(&pairs[0].1) {
            Some(pairs[0].1.as_ref())
        } else {
            return error(400, "invalid node selector", head);
        };
        let result = if path == "/api/v1/snapshots" {
            state.store.snapshots(crate::epoch()).map(|v| (v, None))
        } else if path == "/api/v1/nodes" {
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
/// Bound and TLS-validated before any integrated worker is started.
pub struct HttpServer {
    listener: std::net::TcpListener,
    state: HttpState,
    tls: Option<axum_server::tls_rustls::RustlsConfig>,
}
impl HttpServer {
    pub async fn bind(
        address: SocketAddr,
        mut state: HttpState,
        cert: Option<&Path>,
        key: Option<&Path>,
    ) -> Result<Self> {
        if cert.is_some() != key.is_some() {
            bail!("Certificate and key must be supplied together");
        }
        if state.upstream.is_some() && !address.ip().is_loopback() {
            bail!("Credential bridge requires loopback bind");
        }
        state.bridge_https = cert.is_some();
        let tls = if let (Some(cert), Some(key)) = (cert, key) {
            Some(
                axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key)
                    .await
                    .map_err(|_| anyhow::anyhow!("TLS certificate configuration failed"))?,
            )
        } else {
            None
        };
        let listener = std::net::TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            state,
            tls,
        })
    }
    pub fn local_addr(&self) -> Result<SocketAddr> {
        Ok(self.listener.local_addr()?)
    }
    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let handle = axum_server::Handle::new();
        let app = router(self.state).into_make_service_with_connect_info::<SocketAddr>();
        let graceful = async {
            crate::runtime::wait_for_shutdown(&mut shutdown).await;
            handle.graceful_shutdown(Some(Duration::from_secs(5)));
        };
        let serve = async {
            if let Some(tls) = self.tls {
                axum_server::from_tcp_rustls(self.listener, tls)?
                    .handle(handle.clone())
                    .serve(app)
                    .await?;
            } else {
                axum_server::from_tcp(self.listener)?
                    .handle(handle.clone())
                    .serve(app)
                    .await?;
            }
            Ok::<_, anyhow::Error>(())
        };
        tokio::pin!(serve, graceful);
        tokio::select! {
            result = &mut serve => result,
            _ = &mut graceful => serve.await,
        }
    }
}
pub async fn serve(
    address: SocketAddr,
    state: HttpState,
    cert: Option<&Path>,
    key: Option<&Path>,
) -> Result<()> {
    let server = HttpServer::bind(address, state, cert, key).await?;
    let (stop, shutdown) = watch::channel(false);
    let work = server.run(shutdown);
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => result,
        _ = crate::runtime::shutdown_signal() => {
            let _ = stop.send(true);
            work.await
        }
    }
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
        if ![200, 202, 400, 401, 403, 409, 413, 503].contains(&status)
            || !document["body"].is_object()
        {
            bail!("Invalid config response");
        }
        Ok((status as u16, document["body"].clone()))
    })
    .await
    .map_err(|_| anyhow::anyhow!("Configuration service timed out"))?
}
/// Private configuration listener prepared before the HTTP console is ready.
pub struct ConfigurationService {
    listener: UnixListener,
    config_path: PathBuf,
    apply: bool,
}
impl ConfigurationService {
    pub fn bind(
        config_path: PathBuf,
        socket_path: &Path,
        group: Option<&str>,
        apply: bool,
    ) -> Result<Self> {
        prepare_directory(socket_path, group)?;
        if let Ok(metadata) = fs::symlink_metadata(socket_path) {
            if !metadata.file_type().is_socket() {
                bail!("Configuration path is not a socket");
            }
            fs::remove_file(socket_path)?;
        }
        let listener = UnixListener::bind(socket_path)?;
        fs::set_permissions(socket_path, fs::Permissions::from_mode(0o660))?;
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
        if group.is_none() {
            fs::set_permissions(socket_path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(Self {
            listener,
            config_path,
            apply,
        })
    }
    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let manager = Arc::new(ConfigManager::new(self.config_path));
        let clients = Arc::new(Semaphore::new(8));
        let notify = Arc::new(Notify::new());
        let apply = self.apply;
        let listener = self.listener;
        let mut jobs = JoinSet::new();
        if apply {
            let queue = notify.clone();
            jobs.spawn(async move {
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
            let (mut stream, _) = tokio::select! {
                _ = crate::runtime::wait_for_shutdown(&mut shutdown) => {
                    jobs.abort_all();
                    while jobs.join_next().await.is_some() {}
                    return Ok(());
                },
                _ = jobs.join_next(), if !jobs.is_empty() => continue,
                accepted = listener.accept() => accepted?,
            };
            let Ok(permit) = clients.clone().try_acquire_owned() else {
                continue;
            };
            let manager = manager.clone();
            let notify = notify.clone();
            jobs.spawn(async move {
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
                            let is_agent = matches!(request["method"].as_str(), Some("AGENT_ENROLL"|"AGENT_PUSH")) && request["body"].is_object();
                            let is_post =
                                request["method"] == "POST" && request.get("body").is_some();
                            let valid = is_post || is_agent
                                || (request["method"] == "GET" && request.get("body").is_none());
                            if !valid {
                                json!({"status":400,"body":{"error":"Only configuration GET/POST is supported"}})
                            } else {
                                let m = manager.clone();
                                let result = tokio::task::spawn_blocking(move || {
                                    if is_agent {
                                        let body=&request["body"];
                                        if body.as_object().is_none_or(|o|o.keys().any(|k|!["token","request"].contains(&k.as_str()))) { return Err(config::ConfigError::new(400,"Invalid agent request")); }
                                        let token=body["token"].as_str().unwrap_or("");
                                        if request["method"]=="AGENT_ENROLL" {crate::push::enroll(&m,token,&body["request"],crate::epoch())} else {crate::push::ingest(&m,token,&body["request"],crate::epoch())}
                                    } else if is_post {
                                        m.mutate(&request["body"])
                                    } else {
                                        m.get()
                                    }
                                })
                                .await;
                                match result {
                                    Ok(Ok(body)) => {
                                        if (is_post || (is_agent && body["duplicate"] != true && body.get("accepted").is_none())) && apply {
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
}
struct SocketCleanup(PathBuf);
impl Drop for SocketCleanup {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.0).is_ok_and(|meta| meta.file_type().is_socket()) {
            let _ = fs::remove_file(&self.0);
        }
    }
}
pub async fn config_service(
    config_path: PathBuf,
    socket_path: PathBuf,
    group: Option<&str>,
    apply: bool,
) -> Result<()> {
    let service = ConfigurationService::bind(config_path, &socket_path, group, apply)?;
    let _cleanup = SocketCleanup(socket_path);
    let (stop, shutdown) = watch::channel(false);
    let work = service.run(shutdown);
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => result,
        _ = crate::runtime::shutdown_signal() => {
            let _ = stop.send(true);
            work.await
        }
    }
}
