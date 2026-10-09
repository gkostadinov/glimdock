//! Explicit read-only vendor API mappings, without scripts or expressions.
use crate::{
    config,
    protocol::{empty_snapshot, finish_snapshot, number, source_status, text},
    Collector, HOST_FIELDS, MAX_PAYLOAD, POWER_FIELDS,
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, io::Read, path::PathBuf, time::Duration};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PathPart {
    Key(String),
    Index(usize),
}
fn validate_path(path: &[PathPart]) -> Result<()> {
    if path.is_empty()
        || path.len() > 16
        || path.iter().any(|p| match p {
            PathPart::Key(k) => k.is_empty() || k.len() > 96,
            PathPart::Index(i) => *i > 4096,
        })
    {
        bail!("Invalid bounded JSON path");
    }
    Ok(())
}
fn at_path<'a>(doc: &'a Value, path: &[PathPart]) -> Option<&'a Value> {
    path.iter().try_fold(doc, |value, part| match part {
        PathPart::Key(key) => value.as_object()?.get(key),
        PathPart::Index(index) => value.as_array()?.get(*index),
    })
}
fn one() -> f64 {
    1.
}
fn number_parser() -> String {
    "number".into()
}
fn other() -> String {
    "other".into()
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MetricMapping {
    pub field: String,
    pub path: Vec<PathPart>,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "number_parser")]
    pub parse: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SensorMapping {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub path: Vec<PathPart>,
    #[serde(default = "other")]
    pub kind: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "number_parser")]
    pub parse: String,
    #[serde(default)]
    pub high: Option<f64>,
    #[serde(default)]
    pub crit: Option<f64>,
}
fn validate_conversion(path: &[PathPart], scale: f64, offset: f64, parse: &str) -> Result<()> {
    validate_path(path)?;
    if !scale.is_finite() || !offset.is_finite() || !matches!(parse, "number" | "numeric-string") {
        bail!("Invalid numeric conversion");
    }
    Ok(())
}
fn mapped(doc: &Value, path: &[PathPart], scale: f64, offset: f64, parser: &str) -> Option<f64> {
    let raw = at_path(doc, path)?;
    let n = number(raw).or_else(|| {
        if parser != "numeric-string" {
            return None;
        }
        let string = raw.as_str()?.trim();
        if !regex::Regex::new(r"^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$")
            .unwrap()
            .is_match(string)
        {
            return None;
        }
        string.parse::<f64>().ok().filter(|n| n.is_finite())
    })?;
    let n = n * scale + offset;
    n.is_finite().then_some(n)
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct JsonDeviceConfig {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub address: String,
    pub url: String,
    pub token_file: PathBuf,
    pub interval_s: f64,
    pub timeout_s: f64,
    pub ttl_s: f64,
    pub timestamp_path: Option<Vec<PathPart>>,
    pub sequence_path: Option<Vec<PathPart>>,
    pub host: Vec<MetricMapping>,
    pub power: Vec<MetricMapping>,
    pub sensors: Vec<SensorMapping>,
}
impl Default for JsonDeviceConfig {
    fn default() -> Self {
        Self {
            id: "device".into(),
            name: "Device".into(),
            platform: "other".into(),
            address: String::new(),
            url: String::new(),
            token_file: PathBuf::new(),
            interval_s: 3.,
            timeout_s: 2.5,
            ttl_s: 15.,
            timestamp_path: None,
            sequence_path: None,
            host: vec![],
            power: vec![],
            sensors: vec![],
        }
    }
}
impl JsonDeviceConfig {
    pub fn validate(&mut self) -> Result<()> {
        config::validate_identity(&self.id)?;
        config::validate_platform(&self.platform)?;
        config::validate_text(&self.name, 64)?;
        config::validate_text(&self.address, 255)?;
        if self.name.trim().is_empty()
            || self.url.len() > 1024
            || self.url.chars().any(char::is_whitespace)
        {
            bail!("Invalid device metadata or URL");
        }
        let url = url::Url::parse(&self.url).map_err(|_| anyhow::anyhow!("Invalid device URL"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.port() == Some(0)
        {
            bail!("Device URL must be HTTP(S) without credentials");
        }
        if self.address.is_empty() {
            self.address = url.host_str().unwrap().into();
        }
        if !self.token_file.as_os_str().is_empty() && !self.token_file.is_absolute() {
            bail!("Credential file must be absolute");
        }
        for (value, low, high) in [
            (self.interval_s, 2., 300.),
            (self.timeout_s, 0.5, 3.),
            (self.ttl_s, 5., 900.),
        ] {
            if !value.is_finite() || value < low || value > high {
                bail!("Invalid adapter interval");
            }
        }
        if self.ttl_s < self.interval_s * 2. {
            bail!("TTL must permit two samples");
        }
        for path in [&self.timestamp_path, &self.sequence_path]
            .into_iter()
            .flatten()
        {
            validate_path(path)?;
        }
        if self.host.len() > 16 || self.power.len() > 8 || self.sensors.len() > 64 {
            bail!("Telemetry mapping capacity exceeded");
        }
        let mut ids = HashSet::new();
        for (group, items, allowed) in [
            ("host", &self.host, HOST_FIELDS),
            ("power", &self.power, POWER_FIELDS),
        ] {
            for m in items {
                validate_conversion(&m.path, m.scale, m.offset, &m.parse)?;
                if !allowed.contains(&m.field.as_str())
                    || !ids.insert(format!("{group}/{}", m.field))
                {
                    bail!("Invalid or duplicate metric mapping");
                }
            }
        }
        for m in &mut self.sensors {
            config::validate_identity(&m.id)?;
            validate_conversion(&m.path, m.scale, m.offset, &m.parse)?;
            if m.name.is_empty() {
                m.name = m.id.clone();
            }
            config::validate_text(&m.name, 64)?;
            config::validate_text(&m.kind, 32)?;
            config::validate_text(&m.unit, 16)?;
            if [m.high, m.crit]
                .into_iter()
                .flatten()
                .any(|v| !v.is_finite())
                || m.high.zip(m.crit).is_some_and(|(a, b)| a >= b)
                || !ids.insert(format!("sensor/{}", m.id))
            {
                bail!("Invalid sensor mapping");
            }
        }
        if ids.is_empty() {
            bail!("At least one telemetry mapping is required");
        }
        Ok(())
    }
}
pub struct JsonDeviceCollector {
    pub config: JsonDeviceConfig,
    client: reqwest::blocking::Client,
    sequence: u64,
    last_sequence: Option<u64>,
    sequence_at: f64,
    descriptor: Value,
}
impl JsonDeviceCollector {
    pub fn new(mut config: JsonDeviceConfig) -> Result<Self> {
        config.validate()?;
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs_f64(config.timeout_s))
            .connect_timeout(Duration::from_secs_f64(config.timeout_s))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| anyhow::anyhow!("HTTP initialization failed"))?;
        let descriptor = json!({"id":format!("server:{}",config.id),"type":"server","platform":config.platform,"name":config.name,"address":config.address});
        Ok(Self {
            config,
            client,
            sequence: 0,
            last_sequence: None,
            sequence_at: 0.,
            descriptor,
        })
    }
    fn read(&mut self, now: f64) -> Result<(Value, f64)> {
        let mut request = self
            .client
            .get(&self.config.url)
            .header("Accept", "application/json");
        if !self.config.token_file.as_os_str().is_empty() {
            request = request.bearer_auth(config::read_token(&self.config.token_file, 1)?);
        }
        let response = request
            .send()
            .map_err(|_| anyhow::anyhow!("Device connection failed"))?;
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|n| n > MAX_PAYLOAD as u64)
        {
            bail!("Device HTTP response unavailable");
        }
        let mut raw = Vec::new();
        response
            .take((MAX_PAYLOAD + 1) as u64)
            .read_to_end(&mut raw)
            .map_err(|_| anyhow::anyhow!("Device response failed"))?;
        if raw.len() > MAX_PAYLOAD {
            bail!("Device response exceeds capacity");
        }
        self.validate_document(&raw, now)
    }
    pub fn validate_document(&mut self, raw: &[u8], now: f64) -> Result<(Value, f64)> {
        if !now.is_finite() {
            bail!("Invalid sample timestamp");
        }
        if raw.len() > MAX_PAYLOAD {
            bail!("Device response exceeds capacity");
        }
        let document = config::strict_json(raw)?;
        if !document.is_object() && !document.is_array() {
            bail!("Expected JSON telemetry");
        }
        let stamp = if let Some(path) = &self.config.timestamp_path {
            let stamp = at_path(&document, path)
                .and_then(number)
                .ok_or_else(|| anyhow::anyhow!("Invalid sample timestamp"))?;
            if stamp > now + 60. || now - stamp > self.config.ttl_s {
                bail!("Device sample expired");
            }
            stamp
        } else {
            now
        };
        if let Some(path) = &self.config.sequence_path {
            let seq = at_path(&document, path)
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow::anyhow!("Invalid device sequence"))?;
            if self.last_sequence != Some(seq) {
                self.last_sequence = Some(seq);
                self.sequence_at = now;
            } else if now < self.sequence_at || now - self.sequence_at > self.config.ttl_s {
                bail!("Device sequence stopped");
            }
        }
        Ok((document, stamp))
    }
    pub fn sample_document(&mut self, result: Result<(Value, f64)>, now: f64) -> Result<Value> {
        if !now.is_finite() {
            bail!("Invalid sample timestamp");
        }
        self.sequence += 1;
        let mut snapshot = empty_snapshot(&self.descriptor, self.sequence, now);
        let mut state = source_status(now, Some("Device telemetry unavailable"), true);
        if let Ok((doc, stamp)) = result {
            let (mut valid, mut missing) = (0usize, 0usize);
            for (group, mappings) in [("host", &self.config.host), ("power", &self.config.power)] {
                for m in mappings {
                    let value = mapped(&doc, &m.path, m.scale, m.offset, &m.parse).filter(|v| {
                        if group == "host" {
                            *v >= 0.
                                && (!matches!(m.field.as_str(), "cpu_pct" | "io_wait_pct")
                                    || *v <= 100.)
                        } else {
                            m.field == "cpu_temp_c" || *v >= 0.
                        }
                    });
                    valid += usize::from(value.is_some());
                    missing += usize::from(value.is_none());
                    snapshot[group][&m.field] = json!(value);
                }
            }
            for m in &self.config.sensors {
                let value = mapped(&doc, &m.path, m.scale, m.offset, &m.parse);
                valid += usize::from(value.is_some());
                missing += usize::from(value.is_none());
                snapshot["sensors"].as_array_mut().unwrap().push(json!({"id":m.id,"name":m.name,"kind":m.kind,"unit":m.unit,"value":value,"chip":"device-api","source":"device_api","high":m.high,"crit":m.crit,"updated_at":if value.is_some(){Some(stamp)}else{None},"age_s":if value.is_some(){Some((now-stamp).max(0.))}else{None}}));
                for (threshold, severity) in [(m.crit, "critical"), (m.high, "warning")] {
                    if value.zip(threshold).is_some_and(|(v, t)| v >= t) {
                        snapshot["alerts"].as_array_mut().unwrap().push(json!({"id":format!("sensor/{}",m.id),"severity":severity,"message":text(&format!("{} above threshold",m.name),120)}));
                        break;
                    }
                }
            }
            for (used, total) in [
                ("mem_used_bytes", "mem_total_bytes"),
                ("swap_used_bytes", "swap_total_bytes"),
            ] {
                if number(&snapshot["host"][used])
                    .zip(number(&snapshot["host"][total]))
                    .is_some_and(|(u, t)| u > t)
                {
                    snapshot["host"][used] = Value::Null;
                    valid = valid.saturating_sub(1);
                    missing += 1;
                }
            }
            state = json!({"enabled":true,"ok":missing==0,"updated_at":if valid>0{Some(stamp)}else{None},"age_s":if valid>0{Some((now-stamp).max(0.))}else{None},"error":if missing>0{Some("Some configured device readings are unavailable")}else{None}});
        }
        if state["ok"] != true {
            snapshot["alerts"].as_array_mut().unwrap().insert(
                0,
                json!({"id":"source/device_api","severity":"warning","message":state["error"]}),
            );
        }
        snapshot["sources"]["device_api"] = state;
        finish_snapshot(snapshot, &self.descriptor)
    }
}
impl Collector for JsonDeviceCollector {
    fn descriptor(&self) -> Value {
        self.descriptor.clone()
    }
    fn interval_s(&self) -> f64 {
        self.config.interval_s
    }
    fn sample(&mut self, now: f64, _mono: f64) -> Result<Value> {
        let result = self.read(now);
        self.sample_document(result, now)
    }
}
