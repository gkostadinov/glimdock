//! Fixed read-only upstream host/device feeds. No redirects or transparent proxy.
use crate::{
    config::{valid_platform, NodeConfig},
    number,
    runtime::{empty_snapshot, SourceStatus},
    MAX_PAYLOAD,
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
const FIELDS: &[&str] = &[
    "schema",
    "sequence",
    "generated_at",
    "host",
    "platform",
    "power",
    "guests",
    "storage",
    "disks",
    "sensors",
    "gpus",
    "alerts",
    "sources",
    "faults",
    "limits",
];
pub struct RemoteCollectorReader {
    pub config: NodeConfig,
    client: reqwest::Client,
    last_sequence: Option<u64>,
    sequence_at: Option<f64>,
}
impl RemoteCollectorReader {
    pub fn new(config: NodeConfig) -> Result<Self> {
        Ok(Self {
            client: crate::runtime::http_client(config.timeout_s)?,
            config,
            last_sequence: None,
            sequence_at: None,
        })
    }
    pub async fn read(&mut self) -> Result<Value> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/snapshot", self.config.url))
            .header("Accept", "application/json");
        if !self.config.token_file.is_empty() {
            req = req.bearer_auth(crate::runtime::read_secret(&self.config.token_file, 32)?);
        }
        let document = crate::runtime::bounded_http_json(req, MAX_PAYLOAD).await?;
        self.validate(document, crate::epoch())
    }
    pub fn validate(&mut self, document: Value, now: f64) -> Result<Value> {
        let stamp = document["generated_at"]
            .as_f64()
            .ok_or_else(|| anyhow::anyhow!("Missing remote timestamp"))?;
        let sequence = document["sequence"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing remote sequence"))?;
        if document["schema"].as_u64() != Some(1)
            || document["demo"] == true
            || document.get("printer").is_some()
            || !document["host"].is_object()
        {
            bail!("Expected live schema1 host/device snapshot");
        }
        if stamp > now + 60. || now - stamp > self.config.ttl_s {
            bail!("Remote snapshot expired or future dated");
        }
        if self.last_sequence != Some(sequence) {
            self.last_sequence = Some(sequence);
            self.sequence_at = Some(now);
        } else if now - self.sequence_at.unwrap_or(now) > self.config.ttl_s {
            bail!("Remote snapshot sequence has stopped");
        }
        for field in ["guests", "storage", "disks", "sensors", "gpus", "alerts"] {
            if let Some(value) = document.get(field) {
                if !value
                    .as_array()
                    .is_some_and(|items| items.iter().all(Value::is_object))
                {
                    bail!("Invalid remote inventory");
                }
            }
        }
        if !document["sources"]
            .as_object()
            .is_some_and(|s| s.values().all(Value::is_object))
        {
            bail!("Invalid remote sources");
        }
        for field in ["power", "platform", "limits", "faults"] {
            if document.get(field).is_some_and(|v| !v.is_object()) {
                bail!("Invalid remote object");
            }
        }
        let platform = match document.get("node") {
            Some(node) => {
                if !node.is_object() || node["type"] != self.config.node_type {
                    bail!("Remote endpoint has a different node type");
                }
                match node.get("platform") {
                    None => "",
                    Some(value) => value
                        .as_str()
                        .filter(|p| valid_platform(p))
                        .ok_or_else(|| anyhow::anyhow!("Invalid remote platform"))?,
                }
            }
            None if self.config.node_type == "proxmox" => "",
            None => bail!("Remote host/device descriptor is required"),
        }
        .to_string();
        if !self.config.platform.is_empty()
            && !platform.is_empty()
            && platform != self.config.platform
        {
            bail!("Remote platform does not match configuration");
        }
        if self.config.node_type == "server" && document["host"].get("name").is_none() {
            bail!("Remote host name is required");
        }
        let mut own = json!({});
        for field in FIELDS {
            if let Some(value) = document.get(*field) {
                own[*field] = value.clone();
            }
        }
        for field in ["power", "limits"] {
            if own.get(field).is_none() {
                own[field] = json!({});
            }
        }
        for field in ["guests", "storage", "disks", "sensors", "gpus", "alerts"] {
            if own.get(field).is_none() {
                own[field] = json!([]);
            }
        }
        Ok(json!({"snapshot":own,"generated_at":stamp,"node_platform":platform,"error":null}))
    }
}
pub fn remote_snapshot(
    config: &NodeConfig,
    state: &SourceStatus,
    data: Option<&Value>,
    sequence: u64,
    now: f64,
) -> Value {
    let stamp = data
        .and_then(|d| number(&d["generated_at"]))
        .or(state.updated_at);
    let age = stamp.map(|t| (now - t).max(0.));
    let mut own = serde_json::to_value(state).unwrap();
    own["updated_at"] = json!(stamp);
    own["age_s"] = json!(age);
    let mut snapshot = if let Some(data) =
        data.filter(|_| state.ok && age.is_some_and(|a| a <= config.ttl_s))
    {
        let mut sample = data["snapshot"].clone();
        if let Some(sources) = sample["sources"].as_object_mut() {
            for source in sources.values_mut() {
                if let Some(t) = number(&source["updated_at"]) {
                    source["age_s"] = json!((now - t).max(0.));
                }
            }
        }
        for (field, age_field, stamp_field) in [
            ("guests", "mem_age_s", "mem_updated_at"),
            ("gpus", "age_s", "updated_at"),
            ("sensors", "age_s", "updated_at"),
        ] {
            if let Some(items) = sample[field].as_array_mut() {
                for item in items {
                    if let Some(t) = number(&item[stamp_field]) {
                        item[age_field] = json!((now - t).max(0.));
                    }
                }
            }
        }
        sample
    } else {
        let host = url::Url::parse(&config.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_default();
        let mut sample = empty_snapshot(&config.name, &host, sequence, now);
        own["ok"] = json!(false);
        own["error"] = json!(state
            .error
            .as_deref()
            .unwrap_or("Remote snapshot expired/unavailable"));
        if state.error.as_deref() != Some("initializing") {
            sample["alerts"] = json!([{"id":format!("remote/{}/unavailable",config.id),"severity":"warning","message":if config.node_type=="proxmox"{"Remote Proxmox telemetry unavailable"}else{"Remote host/device telemetry unavailable"}}]);
        }
        sample
    };
    snapshot["sequence"] = json!(sequence);
    snapshot["generated_at"] = json!(now);
    snapshot["sources"]["remote_feed"] = own.clone();
    snapshot["feed"] =
        json!({"updated_at":stamp,"age_s":age,"ttl_s":config.ttl_s,"error":own["error"]});
    snapshot
}
