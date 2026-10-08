//! Reads the existing restricted NAS helper; no remote shell or control action.
use crate::{
    config::Config,
    guests::memory,
    probes::{command, monotonic, num, text, wall},
    procfs::Rates,
};
use serde_json::{json, Value};
#[derive(Default)]
pub struct TrueNasReader {
    rates: Rates,
    boot: Option<String>,
}
impl TrueNasReader {
    pub fn parse(&mut self, d: &Value, now: f64, mono: f64) -> anyhow::Result<Value> {
        if d["schema"] != 1 {
            anyhow::bail!("Unsupported NAS helper schema")
        };
        let timestamp =
            num(&d["generated_at"]).ok_or_else(|| anyhow::anyhow!("NAS timestamp absent"))?;
        if !(-60.0..=90.0).contains(&(now - timestamp)) {
            anyhow::bail!("NAS helper sample timestamp invalid/stale")
        };
        let boot = d["boot_id"].as_str().map(str::to_owned);
        if self.boot.is_some() && boot != self.boot {
            self.rates = Rates::default()
        }
        self.boot = boot;
        let disks=d["disks"].as_array().into_iter().flatten().take(16).filter_map(|item|{let name=item["name"].as_str()?;Some(json!({"name":text(&format!("NAS/{name}"),64),"model":text(item["model"].as_str().unwrap_or(""),64),"temp_c":num(&item["temp_c"]).filter(|v|(-50.0..=150.0).contains(v)),"health":null,"status":"unknown","zfs_status":text(item["zfs_status"].as_str().unwrap_or("unknown"),24),"temperature_source":"truenas-cache","read_bps":self.rates.rate(&format!("{name}/read"),num(&item["read_bytes"]),mono),"write_bps":self.rates.rate(&format!("{name}/write"),num(&item["write_bytes"]),mono),"read_errors":num(&item["read_errors"]),"write_errors":num(&item["write_errors"]),"checksum_errors":num(&item["checksum_errors"]),"size_bytes":num(&item["size_bytes"])}))}).collect::<Vec<_>>();
        let storage=d["pools"].as_array().into_iter().flatten().take(16).filter_map(|p|{let name=p["name"].as_str()?;Some(json!({"id":text(&format!("nas/pool/{name}"),96),"name":text(&format!("NAS · {name}"),64),"used_bytes":num(&p["used_bytes"]),"total_bytes":num(&p["total_bytes"]),"status":text(p["status"].as_str().unwrap_or("unknown"),24),"healthy":p["healthy"].as_bool(),"scrub_state":text(p["scrub_state"].as_str().unwrap_or(""),24),"scrub_errors":num(&p["scrub_errors"]),"read_errors":num(&p["read_errors"]),"write_errors":num(&p["write_errors"]),"checksum_errors":num(&p["checksum_errors"])}))}).collect::<Vec<_>>();
        let alerts=d["alerts"].as_array().into_iter().flatten().take(24).filter(|a|a["severity"]=="warning"||a["severity"]=="critical").map(|a|json!({"id":text(&format!("nas/{}",a["id"].as_str().unwrap_or("alert")),96),"severity":a["severity"],"message":text(&format!("NAS: {}",a["message"].as_str().unwrap_or("")),120)})).collect::<Vec<_>>();
        let mut memory_error = d["memory_error"].as_str().map(|s| text(s, 120));
        let mem = if d["memory"].is_object() {
            match memory(
                num(&d["memory"]["mem_total_bytes"]),
                num(&d["memory"]["mem_available_bytes"]),
                num(&d["memory"]["mem_cache_bytes"]),
            ) {
                Ok(mut m) => {
                    m["memory_basis"] = json!("guest-os-arc");
                    Some(m)
                }
                Err(_) => {
                    memory_error = Some("NAS OS memory sample invalid".into());
                    None
                }
            }
        } else {
            None
        };
        Ok(
            json!({"disks":disks,"storage":storage,"alerts":alerts,"memory":mem,"memory_error":memory_error,"generated_at":timestamp,"temperature_cache_interval_s":num(&d["temperature_cache_interval_s"]),"error":d["error"].as_str().map(|s|text(s,180)),"counts":d["counts"]}),
        )
    }
    pub fn read(&mut self, c: &Config) -> anyhow::Result<Value> {
        let known = format!("UserKnownHostsFile={}", c.truenas_known_hosts);
        let r = command(
            "ssh",
            &[
                "-T",
                "-i",
                &c.truenas_ssh_key,
                "-o",
                "BatchMode=yes",
                "-o",
                "PasswordAuthentication=no",
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                &known,
                "-o",
                "ConnectTimeout=5",
                "-o",
                "LogLevel=ERROR",
                "-l",
                &c.truenas_ssh_user,
                &c.truenas_ssh_host,
                "snapshot",
            ],
            15.,
            false,
        )?;
        if r.stdout.len() > 60 * 1024 {
            anyhow::bail!("NAS helper payload exceeds limit")
        };
        self.parse(&serde_json::from_str(&r.stdout)?, wall(), monotonic())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pools_not_smart_and_rate_boot() {
        let mut r = TrueNasReader::default();
        let mut d = json!({"schema":1,"generated_at":100,"boot_id":"a","disks":[{"name":"sda","zfs_status":"ONLINE","read_bytes":100,"write_bytes":10}],"pools":[],"memory":{"mem_total_bytes":1000,"mem_available_bytes":400,"mem_cache_bytes":100}});
        let a = r.parse(&d, 100., 1.).unwrap();
        assert!(a["disks"][0]["health"].is_null());
        assert!(a["disks"][0]["read_bps"].is_null());
        d["disks"][0]["read_bytes"] = json!(200);
        assert_eq!(r.parse(&d, 100., 3.).unwrap()["disks"][0]["read_bps"], 50.);
        d["boot_id"] = json!("b");
        assert!(r.parse(&d, 100., 4.).unwrap()["disks"][0]["read_bps"].is_null());
        assert!(r.parse(&d, 300., 5.).is_err())
    }
}
