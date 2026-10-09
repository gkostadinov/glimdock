//! Authenticated read-only native endpoint; requests never trigger collection.
use crate::{config, protocol::snapshot_status, MAX_PAYLOAD};
use anyhow::{bail, Result};
use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, Method},
    response::Response,
    Router,
};
use serde_json::{json, Value};
use std::{
    path::Path,
    sync::{Arc, RwLock},
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

/// Bound accepted connections before HTTP parsing or a TLS handshake begins.
/// Clones share the same twelve permits, retained until the connection closes.
#[derive(Clone)]
pub struct BoundedAcceptor<A> {
    inner: A,
    connections: Arc<Semaphore>,
    accept_timeout: std::time::Duration,
}

impl<A> BoundedAcceptor<A> {
    pub fn new(inner: A) -> Self {
        Self {
            inner,
            connections: Arc::new(Semaphore::new(12)),
            accept_timeout: std::time::Duration::from_secs(5),
        }
    }
}

impl<A, I, S> axum_server::accept::Accept<I, S> for BoundedAcceptor<A>
where
    A: axum_server::accept::Accept<I, S>,
    A::Future: Send + 'static,
    A::Stream: Send + 'static,
    A::Service: Send + 'static,
{
    type Stream = ConnectionStream<A::Stream>;
    type Service = A::Service;
    type Future = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::io::Result<(Self::Stream, Self::Service)>> + Send,
        >,
    >;

    fn accept(&self, stream: I, service: S) -> Self::Future {
        // A denied connection is closed immediately instead of queued for a permit.
        let permit = match self.connections.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                return Box::pin(std::future::ready(Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "Connection capacity reached",
                ))));
            }
        };
        let accepting = self.inner.accept(stream, service);
        let timeout = self.accept_timeout;
        Box::pin(async move {
            let (stream, service) =
                tokio::time::timeout(timeout, accepting)
                    .await
                    .map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "Connection acceptance timed out",
                        )
                    })??;
            Ok((
                ConnectionStream {
                    inner: stream,
                    _permit: permit,
                },
                service,
            ))
        })
    }
}

pub struct ConnectionStream<I> {
    inner: I,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl<I: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for ConnectionStream<I> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_read(cx, buffer)
    }
}

impl<I: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for ConnectionStream<I> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, buffer)
    }

    fn poll_write_vectored(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffers: &[std::io::IoSlice<'_>],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, buffers)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[derive(Clone)]
pub struct ServerState {
    pub snapshot: Arc<RwLock<Value>>,
    token: String,
    interval_s: f64,
    clients: Arc<Semaphore>,
}
impl ServerState {
    pub fn new(snapshot: Value, token: String, interval_s: f64) -> Result<Self> {
        if !(32..=256).contains(&token.len()) || !token.bytes().all(|c| (33..=126).contains(&c)) {
            bail!("Invalid display token");
        }
        if !interval_s.is_finite() || interval_s < 1. || interval_s > 300. {
            bail!("Invalid publication interval");
        }
        if serde_json::to_vec(&snapshot)?.len() > MAX_PAYLOAD
            || snapshot["schema"] != 1
            || snapshot["node"]["type"] != "server"
        {
            bail!("Invalid initial snapshot");
        }
        Ok(Self {
            snapshot: Arc::new(RwLock::new(snapshot)),
            token,
            interval_s,
            clients: Arc::new(Semaphore::new(12)),
        })
    }
    pub fn publish(&self, snapshot: Value) -> Result<()> {
        if serde_json::to_vec(&snapshot)?.len() > MAX_PAYLOAD {
            bail!("Snapshot exceeds capacity");
        }
        *self
            .snapshot
            .write()
            .map_err(|_| anyhow::anyhow!("Snapshot lock unavailable"))? = snapshot;
        Ok(())
    }
}
pub fn load_token(path: &Path) -> Result<String> {
    config::read_token(path, 32)
}
pub fn fresh(snapshot: &Value, now: f64, interval: f64) -> bool {
    crate::protocol::number(&snapshot["generated_at"])
        .is_some_and(|stamp| stamp <= now && now - stamp <= 15f64.max(2. * interval))
        && snapshot_status(snapshot) != "offline"
}
fn authorized(request: &Request, token: &str) -> bool {
    let mut values = request.headers().get_all(header::AUTHORIZATION).iter();
    let Some(supplied) = values.next() else {
        return false;
    };
    values.next().is_none()
        && bool::from(
            supplied
                .as_bytes()
                .ct_eq(format!("Bearer {token}").as_bytes()),
        )
}
fn reply(code: u16, value: Value, head: bool, age: Option<f64>) -> Response {
    let raw = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
    let mut response = Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .header(header::CONTENT_LENGTH, raw.len())
        .header(header::CACHE_CONTROL, "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header(header::CONNECTION, "close");
    if code == 401 {
        response = response.header(header::WWW_AUTHENTICATE, "Bearer realm=\"homelab-display\"");
    }
    if let Some(age) = age {
        response = response.header("X-Snapshot-Age", format!("{age:.1}"));
    }
    response
        .body(if head { Body::empty() } else { Body::from(raw) })
        .unwrap()
}
async fn request(State(state): State<ServerState>, request: Request) -> Response {
    let head = request.method() == Method::HEAD;
    let Ok(_permit) = state.clients.try_acquire() else {
        return reply(503, json!({"error":"busy"}), head, None);
    };
    let path = request.uri().path();
    let query = request.uri().query();
    if !matches!(path, "/api/v1/snapshot" | "/api/v1/nodes" | "/healthz") {
        return reply(404, json!({"error":"not found"}), head, None);
    }
    if request.method() != Method::GET && !head {
        return reply(405, json!({"error":"read only"}), head, None);
    }
    if path == "/healthz" && query.is_some() {
        return reply(404, json!({"error":"not found"}), head, None);
    }
    if path != "/healthz" && !authorized(&request, &state.token) {
        return reply(401, json!({"error":"unauthorized"}), head, None);
    }
    let snapshot = match state.snapshot.read() {
        Ok(s) => s.clone(),
        Err(_) => return reply(503, json!({"error":"snapshot unavailable"}), head, None),
    };
    let now = crate::epoch();
    if path == "/healthz" {
        let healthy = fresh(&snapshot, now, state.interval_s);
        return reply(
            if healthy { 200 } else { 503 },
            json!({"ok":healthy}),
            head,
            None,
        );
    }
    if let Some(query) = query {
        let pairs = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
        if path != "/api/v1/snapshot" || pairs.is_empty() || pairs.iter().any(|(k, _)| k != "node")
        {
            return reply(404, json!({"error":"not found"}), head, None);
        }
        if pairs.len() != 1
            || pairs[0].1.len() > 63
            || !regex::Regex::new(r"^server:[A-Za-z0-9][A-Za-z0-9_.-]{0,55}$")
                .unwrap()
                .is_match(&pairs[0].1)
        {
            return reply(400, json!({"error":"invalid node selector"}), head, None);
        }
        if snapshot["node"]["id"].as_str() != Some(pairs[0].1.as_ref()) {
            return reply(404, json!({"error":"unknown node"}), head, None);
        }
    }
    if !fresh(&snapshot, now, state.interval_s) {
        return reply(503, json!({"error":"snapshot unavailable"}), head, None);
    }
    let age = crate::protocol::number(&snapshot["generated_at"]).map(|t| (now - t).max(0.));
    let response = if path == "/api/v1/nodes" {
        json!({"schema":2,"sequence":snapshot["sequence"],"generated_at":snapshot["generated_at"],"nodes":snapshot["nodes"]})
    } else {
        snapshot
    };
    reply(200, response, head, age)
}
pub fn router(state: ServerState) -> Router {
    Router::new().fallback(request).with_state(state)
}

#[cfg(test)]
mod accept_tests {
    use super::*;
    use axum_server::accept::{Accept, DefaultAcceptor};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn permits_follow_connection_lifetime_and_stream_preserves_io() {
        let acceptor = BoundedAcceptor::new(DefaultAcceptor::new());
        let mut streams = Vec::new();
        for _ in 0..12 {
            let (stream, _peer) = tokio::io::duplex(16);
            let (stream, ()) = acceptor.accept(stream, ()).await.unwrap();
            streams.push(stream);
        }
        let (stream, _peer) = tokio::io::duplex(16);
        let result = acceptor.clone().accept(stream, ()).await;
        assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
        assert_eq!(acceptor.connections.available_permits(), 0);
        streams.pop();
        assert_eq!(acceptor.connections.available_permits(), 1);
        let (stream, mut peer) = tokio::io::duplex(16);
        let (mut stream, ()) = acceptor.accept(stream, ()).await.unwrap();
        peer.write_all(b"ping").await.unwrap();
        let mut bytes = [0_u8; 4];
        stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"ping");
        stream.write_all(b"pong").await.unwrap();
        peer.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"pong");
        drop(stream);
        drop(streams);
        assert_eq!(acceptor.connections.available_permits(), 12);
    }

    #[derive(Clone)]
    struct StalledAccept;
    impl Accept<tokio::io::DuplexStream, ()> for StalledAccept {
        type Stream = tokio::io::DuplexStream;
        type Service = ();
        type Future = std::future::Pending<std::io::Result<(Self::Stream, Self::Service)>>;
        fn accept(&self, _stream: Self::Stream, _service: ()) -> Self::Future {
            std::future::pending()
        }
    }

    #[tokio::test]
    async fn stalled_acceptance_times_out_and_releases_the_permit() {
        let mut acceptor = BoundedAcceptor::new(StalledAccept);
        acceptor.accept_timeout = std::time::Duration::from_millis(10);
        let (stream, _peer) = tokio::io::duplex(16);
        let accepting = acceptor.accept(stream, ());
        assert_eq!(acceptor.connections.available_permits(), 11);
        let result = accepting.await;
        assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::TimedOut));
        assert_eq!(acceptor.connections.available_permits(), 12);
    }
}
