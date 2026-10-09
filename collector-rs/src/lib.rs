//! Glimdock's native Rust monitoring runtime. No Python collection engine.
pub mod config;
pub mod faults;
pub mod gpus;
pub mod guests;
pub mod hub;
pub mod printers;
pub mod probes;
pub mod procfs;
pub mod proxmox;
pub mod remote_feeds;
pub mod runtime;
pub mod sensors;
pub mod server;
pub mod storage;
pub mod truenas;

pub const MAX_PAYLOAD: usize = 48 * 1024;
pub const MAX_AGGREGATE: usize = 256 * 1024;
pub const MAX_NODES: usize = 4;

pub fn epoch() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
pub fn compact_text(value: &str, size: usize) -> String {
    value
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .take(size)
        .collect()
}
pub fn number(value: &serde_json::Value) -> Option<f64> {
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))?;
    number.is_finite().then_some(number)
}
pub fn finite(value: &serde_json::Value, low: f64, high: f64) -> Option<f64> {
    number(value).filter(|n| *n >= low && *n <= high)
}
