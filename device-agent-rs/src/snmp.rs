//! Bounded, read-only SNMP requests through native Net-SNMP. Credentials never
//! enter arguments, logs or telemetry; configuration selects numeric OIDs only.
use crate::{
    config::{read_bounded, strict_json, validate_identity, validate_platform},
    protocol::{empty_snapshot, finish_snapshot, source_status, text, Rates},
    Collector, HOST_FIELDS, MAX_CONFIG,
};
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::{OsStr, OsString},
    fs::OpenOptions,
    io::{Read, Write},
    net::IpAddr,
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub const MAX_OIDS: usize = 128;
pub const MAX_OUTPUT: usize = 32 * 1024;
pub const BATCH_SIZE: usize = 32;
pub const SYS_UPTIME: &str = ".1.3.6.1.2.1.1.3.0";
pub const HOST_UPTIME: &str = ".1.3.6.1.2.1.25.1.1.0";
pub const STORAGE: &str = ".1.3.6.1.2.1.25.2.3.1";
pub const INTERFACE: &str = ".1.3.6.1.2.1.2.2.1";
pub const IF_EXTENDED: &str = ".1.3.6.1.2.1.31.1.1.1";

fn one() -> f64 {
    1.0
}
fn counter_bits() -> u8 {
    64
}
fn other() -> String {
    "other".into()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InterfaceConfig {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub index: u32,
    #[serde(default = "counter_bits")]
    pub counter_bits: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StorageConfig {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub index: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarConfig {
    pub oid: String,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SensorConfig {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub oid: String,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "other")]
    pub kind: String,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub high: Option<f64>,
    #[serde(default)]
    pub crit: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct SnmpConfig {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub address: String,
    pub port: u16,
    pub credential_file: String,
    pub interval_s: f64,
    pub timeout_s: f64,
    pub interfaces: Vec<InterfaceConfig>,
    pub storage: Vec<StorageConfig>,
    pub memory_index: Option<u32>,
    pub cpu_oids: Vec<String>,
    pub host_metrics: BTreeMap<String, ScalarConfig>,
    pub sensors: Vec<SensorConfig>,
}

impl Default for SnmpConfig {
    fn default() -> Self {
        Self {
            id: "router".into(),
            name: "Router".into(),
            platform: "router".into(),
            address: String::new(),
            port: 161,
            credential_file: String::new(),
            interval_s: 10.0,
            timeout_s: 2.0,
            interfaces: vec![],
            storage: vec![],
            memory_index: None,
            cpu_oids: vec![],
            host_metrics: BTreeMap::new(),
            sensors: vec![],
        }
    }
}

fn label(value: &str, cap: usize, empty: bool) -> Result<()> {
    ensure!(
        (empty || !value.is_empty())
            && value.chars().count() <= cap
            && !value.chars().any(char::is_control),
        "Invalid SNMP label"
    );
    Ok(())
}

/// Canonical numeric OIDs prevent flags, named-MIB lookups and command injection.
pub fn oid(value: &str) -> Result<String> {
    ensure!(value.len() <= 512, "Invalid SNMP OID");
    let pieces = value
        .strip_prefix('.')
        .unwrap_or(value)
        .split('.')
        .collect::<Vec<_>>();
    ensure!((2..=64).contains(&pieces.len()), "Invalid SNMP OID");
    let mut numbers = Vec::with_capacity(pieces.len());
    for part in pieces {
        ensure!(
            !part.is_empty() && part.len() <= 10 && part.bytes().all(|b| b.is_ascii_digit()),
            "SNMP OIDs must be numeric identifiers"
        );
        numbers.push(
            part.parse::<u32>()
                .map_err(|_| anyhow::anyhow!("Invalid SNMP OID"))?,
        );
    }
    ensure!(
        numbers[0] <= 2 && (numbers[0] == 2 || numbers[1] <= 39),
        "Invalid SNMP OID"
    );
    Ok(format!(
        ".{}",
        numbers
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".")
    ))
}

impl SnmpConfig {
    pub fn validate(&self) -> Result<()> {
        validate_identity(&self.id)?;
        label(&self.name, 96, false)?;
        validate_platform(&self.platform)?;
        let hostname = !self.address.is_empty()
            && self.address.len() <= 253
            && self.address.as_bytes()[0].is_ascii_alphanumeric()
            && self.address.as_bytes()[self.address.len() - 1].is_ascii_alphanumeric()
            && self
                .address
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
        ensure!(
            self.address.len() <= 253 && (self.address.parse::<IpAddr>().is_ok() || hostname),
            "SNMP address must be a hostname or IP address"
        );
        ensure!(self.port != 0, "Invalid SNMP port");
        ensure!(
            Path::new(&self.credential_file).is_absolute(),
            "SNMP credential_file must be an absolute private path"
        );
        ensure!(
            self.interval_s.is_finite() && (2.0..=300.0).contains(&self.interval_s),
            "Invalid SNMP interval"
        );
        ensure!(
            self.timeout_s.is_finite() && (0.5..=3.0).contains(&self.timeout_s),
            "Invalid SNMP timeout"
        );
        ensure!(
            self.interfaces.len() <= 8 && self.storage.len() <= 8,
            "SNMP supports at most eight interfaces and storage entries"
        );
        let mut ids = BTreeSet::new();
        let mut indices = BTreeSet::new();
        for row in &self.interfaces {
            validate_identity(&row.id)?;
            if !row.name.is_empty() {
                label(&row.name, 96, false)?;
            }
            ensure!(
                ids.insert(&row.id)
                    && indices.insert(row.index)
                    && row.index > 0
                    && row.index <= i32::MAX as u32,
                "Invalid or duplicate SNMP interface"
            );
            ensure!(
                [32, 64].contains(&row.counter_bits),
                "SNMP interface counter_bits must be 32 or 64"
            );
        }
        ids.clear();
        indices.clear();
        for row in &self.storage {
            validate_identity(&row.id)?;
            if !row.name.is_empty() {
                label(&row.name, 96, false)?;
            }
            ensure!(
                ids.insert(&row.id)
                    && indices.insert(row.index)
                    && row.index > 0
                    && row.index <= i32::MAX as u32,
                "Invalid or duplicate SNMP storage"
            );
        }
        ensure!(
            self.memory_index
                .is_none_or(|v| v > 0 && v <= i32::MAX as u32),
            "Invalid SNMP memory index"
        );
        ensure!(
            self.cpu_oids.len() <= 16 && self.sensors.len() <= 32,
            "Too many SNMP CPU or sensor OIDs"
        );
        for value in &self.cpu_oids {
            oid(value)?;
        }
        for (key, mapping) in &self.host_metrics {
            ensure!(
                HOST_FIELDS.contains(&key.as_str()),
                "Unsupported SNMP host metric"
            );
            oid(&mapping.oid)?;
            ensure!(
                mapping.scale.is_finite() && mapping.offset.is_finite(),
                "SNMP scale and offset must be finite"
            );
        }
        ids.clear();
        for sensor in &self.sensors {
            validate_identity(&sensor.id)?;
            ensure!(ids.insert(&sensor.id), "Duplicate SNMP sensor id");
            if !sensor.name.is_empty() {
                label(&sensor.name, 96, false)?;
            }
            label(&sensor.kind, 32, true)?;
            label(&sensor.unit, 16, true)?;
            oid(&sensor.oid)?;
            ensure!(
                sensor.scale.is_finite()
                    && sensor.offset.is_finite()
                    && sensor.high.is_none_or(f64::is_finite)
                    && sensor.crit.is_none_or(f64::is_finite),
                "SNMP scale and thresholds must be finite"
            );
            ensure!(
                !matches!((sensor.high,sensor.crit), (Some(h),Some(c)) if h >= c),
                "SNMP warning threshold must be below critical"
            );
        }
        ensure!(
            requested_oids(self)?.len() <= MAX_OIDS,
            "SNMP configuration exceeds 128 requested OIDs"
        );
        Ok(())
    }

    fn normalize(mut self) -> Result<Self> {
        self.validate()?;
        for row in &mut self.interfaces {
            if row.name.is_empty() {
                row.name = row.id.clone();
            }
        }
        for row in &mut self.storage {
            if row.name.is_empty() {
                row.name = row.id.clone();
            }
        }
        let mut seen = BTreeSet::new();
        self.cpu_oids = self
            .cpu_oids
            .iter()
            .map(|v| oid(v))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|v| seen.insert(v.clone()))
            .collect();
        for mapping in self.host_metrics.values_mut() {
            mapping.oid = oid(&mapping.oid)?;
        }
        for sensor in &mut self.sensors {
            sensor.oid = oid(&sensor.oid)?;
            if sensor.name.is_empty() {
                sensor.name = sensor.id.clone();
            }
        }
        Ok(self)
    }
}

pub fn interface_oids(row: &InterfaceConfig) -> BTreeMap<&'static str, String> {
    let i = row.index;
    BTreeMap::from([
        (
            "rx",
            if row.counter_bits == 64 {
                format!("{IF_EXTENDED}.6.{i}")
            } else {
                format!("{INTERFACE}.10.{i}")
            },
        ),
        (
            "tx",
            if row.counter_bits == 64 {
                format!("{IF_EXTENDED}.10.{i}")
            } else {
                format!("{INTERFACE}.16.{i}")
            },
        ),
        ("status", format!("{INTERFACE}.8.{i}")),
        ("speed_mbps", format!("{IF_EXTENDED}.15.{i}")),
        ("errors_in", format!("{INTERFACE}.14.{i}")),
        ("errors_out", format!("{INTERFACE}.20.{i}")),
        ("drops_in", format!("{INTERFACE}.13.{i}")),
        ("drops_out", format!("{INTERFACE}.19.{i}")),
    ])
}

pub fn storage_oids(index: u32) -> [String; 3] {
    [4, 5, 6].map(|column| format!("{STORAGE}.{column}.{index}"))
}

pub fn requested_oids(config: &SnmpConfig) -> Result<Vec<String>> {
    let mut values = vec![SYS_UPTIME.into(), HOST_UPTIME.into()];
    values.extend(
        config
            .cpu_oids
            .iter()
            .map(|v| oid(v))
            .collect::<Result<Vec<_>>>()?,
    );
    for row in &config.interfaces {
        values.extend(interface_oids(row).into_values());
    }
    for row in &config.storage {
        values.extend(storage_oids(row.index));
    }
    if let Some(index) = config.memory_index {
        values.extend(storage_oids(index));
    }
    for sensor in &config.sensors {
        values.push(oid(&sensor.oid)?);
    }
    for mapping in config.host_metrics.values() {
        values.push(oid(&mapping.oid)?);
    }
    let mut seen = BTreeSet::new();
    values.retain(|v| seen.insert(v.clone()));
    Ok(values)
}

/// Integers stay JSON integers through parsing so Counter64 deltas lose no bits.
pub fn parse_values(raw: &str, expected: &[String]) -> Result<BTreeMap<String, Value>> {
    ensure!(raw.len() <= MAX_OUTPUT, "SNMP response exceeds capacity");
    let allowed = expected.iter().collect::<BTreeSet<_>>();
    let mut values = BTreeMap::new();
    let number = regex::Regex::new(r"^[-+]?\d+(?:\.\d+)?(?:[eE][-+]?\d+)?$")?;
    for line in raw.lines() {
        let split = line
            .find(char::is_whitespace)
            .ok_or_else(|| anyhow::anyhow!("Invalid SNMP numeric response"))?;
        let identity = oid(&line[..split])?;
        ensure!(
            allowed.contains(&identity) && !values.contains_key(&identity),
            "Unexpected SNMP OID response"
        );
        let value = line[split..].trim();
        let parsed = if !number.is_match(value) {
            Value::Null
        } else if let Ok(integer) = value.parse::<u64>() {
            json!(integer)
        } else if let Ok(integer) = value.parse::<i64>() {
            json!(integer)
        } else {
            value
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map_or(Value::Null, |v| json!(v))
        };
        values.insert(identity, parsed);
    }
    ensure!(!values.is_empty(), "Empty SNMP response");
    Ok(values)
}

fn secure_value(secret: &Value, key: &str, minimum: usize) -> Result<String> {
    let value = secret
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Invalid SNMP credential format"))?;
    ensure!(
        (minimum..=256).contains(&value.len())
            && value
                .bytes()
                .all(|b| (33..=126).contains(&b) && !b"\"\\#".contains(&b)),
        "Invalid SNMP credential format"
    );
    Ok(value.into())
}

pub fn credential_config(path: &Path) -> Result<String> {
    let raw = read_bounded(path, MAX_CONFIG)
        .map_err(|_| anyhow::anyhow!("SNMP credentials unavailable"))?;
    let secret = strict_json(&raw).map_err(|_| anyhow::anyhow!("Invalid SNMP credentials"))?;
    let object = secret
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Invalid SNMP credentials"))?;
    let version = secret.get("version").and_then(Value::as_str).unwrap_or("");
    let allowed: &[&str] = match version {
        "2c" => &["version", "community"],
        "3" => &[
            "version",
            "username",
            "security_level",
            "auth_protocol",
            "auth_password",
            "priv_protocol",
            "priv_password",
        ],
        _ => bail!("Unsupported SNMP credential version"),
    };
    ensure!(
        object.keys().all(|key| allowed.contains(&key.as_str())),
        "Unsupported SNMP credential fields"
    );
    let mut lines = vec![format!("defVersion {version}")];
    if version == "2c" {
        lines.push(format!(
            "defCommunity {}",
            secure_value(&secret, "community", 1)?
        ));
    } else {
        let level = secret
            .get("security_level")
            .map(|v| v.as_str().unwrap_or(""))
            .unwrap_or("authPriv");
        let auth = secret
            .get("auth_protocol")
            .map(|v| v.as_str().unwrap_or(""))
            .unwrap_or("SHA");
        ensure!(
            ["authNoPriv", "authPriv"].contains(&level),
            "SNMPv3 requires authentication"
        );
        ensure!(
            ["SHA", "SHA-224", "SHA-256", "SHA-384", "SHA-512"].contains(&auth),
            "Unsupported SNMP authentication protocol"
        );
        lines.extend([
            format!("defSecurityName {}", secure_value(&secret, "username", 1)?),
            format!("defSecurityLevel {level}"),
            format!("defAuthType {auth}"),
            format!(
                "defAuthPassphrase {}",
                secure_value(&secret, "auth_password", 8)?
            ),
        ]);
        if level == "authPriv" {
            let privacy = secret
                .get("priv_protocol")
                .map(|v| v.as_str().unwrap_or(""))
                .unwrap_or("AES");
            ensure!(privacy == "AES", "Unsupported SNMP privacy protocol");
            lines.extend([
                format!("defPrivType {privacy}"),
                format!(
                    "defPrivPassphrase {}",
                    secure_value(&secret, "priv_password", 8)?
                ),
            ]);
        }
    }
    lines.extend([
        "noPersistentLoad true".into(),
        "noPersistentSave true".into(),
    ]);
    Ok(lines.join("\n") + "\n")
}

#[cfg(unix)]
fn private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(windows)]
fn private_directory(path: &Path) -> Result<()> {
    windows_private(path)
}

// A protected DACL grants full access to this object's owner only. Apply it to
// the empty directory before creating the credential file, then to the file.
#[cfg(windows)]
fn windows_private(path: &Path) -> Result<()> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
            value: *const u16,
            revision: u32,
            descriptor: *mut *mut c_void,
            size: *mut u32,
        ) -> i32;
        fn SetFileSecurityW(path: *const u16, information: u32, descriptor: *mut c_void) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    let encoded = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let sddl = "D:P(A;OICI;FA;;;OW)"
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut descriptor = ptr::null_mut();
    unsafe {
        ensure!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut()
            ) != 0,
            "Unable to secure SNMP credential directory"
        );
        let result = SetFileSecurityW(encoded.as_ptr(), 0x4 | 0x80000000, descriptor);
        LocalFree(descriptor);
        ensure!(result != 0, "Unable to secure SNMP credential directory");
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn private_directory(_: &Path) -> Result<()> {
    bail!("Private SNMP credentials unsupported on this platform")
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(windows)]
    windows_private(path)?;
    file.write_all(bytes)?;
    Ok(())
}

pub struct SnmpRequest {
    pub args: Vec<String>,
    pub env: BTreeMap<String, OsString>,
    pub timeout: Duration,
    pub max_output: usize,
}

/// A reader retains at most limit+1 bytes; the supervisor kills on overflow or
/// deadline. Stderr is discarded because a native CLI may echo credentials.
pub fn bounded_command(
    program: &OsStr,
    args: &[String],
    env: &BTreeMap<String, OsString>,
    timeout: Duration,
    max_output: usize,
) -> Result<String> {
    ensure!(
        max_output <= MAX_OUTPUT && !timeout.is_zero(),
        "Invalid bounded SNMP request"
    );
    let mut child = Command::new(program)
        .args(args)
        .envs(env)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| anyhow::anyhow!("SNMP request failed"))?;
    let mut pipe = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("SNMP request failed"))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let flag = overflow.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = Vec::with_capacity(max_output.min(4096));
        let result = (|| -> std::io::Result<Vec<u8>> {
            let mut buffer = [0u8; 1024];
            loop {
                let n = pipe.read(&mut buffer)?;
                if n == 0 {
                    return Ok(output);
                }
                let room = (max_output + 1).saturating_sub(output.len());
                output.extend_from_slice(&buffer[..n.min(room)]);
                if output.len() > max_output {
                    flag.store(true, Ordering::Release);
                    return Ok(output);
                }
            }
        })();
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if overflow.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("SNMP response exceeds capacity");
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("SNMP request failed");
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("SNMP request timed out");
        }
        thread::sleep(Duration::from_millis(5));
    };
    let output = rx
        .recv_timeout(Duration::from_millis(250))
        .map_err(|_| anyhow::anyhow!("SNMP request failed"))?
        .map_err(|_| anyhow::anyhow!("SNMP request failed"))?;
    ensure!(output.len() <= max_output, "SNMP response exceeds capacity");
    ensure!(status.success(), "SNMP request failed");
    Ok(String::from_utf8_lossy(&output).into_owned())
}

type Runner = Box<dyn FnMut(&SnmpRequest) -> Result<String> + Send>;
pub struct SnmpCollector {
    pub config: SnmpConfig,
    descriptor: Value,
    runner: Runner,
    rates: Rates,
    sequence: u64,
    last_uptime: Option<f64>,
}

impl SnmpCollector {
    pub fn new(config: SnmpConfig) -> Result<Self> {
        Self::with_runner(config, |request| {
            bounded_command(
                OsStr::new("snmpget"),
                &request.args,
                &request.env,
                request.timeout,
                request.max_output,
            )
        })
    }
    /// Injection is for embedding/fixtures; executable selection is absent from
    /// deserialized configuration and the production collector always snmpget.
    pub fn with_runner<F>(config: SnmpConfig, runner: F) -> Result<Self>
    where
        F: FnMut(&SnmpRequest) -> Result<String> + Send + 'static,
    {
        let config = config.normalize()?;
        let descriptor = json!({"id":format!("server:{}",config.id),"type":"server","platform":config.platform,"name":config.name,"address":config.address});
        Ok(Self {
            config,
            descriptor,
            runner: Box::new(runner),
            rates: Rates::default(),
            sequence: 0,
            last_uptime: None,
        })
    }

    pub fn read(&mut self) -> Result<BTreeMap<String, Value>> {
        let credentials = credential_config(Path::new(&self.config.credential_file))?;
        let directory = tempfile::Builder::new()
            .prefix("glimdock-snmp-")
            .tempdir()?;
        private_directory(directory.path())?;
        write_private(&directory.path().join("snmp.conf"), credentials.as_bytes())?;
        let env = BTreeMap::from([
            ("LC_ALL".into(), OsString::from("C")),
            ("LANG".into(), OsString::from("C")),
            ("MIBS".into(), OsString::from("")),
            ("SNMPCONFPATH".into(), directory.path().as_os_str().into()),
            (
                "SNMP_PERSISTENT_DIR".into(),
                directory.path().as_os_str().into(),
            ),
            (
                "SNMP_PERSISTENT_FILE".into(),
                directory.path().join("persistent.conf").into_os_string(),
            ),
        ]);
        let target = if self.config.address.contains(':') {
            format!("udp6:[{}]:{}", self.config.address, self.config.port)
        } else {
            format!("udp:{}:{}", self.config.address, self.config.port)
        };
        let requested = requested_oids(&self.config)?;
        let mut values = BTreeMap::new();
        for batch in requested.chunks(BATCH_SIZE) {
            let mut args = vec![
                "-Cf".into(),
                "-OnqteU".into(),
                "-t".into(),
                self.config.timeout_s.to_string(),
                "-r".into(),
                "0".into(),
                target.clone(),
            ];
            args.extend_from_slice(batch);
            let request = SnmpRequest {
                args,
                env: env.clone(),
                timeout: Duration::from_secs_f64(self.config.timeout_s + 1.0),
                max_output: MAX_OUTPUT,
            };
            let raw = (self.runner)(&request)?;
            values.extend(parse_values(&raw, batch)?);
        }
        Ok(values)
    }

    fn render(&mut self, values: &BTreeMap<String, Value>, now: f64, mono: f64) -> Result<Value> {
        let mut result = empty_snapshot(&self.descriptor, self.sequence, now);
        result["platform"] = json!({"os":self.config.platform,"transport":"snmp","interfaces":[],"uptime_scope":null});
        result["host"]["os"] = json!(self.config.platform);
        let numeric = |identity: &str| {
            values
                .get(identity)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite())
        };
        let host_uptime = numeric(HOST_UPTIME).filter(|v| *v >= 0.0);
        let uptime = host_uptime.or_else(|| numeric(SYS_UPTIME).filter(|v| *v >= 0.0));
        result["platform"]["uptime_scope"] = json!(if host_uptime.is_some() {
            "host"
        } else {
            "snmp-agent"
        });
        result["host"]["uptime_s"] = json!(uptime.map(|v| round(v / 100.0)));
        if matches!((uptime,self.last_uptime),(Some(current),Some(last)) if current < last) {
            self.rates.clear();
        }
        self.last_uptime = uptime;
        result["sources"]["snmp"] = source_status(
            now,
            if uptime.is_some() {
                None
            } else {
                Some("SNMP uptime unavailable")
            },
            true,
        );
        // A responding vendor device may not implement either uptime OID.
        // Preserve evidence of numeric telemetry even when this source is
        // partial so the dashboard reports degraded rather than offline.
        if uptime.is_none() && values.values().any(Value::is_number) {
            result["sources"]["snmp"]["updated_at"] = json!(now);
            result["sources"]["snmp"]["age_s"] = json!(0);
        }
        if !self.config.cpu_oids.is_empty() {
            let cores = self
                .config
                .cpu_oids
                .iter()
                .map(|key| numeric(key).filter(|v| (0.0..=100.0).contains(v)))
                .collect::<Vec<_>>();
            let average = cores
                .iter()
                .copied()
                .collect::<Option<Vec<_>>>()
                .map(|v| round(v.iter().sum::<f64>() / v.len() as f64));
            result["host"]["cpu_cores"] = json!(cores);
            result["host"]["cpu_pct"] = json!(average);
            set_status(
                &mut result,
                "snmp_cpu",
                now,
                cores.iter().all(Option::is_some),
            );
        }
        let capacity = |index| {
            let fields = storage_oids(index);
            let (allocation, total, used) = (
                numeric(&fields[0]),
                numeric(&fields[1]),
                numeric(&fields[2]),
            );
            match (allocation, total, used) {
                (Some(a), Some(t), Some(u))
                    if a > 0.0
                        && t >= 0.0
                        && u >= 0.0
                        && u <= t
                        && (u * a).is_finite()
                        && (t * a).is_finite() =>
                {
                    (Some(u * a), Some(t * a))
                }
                _ => (None, None),
            }
        };
        if let Some(index) = self.config.memory_index {
            let (used, total) = capacity(index);
            result["host"]["mem_used_bytes"] = json!(used);
            result["host"]["mem_total_bytes"] = json!(total);
            set_status(
                &mut result,
                "snmp_memory",
                now,
                used.is_some() && total.is_some(),
            );
        }
        if !self.config.storage.is_empty() {
            let rows = self.config.storage.iter().map(|row| {
                let (used,total) = capacity(row.index);
                json!({"id":format!("snmp/{}",row.id),"name":row.name,"used_bytes":used,"total_bytes":total,"status":if total.is_some(){"online"}else{"unknown"}})
            }).collect::<Vec<_>>();
            let available = rows.iter().all(|r| !r["total_bytes"].is_null());
            result["storage"] = json!(rows);
            set_status(&mut result, "snmp_storage", now, available);
        }
        if !self.config.interfaces.is_empty() {
            let mut rows = Vec::new();
            let mut available = true;
            for interface in &self.config.interfaces {
                let ids = interface_oids(interface);
                let counter = |key: &str| values.get(&ids[key]).and_then(Value::as_u64);
                let status = numeric(&ids["status"]);
                let rx =
                    counter("rx").filter(|v| interface.counter_bits == 64 || *v <= u32::MAX as u64);
                let tx =
                    counter("tx").filter(|v| interface.counter_bits == 64 || *v <= u32::MAX as u64);
                available &= rx.is_some() && tx.is_some() && status.is_some();
                rows.push(json!({"id":interface.id,"name":interface.name,"index":interface.index,"status":if status == Some(1.0){"up"}else if status.is_some(){"down"}else{"unknown"},"speed_mbps":numeric(&ids["speed_mbps"]).filter(|v| *v>=0.0),"mtu":null,
                    "rx_bps":self.rates.rate(&format!("rx/{}",interface.id),rx,mono),"tx_bps":self.rates.rate(&format!("tx/{}",interface.id),tx,mono),
                    "errors_in":counter("errors_in"),"errors_out":counter("errors_out"),"drops_in":counter("drops_in"),"drops_out":counter("drops_out")}));
            }
            for (output, key) in [("net_rx_bps", "rx_bps"), ("net_tx_bps", "tx_bps")] {
                let total = rows
                    .iter()
                    .map(|row| row[key].as_f64())
                    .collect::<Option<Vec<_>>>()
                    .map(|v| round(v.iter().sum()));
                result["host"][output] = json!(total);
            }
            result["limits"]["network_interfaces"] =
                json!(rows.iter().map(|r| &r["name"]).collect::<Vec<_>>());
            result["platform"]["interfaces"] = json!(rows);
            set_status(&mut result, "snmp_interfaces", now, available);
        }
        if !self.config.host_metrics.is_empty() {
            let mut available = true;
            for (key, mapping) in &self.config.host_metrics {
                let value = numeric(&mapping.oid)
                    .map(|v| v * mapping.scale + mapping.offset)
                    .filter(|v| {
                        v.is_finite()
                            && *v >= 0.0
                            && (!["cpu_pct", "io_wait_pct"].contains(&key.as_str()) || *v <= 100.0)
                    });
                result["host"][key] = json!(value);
                available &= value.is_some();
            }
            set_status(&mut result, "snmp_host_metrics", now, available);
        }
        if !self.config.sensors.is_empty() {
            let mut rows = Vec::new();
            let mut alerts = Vec::new();
            let mut available = true;
            for mapping in &self.config.sensors {
                let value = numeric(&mapping.oid)
                    .map(|v| v * mapping.scale + mapping.offset)
                    .filter(|v| v.is_finite());
                available &= value.is_some();
                rows.push(json!({"id":format!("snmp/{}",mapping.id),"name":mapping.name,"chip":"SNMP","kind":mapping.kind,"unit":mapping.unit,"value":value,"high":mapping.high,"crit":mapping.crit,"alarm":false,"fault":false,"source":"snmp_sensors","updated_at":value.map(|_| now),"age_s":value.map(|_| 0)}));
                let severity = if matches!((value,mapping.crit),(Some(v),Some(c)) if v>=c) {
                    Some("critical")
                } else if matches!((value,mapping.high),(Some(v),Some(h)) if v>=h) {
                    Some("warning")
                } else {
                    None
                };
                if let Some(severity) = severity {
                    alerts.push(json!({"id":format!("snmp/{}",mapping.id),"severity":severity,"message":text(&format!("{} above threshold",mapping.name),160)}));
                }
            }
            result["sensors"] = json!(rows);
            result["alerts"] = json!(alerts);
            set_status(&mut result, "snmp_sensors", now, available);
        }
        finish_snapshot(result, &self.descriptor)
    }
}

fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
fn set_status(snapshot: &mut Value, name: &str, now: f64, available: bool) {
    snapshot["sources"][name] = source_status(
        now,
        if available {
            None
        } else {
            Some("Some SNMP values unavailable")
        },
        true,
    );
}

impl Collector for SnmpCollector {
    fn descriptor(&self) -> Value {
        self.descriptor.clone()
    }
    fn interval_s(&self) -> f64 {
        self.config.interval_s
    }
    fn sample(&mut self, now: f64, mono: f64) -> Result<Value> {
        self.sequence = self.sequence.wrapping_add(1);
        match self.read() {
            Ok(values) => self.render(&values, now, mono),
            Err(_) => {
                self.rates.clear();
                self.last_uptime = None;
                let mut result = empty_snapshot(&self.descriptor, self.sequence, now);
                result["platform"] = json!({"os":self.config.platform,"transport":"snmp","interfaces":[],"uptime_scope":null});
                result["host"]["os"] = json!(self.config.platform);
                result["sources"]["snmp"] =
                    source_status(now, Some("SNMP telemetry unavailable"), true);
                finish_snapshot(result, &self.descriptor)
            }
        }
    }
}
