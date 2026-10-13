//! Portable read-only agent operations. No agent execution or source mutation.
use anyhow::{bail, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::Path,
    sync::LazyLock,
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;

static SCHEMA: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../contracts/agent-operations/snapshot-v1.schema.json"
    ))
    .expect("authored operations schema")
});

// This evaluator supports the exact authored v1 vocabulary. It never resolves
// external schemas or executes upstream content. The contract remains the source.
fn schema_value(value: &Value, schema: &Value, depth: usize) -> bool {
    if depth > 32 {
        return false;
    }
    if let Some(reference) = schema["$ref"].as_str() {
        return reference
            .strip_prefix('#')
            .and_then(|p| SCHEMA.pointer(p))
            .is_some_and(|s| schema_value(value, s, depth + 1));
    }
    if let Some(choices) = schema["anyOf"].as_array() {
        return choices.iter().any(|s| schema_value(value, s, depth + 1));
    }
    if let Some(constant) = schema.get("const") {
        if value != constant {
            return false;
        }
    }
    if let Some(choices) = schema["enum"].as_array() {
        if !choices.contains(value) {
            return false;
        }
    }
    if let Some(kind) = schema["type"].as_str() {
        let valid = match kind {
            "null" => value.is_null(),
            "boolean" => value.is_boolean(),
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_u64().is_some() || value.as_i64().is_some(),
            "number" => value.as_f64().is_some_and(f64::is_finite),
            _ => false,
        };
        if !valid {
            return false;
        }
    }
    if let Some(object) = value.as_object() {
        if schema["required"].as_array().is_some_and(|keys| {
            keys.iter()
                .any(|key| key.as_str().is_none_or(|k| !object.contains_key(k)))
        }) {
            return false;
        }
        for (key, item) in object {
            match schema["properties"].get(key) {
                Some(s) => {
                    if !schema_value(item, s, depth + 1) {
                        return false;
                    }
                }
                None => {
                    if schema["additionalProperties"] == false {
                        return false;
                    }
                }
            }
        }
    }
    if let Some(array) = value.as_array() {
        if schema["maxItems"]
            .as_u64()
            .is_some_and(|max| array.len() as u64 > max)
        {
            return false;
        }
        if schema["uniqueItems"] == true
            && array
                .iter()
                .enumerate()
                .any(|(i, v)| array[..i].contains(v))
        {
            return false;
        }
        if let Some(items) = schema.get("items") {
            if array
                .iter()
                .any(|item| !schema_value(item, items, depth + 1))
            {
                return false;
            }
        }
    }
    if let Some(text) = value.as_str() {
        if schema["maxLength"]
            .as_u64()
            .is_some_and(|max| text.chars().count() as u64 > max)
        {
            return false;
        }
        if let Some(pattern) = schema["pattern"].as_str() {
            if !Regex::new(pattern).is_ok_and(|r| r.is_match(text)) {
                return false;
            }
        }
    }
    if let Some(number) = value.as_f64() {
        if !number.is_finite()
            || schema["minimum"].as_f64().is_some_and(|min| number < min)
            || schema["maximum"].as_f64().is_some_and(|max| number > max)
        {
            return false;
        }
    }
    true
}

pub fn validate(document: &Value) -> Result<()> {
    if serde_json::to_vec(document)?.len() > crate::MAX_PAYLOAD
        || !schema_value(document, &SCHEMA, 0)
    {
        bail!("Invalid or oversized operations snapshot");
    }
    let sources = document["sources"].as_array().unwrap();
    let systems = document["systems"].as_array().unwrap();
    let source_ids = sources
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    let system_ids = systems
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect::<HashSet<_>>();
    if source_ids.len() != sources.len() || system_ids.len() != systems.len() {
        bail!("Duplicate operations identity");
    }
    for field in [
        "systems",
        "agents",
        "tasks",
        "events",
        "resources",
        "channels",
    ] {
        let rows = document[field].as_array().unwrap();
        let mut seen = HashSet::new();
        for row in rows {
            let identity = (
                row["system_id"]
                    .as_str()
                    .unwrap_or(row["id"].as_str().unwrap()),
                row["id"].as_str().unwrap(),
            );
            if !seen.insert(identity)
                || !source_ids.contains(row["source_id"].as_str().unwrap())
                || (field != "systems" && !system_ids.contains(row["system_id"].as_str().unwrap()))
            {
                bail!("Invalid scoped operations reference");
            }
        }
        let count = document["limits"]["counts"][field].as_u64().unwrap();
        if count < rows.len() as u64
            || (count > rows.len() as u64 && document["limits"]["truncated"][field] != true)
        {
            bail!("Invalid operations truncation");
        }
    }
    for row in systems.iter().chain(document["agents"].as_array().unwrap()) {
        let usage = &row["usage"];
        if !usage["cost"].is_null()
            && (usage["currency"].is_null()
                || usage["provenance"]
                    .as_str()
                    .is_none_or(|v| v.trim().is_empty()))
        {
            bail!("Cost requires currency and provenance");
        }
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OperationsConfig {
    pub enabled: bool,
    pub url: String,
    pub snapshot_file: String,
    pub token_file: String,
    pub allow_insecure_http: bool,
    pub poll_interval_s: f64,
    pub timeout_s: f64,
}
impl Default for OperationsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            url: String::new(),
            snapshot_file: String::new(),
            token_file: String::new(),
            allow_insecure_http: false,
            poll_interval_s: 5.,
            timeout_s: 3.,
        }
    }
}
impl OperationsConfig {
    pub fn validate(&self) -> Result<()> {
        if !(1.0..=300.).contains(&self.poll_interval_s) || !(1.0..=10.).contains(&self.timeout_s) {
            bail!("Invalid operations timing");
        }
        if !self.enabled {
            return Ok(());
        }
        if self.url.is_empty() == self.snapshot_file.is_empty() {
            bail!("Configure one operations URL or snapshot file");
        }
        if !self.snapshot_file.is_empty() {
            if !Path::new(&self.snapshot_file).is_absolute() || !self.token_file.is_empty() {
                bail!("Operations file must be absolute without a network key");
            }
            return Ok(());
        }
        let url = url::Url::parse(&self.url)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.port() == Some(0)
        {
            bail!("Invalid operations source URL");
        }
        let local = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if url.scheme() == "http" && !local && !self.allow_insecure_http {
            bail!("Remote operations HTTP needs explicit trusted-network opt-in");
        }
        if url.query_pairs().any(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "token" | "key" | "password" | "secret"
            )
        }) {
            bail!("Operations credentials belong in a private header key file");
        }
        Ok(())
    }
    async fn read(self) -> Result<Value> {
        let document = if !self.snapshot_file.is_empty() {
            let bytes =
                crate::config::read_bounded(Path::new(&self.snapshot_file), crate::MAX_PAYLOAD)?;
            crate::config::strict_json(&bytes)?
        } else {
            let client = crate::runtime::http_client(self.timeout_s)?;
            let mut request = client.get(&self.url);
            if !self.token_file.is_empty() {
                request = request.bearer_auth(crate::runtime::read_secret(&self.token_file, 24)?);
            }
            crate::runtime::bounded_http_json(request, crate::MAX_PAYLOAD).await?
        };
        validate(&document)?;
        Ok(document)
    }
}

pub struct OperationsSource {
    config: OperationsConfig,
    handle: Option<JoinHandle<Result<Value>>>,
    next: Instant,
    data: Option<Value>,
}
impl OperationsSource {
    pub fn new(config: OperationsConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            handle: None,
            next: Instant::now(),
            data: None,
        })
    }
    pub async fn advance(&mut self) -> Option<&Value> {
        if self.handle.as_ref().is_some_and(JoinHandle::is_finished) {
            self.data = self.handle.take().unwrap().await.ok().and_then(Result::ok);
        }
        if self.handle.is_none() && Instant::now() >= self.next {
            let config = self.config.clone();
            self.handle = Some(tokio::spawn(async move { config.read().await }));
            self.next = Instant::now() + Duration::from_secs_f64(self.config.poll_interval_s);
        }
        self.data.as_ref()
    }
}
impl Drop for OperationsSource {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

pub fn snapshot_path(hardware_snapshot: &Path) -> std::path::PathBuf {
    hardware_snapshot.with_file_name("agent-operations.json")
}
pub fn read_snapshot(hardware_snapshot: &Path) -> Result<Value> {
    let bytes = crate::config::read_bounded(&snapshot_path(hardware_snapshot), crate::MAX_PAYLOAD)?;
    let doc = crate::config::strict_json(&bytes)?;
    validate(&doc)?;
    Ok(doc)
}
