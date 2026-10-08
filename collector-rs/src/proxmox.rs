//! Fixed, read-only Proxmox resource queries.
use crate::{
    config::Config,
    probes::{command, monotonic, num, round, text},
    procfs::{hostname, Rates},
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[derive(Default)]
pub struct ProxmoxReader {
    rates: Rates,
    uptime: BTreeMap<String, f64>,
}
impl ProxmoxReader {
    pub fn parse_resources(
        &mut self,
        c: &Config,
        resources: &Value,
        stores: &Value,
        status: Option<&Value>,
        now: f64,
    ) -> anyhow::Result<Value> {
        let resources = resources
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Proxmox resources must be array"))?;
        let node = if c.node.is_empty() {
            hostname()
        } else {
            c.node.clone()
        };
        let mut guests = vec![];
        let mut storage = vec![];
        for item in resources {
            if item["node"] != node {
                continue;
            };
            let kind = item["type"].as_str().unwrap_or("");
            if kind == "qemu" || kind == "lxc" {
                let Some(id) = item["vmid"].as_u64() else {
                    continue;
                };
                let identity = format!("{kind}/{id}");
                let up = num(&item["uptime"]);
                if let Some(up) = up {
                    if self.uptime.get(&identity).is_some_and(|old| up < *old) {
                        self.rates.clear_prefix(&format!("{identity}/"))
                    }
                    self.uptime.insert(identity.clone(), up);
                }
                let status = item["status"].as_str().unwrap_or("unknown");
                let mut guest = json!({"id":id,"name":text(item["name"].as_str().unwrap_or(&identity),64),"type":if kind=="qemu"{"vm"}else{"lxc"},"status":text(status,16),"cpu_pct":num(&item["cpu"]).map(|v|round(v*100.,2)),"mem_used_bytes":num(&item["mem"]),"mem_total_bytes":num(&item["maxmem"]),"mem_host_bytes":num(&item["memhost"]),"mem_assigned_bytes":num(&item["maxmem"]),"uptime_s":up});
                for (src, dst) in [
                    ("netin", "net_rx_bps"),
                    ("netout", "net_tx_bps"),
                    ("diskread", "disk_read_bps"),
                    ("diskwrite", "disk_write_bps"),
                ] {
                    let rate = self
                        .rates
                        .rate(&format!("{identity}/{src}"), num(&item[src]), now);
                    guest[dst] = json!(if status == "running" { rate } else { None })
                }
                guests.push(guest)
            } else if kind == "storage" {
                storage.push(json!({"id":text(item["id"].as_str().unwrap_or(""),96),"name":text(item["storage"].as_str().or(item["id"].as_str()).unwrap_or(""),64),"used_bytes":num(&item["disk"]),"total_bytes":num(&item["maxdisk"]),"status":text(item["status"].as_str().unwrap_or("unknown"),16)}))
            }
        }
        if let Some(stores) = stores.as_array() {
            storage=stores.iter().map(|item|{let name=item["storage"].as_str().unwrap_or("unknown");json!({"id":format!("storage/{node}/{name}"),"name":text(name,64),"used_bytes":num(&item["used"]),"total_bytes":num(&item["total"]),"status":if num(&item["enabled"])==Some(0.){"disabled"}else if num(&item["active"])==Some(1.){"online"}else if num(&item["active"])==Some(0.){"offline"}else{"unknown"}})}).collect()
        }
        guests.sort_by_key(|g| g["id"].as_u64());
        storage.sort_by_key(|s| s["name"].as_str().unwrap_or("").to_owned());
        Ok(
            json!({"guests":guests,"storage":storage,"io_wait_pct":status.and_then(|s|num(&s["wait"])).map(|v|round(v*100.,2)),"node_error":if status.is_none(){Some("Proxmox node status unavailable")}else{None}}),
        )
    }
    pub fn read(&mut self, c: &Config) -> anyhow::Result<Value> {
        let node = if c.node.is_empty() {
            hostname()
        } else {
            c.node.clone()
        };
        let res: Value = serde_json::from_str(
            &command(
                "pvesh",
                &["get", "/cluster/resources", "--output-format", "json"],
                5.,
                false,
            )?
            .stdout,
        )?;
        let stores: Value = serde_json::from_str(
            &command(
                "pvesh",
                &[
                    "get",
                    &format!("/nodes/{node}/storage"),
                    "--output-format",
                    "json",
                ],
                5.,
                false,
            )?
            .stdout,
        )?;
        let status = command(
            "pvesh",
            &[
                "get",
                &format!("/nodes/{node}/status"),
                "--output-format",
                "json",
            ],
            5.,
            false,
        )
        .ok()
        .and_then(|r| serde_json::from_str::<Value>(&r.stdout).ok());
        self.parse_resources(c, &res, &stores, status.as_ref(), monotonic())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn per_guest_rate_restart() {
        let mut r = ProxmoxReader::default();
        let c = Config {
            node: "node".into(),
            ..Config::default()
        };
        let mut data = json!([{"type":"qemu","node":"node","vmid":100,"status":"running","cpu":0.2,"mem":10,"maxmem":100,"uptime":10,"netin":100}]);
        let a = r
            .parse_resources(&c, &data, &json!([]), Some(&json!({"wait":0.1})), 1.)
            .unwrap();
        assert!(a["guests"][0]["net_rx_bps"].is_null());
        data[0]["netin"] = json!(200);
        data[0]["uptime"] = json!(11);
        let b = r
            .parse_resources(&c, &data, &json!([]), Some(&json!({})), 3.)
            .unwrap();
        assert_eq!(b["guests"][0]["net_rx_bps"], 50.);
        data[0]["uptime"] = json!(1);
        let b = r.parse_resources(&c, &data, &json!([]), None, 4.).unwrap();
        assert!(b["guests"][0]["net_rx_bps"].is_null());
        assert_eq!(b["guests"][0]["cpu_pct"], 20.)
    }
}
