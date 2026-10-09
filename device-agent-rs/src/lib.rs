//! Native portable collectors; no interpreter or Linux hub dependency.
pub mod config;
pub mod host;
pub mod json_device;
pub mod protocol;
pub mod server;
pub mod snmp;

use anyhow::Result;
use serde_json::Value;
pub const MAX_PAYLOAD: usize = 48 * 1024;
pub const MAX_CONFIG: usize = 32 * 1024;
pub const HOST_FIELDS: &[&str] = &[
    "uptime_s",
    "cpu_pct",
    "mem_used_bytes",
    "mem_total_bytes",
    "swap_used_bytes",
    "swap_total_bytes",
    "arc_bytes",
    "io_wait_pct",
    "net_rx_bps",
    "net_tx_bps",
    "disk_read_bps",
    "disk_write_bps",
];
pub const POWER_FIELDS: &[&str] = &[
    "package_w",
    "cores_w",
    "graphics_w",
    "cpu_mhz",
    "busy_mhz",
    "cpu_temp_c",
];

pub trait Collector: Send {
    fn descriptor(&self) -> Value;
    fn interval_s(&self) -> f64;
    fn sample(&mut self, now: f64, mono: f64) -> Result<Value>;
}
pub fn epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
