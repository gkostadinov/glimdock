//! Read-only Moonraker monitoring. Heater power is PWM duty, never watts.
use crate::{
    compact_text,
    config::NodeConfig,
    finite, number,
    runtime::{empty_snapshot, SourceStatus},
};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const BASE_OBJECTS: &[(&str, &str)] = &[
    ("webhooks", "state,state_message"),
    (
        "print_stats",
        "state,message,filename,print_duration,total_duration,filament_used,info",
    ),
    ("display_status", "progress,message"),
    ("virtual_sdcard", "progress,is_active"),
    ("pause_resume", "is_paused"),
    ("fan", "speed"),
];
pub fn empty_printer(id: &str) -> Value {
    json!({"id":id,"host_name":"","klippy_state":"unknown","state":"unknown","message":"","filename":"","progress_pct":null,"progress_basis":null,"print_duration_s":null,"total_duration_s":null,"current_layer":null,"total_layers":null,"filament_used_mm":null,"slicer_estimated_time_s":null,"remaining_s":null,"eta_at":null,"eta_basis":null,"heaters":[],"temperatures":[],"fan_pct":null,"filament_detected":null,"updated_at":null,"age_s":null,"error":null})
}
fn text(value: &Value, bound: usize) -> String {
    compact_text(value.as_str().unwrap_or(""), bound)
}
fn heater(name: &str) -> bool {
    name == "heater_bed"
        || name
            .strip_prefix("extruder")
            .is_some_and(|s| s.bytes().all(|b| b.is_ascii_digit()))
        || name.starts_with("heater_generic ")
}
pub fn normalize_printer(
    id: &str,
    status: &Value,
    metadata: &Value,
    host_name: &str,
    now: f64,
) -> Value {
    let mut result = empty_printer(id);
    let hooks = &status["webhooks"];
    let stats = &status["print_stats"];
    result["host_name"] = json!(compact_text(host_name, 64));
    result["klippy_state"] = json!(compact_text(
        hooks["state"].as_str().unwrap_or("unknown"),
        24
    ));
    let message = [
        &stats["message"],
        &hooks["state_message"],
        &status["display_status"]["message"],
    ]
    .into_iter()
    .find(|v| v.as_str().is_some_and(|s| !s.is_empty()))
    .cloned()
    .unwrap_or(Value::Null);
    result["message"] = json!(text(&message, 128));
    result["updated_at"] = json!(now);
    result["age_s"] = json!(0);
    let mut state = stats["state"].as_str().unwrap_or("unknown");
    if ![
        "standby",
        "printing",
        "paused",
        "complete",
        "error",
        "cancelled",
    ]
    .contains(&state)
    {
        state = "unknown";
    }
    if state == "printing" && status["pause_resume"]["is_paused"] == true {
        state = "paused";
    }
    result["state"] = json!(state);
    let filename = stats["filename"]
        .as_str()
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("");
    result["filename"] = json!(compact_text(filename, 96));
    for (field, key) in [
        ("print_duration_s", "print_duration"),
        ("total_duration_s", "total_duration"),
        ("filament_used_mm", "filament_used"),
    ] {
        result[field] = json!(finite(&stats[key], 0., f64::MAX));
    }
    for (field, key) in [
        ("current_layer", "current_layer"),
        ("total_layers", "total_layer"),
    ] {
        result[field] = json!(finite(&stats["info"][key], 0., 1_000_000.)
            .filter(|n| n.fract() == 0.)
            .map(|n| n as u64));
    }
    let display = finite(&status["display_status"]["progress"], 0., 1.);
    let progress = display.or_else(|| finite(&status["virtual_sdcard"]["progress"], 0., 1.));
    result["progress_basis"] = json!(progress.map(|_| if display.is_some() {
        "display-status"
    } else {
        "file-position"
    }));
    result["progress_pct"] = json!(progress.map(|p| (p * 10000.).round() / 100.));
    result["slicer_estimated_time_s"] = json!(finite(&metadata["estimated_time"], 0.001, f64::MAX));
    if state == "complete" {
        result["progress_pct"] = json!(100);
        result["remaining_s"] = json!(0);
        result["eta_basis"] = json!("completed");
    } else if ["printing", "paused"].contains(&state) {
        if let (Some(p), Some(duration)) = (progress, number(&result["print_duration_s"])) {
            if (0.05..1.).contains(&p) && duration >= 120. {
                let remaining = duration * (1. - p) / p;
                result["remaining_s"] = json!(remaining.round());
                result["eta_basis"] = json!("progress-average");
                if state == "printing" {
                    result["eta_at"] = json!((now + remaining).round());
                }
            }
        }
    }
    let mut heaters = Vec::new();
    let mut temperatures = Vec::new();
    let mut filaments = Vec::new();
    if let Some(items) = status.as_object() {
        for (name, values) in items {
            if !values.is_object() {
                continue;
            }
            if heater(name) && heaters.len() < 4 {
                let label = if name == "heater_bed" {
                    "Bed"
                } else if name == "extruder" {
                    "Nozzle"
                } else {
                    name.split_once(' ').map(|(_, n)| n).unwrap_or(name)
                };
                heaters.push(json!({"name":compact_text(label,32),"temp_c":finite(&values["temperature"],-50.,500.),"target_c":finite(&values["target"],0.,500.),"duty_pct":finite(&values["power"],0.,1.).map(|p|(p*10000.).round()/100.)}));
            } else if let Some(label) = name.strip_prefix("temperature_sensor ") {
                if temperatures.len() < 8 {
                    temperatures.push(json!({"name":compact_text(label,32),"temp_c":finite(&values["temperature"],-50.,250.)}));
                }
            }
            if (name.starts_with("filament_switch_sensor ")
                || name.starts_with("filament_motion_sensor "))
                && values["enabled"] == true
            {
                if let Some(detected) = values["filament_detected"].as_bool() {
                    filaments.push(detected);
                }
            }
        }
    }
    result["heaters"] = heaters.into();
    result["temperatures"] = temperatures.into();
    result["fan_pct"] =
        json!(finite(&status["fan"]["speed"], 0., 1.).map(|p| (p * 10000.).round() / 100.));
    result["filament_detected"] =
        json!((!filaments.is_empty()).then(|| filaments.iter().all(|v| *v)));
    if result["klippy_state"] != "ready" {
        result["state"] = json!("unknown");
        for key in ["progress_pct", "remaining_s", "eta_at", "eta_basis"] {
            result[key] = Value::Null;
        }
    }
    result
}
pub struct MoonrakerReader {
    pub config: NodeConfig,
    client: reqwest::Client,
    objects: BTreeMap<String, String>,
    discovered: bool,
    host_name: String,
    metadata: Value,
    metadata_key: Option<String>,
    metadata_retry_at: f64,
    previous_eventtime: Option<f64>,
    previous_duration: Option<f64>,
    previous_filename: Option<String>,
}
impl MoonrakerReader {
    pub fn new(config: NodeConfig) -> Result<Self> {
        Ok(Self {
            client: crate::runtime::http_client(config.timeout_s)?,
            config,
            objects: BASE_OBJECTS[..2]
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            discovered: false,
            host_name: String::new(),
            metadata: json!({}),
            metadata_key: None,
            metadata_retry_at: 0.,
            previous_eventtime: None,
            previous_duration: None,
            previous_filename: None,
        })
    }
    async fn get(&self, path: &str, params: Option<&BTreeMap<String, String>>) -> Result<Value> {
        if ![
            "/server/info",
            "/printer/info",
            "/printer/objects/list",
            "/printer/objects/query",
            "/server/files/metadata",
        ]
        .contains(&path)
        {
            bail!("Unsupported printer endpoint");
        }
        let mut req = self
            .client
            .get(format!("{}{path}", self.config.url))
            .header("Accept", "application/json");
        if let Some(p) = params {
            req = req.query(p);
        }
        if !self.config.api_key_file.is_empty() {
            let key = crate::runtime::read_secret(&self.config.api_key_file, 1)?;
            req = req.header("X-Api-Key", key);
        }
        let document = crate::runtime::bounded_http_json(req, 64 * 1024).await?;
        if document.get("error").is_some() || !document["result"].is_object() {
            bail!("Invalid Moonraker response");
        }
        Ok(document["result"].clone())
    }
    async fn discover(&mut self) -> Result<()> {
        let data = self.get("/printer/objects/list", None).await?;
        let objects = data["objects"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Printer object list unavailable"))?;
        let names = objects.iter().filter_map(Value::as_str).collect::<Vec<_>>();
        if !["webhooks", "print_stats"]
            .iter()
            .all(|n| names.contains(n))
        {
            bail!("Printer core objects unavailable");
        }
        self.objects = BASE_OBJECTS
            .iter()
            .filter(|(n, _)| names.contains(n))
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        let mut sorted = names;
        sorted.sort();
        let (mut heaters, mut temps, mut filaments) = (0, 0, 0);
        for name in sorted {
            let fields = if heater(name) && heaters < 4 {
                heaters += 1;
                Some("temperature,target,power")
            } else if name.starts_with("temperature_sensor ") && temps < 8 {
                temps += 1;
                Some("temperature")
            } else if (name.starts_with("filament_switch_sensor ")
                || name.starts_with("filament_motion_sensor "))
                && filaments < 4
            {
                filaments += 1;
                Some("enabled,filament_detected")
            } else {
                None
            };
            if let Some(fields) = fields {
                self.objects.insert(name.to_string(), fields.to_string());
            }
        }
        self.discovered = true;
        if let Ok(info) = self.get("/printer/info", None).await {
            self.host_name = text(&info["hostname"], 64);
        }
        Ok(())
    }
    pub async fn read(&mut self) -> Result<Value> {
        if !self.discovered {
            let _ = self.discover().await;
        }
        let data = match self
            .get("/printer/objects/query", Some(&self.objects))
            .await
        {
            Ok(v) => v,
            Err(error) => {
                let server = self.get("/server/info", None).await?;
                let state = server["klippy_state"].as_str().unwrap_or("");
                if ["startup", "shutdown", "error", "disconnected"].contains(&state) {
                    self.discovered = false;
                    let mut message = format!("Klipper is {state}");
                    if let Ok(info) = self.get("/printer/info", None).await {
                        if let Some(s) = info["state_message"].as_str() {
                            message = s.to_string();
                        }
                    }
                    json!({"status":{"webhooks":{"state":state,"state_message":message}}})
                } else {
                    return Err(error);
                }
            }
        };
        let status = &data["status"];
        if !status.is_object()
            || !status["webhooks"].is_object()
            || (status["webhooks"]["state"] == "ready" && !status["print_stats"].is_object())
        {
            bail!("Printer status incomplete");
        }
        let filename = status["print_stats"]["filename"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let duration = finite(&status["print_stats"]["print_duration"], 0., f64::MAX);
        let eventtime = finite(&data["eventtime"], 0., f64::MAX);
        let restarted = eventtime
            .zip(self.previous_eventtime)
            .is_some_and(|(a, b)| a < b)
            || duration
                .zip(self.previous_duration)
                .is_some_and(|(a, b)| a < b);
        if restarted || self.previous_filename.as_ref() != Some(&filename) {
            self.metadata = json!({});
            self.metadata_key = None;
            self.metadata_retry_at = 0.;
        }
        if restarted {
            self.discovered = false;
        }
        let now = crate::epoch();
        if !filename.is_empty()
            && self.metadata_key.as_ref() != Some(&filename)
            && now >= self.metadata_retry_at
        {
            let params = BTreeMap::from([("filename".into(), filename.clone())]);
            match self.get("/server/files/metadata", Some(&params)).await {
                Ok(v) => {
                    self.metadata = v;
                    self.metadata_key = Some(filename.clone());
                }
                Err(_) => self.metadata_retry_at = now + 60.,
            }
        }
        self.previous_eventtime = eventtime;
        self.previous_duration = duration;
        self.previous_filename = Some(filename);
        Ok(
            json!({"printer":normalize_printer(&self.config.id,status,&self.metadata,&self.host_name,now),"generated_at":now,"error":null}),
        )
    }
}
pub fn printer_snapshot(
    config: &NodeConfig,
    state: &SourceStatus,
    data: Option<&Value>,
    sequence: u64,
    now: f64,
) -> Value {
    let mut printer = data
        .map(|d| d["printer"].clone())
        .unwrap_or_else(|| empty_printer(&config.id));
    let stamp = number(&printer["updated_at"]).or(state.updated_at);
    let age = stamp.map(|t| (now - t).max(0.));
    let mut own = serde_json::to_value(state).unwrap();
    if !state.ok || age.is_none() || age.is_some_and(|a| a > config.ttl_s) {
        let previous = printer;
        printer = empty_printer(&config.id);
        for key in ["host_name", "filename"] {
            printer[key] = previous[key].clone();
        }
        printer["updated_at"] = json!(stamp);
        printer["age_s"] = json!(age);
        printer["error"] = json!(state
            .error
            .as_deref()
            .unwrap_or("Printer sample expired/unavailable"));
        own["ok"] = json!(false);
        own["error"] = printer["error"].clone();
    } else {
        printer["age_s"] = json!(age);
    }
    own["updated_at"] = json!(stamp);
    own["age_s"] = json!(age);
    printer["ttl_s"] = json!(config.ttl_s);
    let host = url::Url::parse(&config.url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default();
    let mut snapshot = empty_snapshot(&config.name, &host, sequence, now);
    let mut alerts = Vec::new();
    let mut alert = |id: &str, severity: &str, message: &str| {
        alerts.push(json!({"id":format!("printer/{}/{id}",config.id),"severity":severity,"message":compact_text(message,120)}))
    };
    let klippy = printer["klippy_state"].as_str().unwrap_or("unknown");
    let job = printer["state"].as_str().unwrap_or("unknown");
    let message = printer["message"].as_str().unwrap_or("");
    if !printer["error"].is_null() && state.error.as_deref() != Some("initializing") {
        alert("unavailable", "warning", "Printer telemetry unavailable");
    } else if ["shutdown", "error"].contains(&klippy) {
        alert(
            "klipper",
            "critical",
            if message.is_empty() {
                "Klipper error/shutdown"
            } else {
                message
            },
        );
    } else if ["startup", "disconnected"].contains(&klippy) {
        alert(
            "klipper",
            "warning",
            if message.is_empty() {
                "Klipper starting/disconnected"
            } else {
                message
            },
        );
    } else if job == "error" {
        alert(
            "print",
            "critical",
            if message.is_empty() {
                "Print job failed"
            } else {
                message
            },
        );
    } else if job == "paused" {
        alert("paused", "warning", "Print paused");
    }
    if printer["filament_detected"] == false && ["printing", "paused"].contains(&job) {
        alert("filament", "warning", "Printer filament not detected");
    }
    if klippy == "ready" && job == "unknown" {
        alert("status", "warning", "Printer job state unavailable");
    }
    snapshot["alerts"] = alerts.into();
    snapshot["sources"] = json!({"moonraker":own});
    snapshot["printer"] = printer;
    snapshot
}
