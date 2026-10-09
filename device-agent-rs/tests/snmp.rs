use glimdock_agent::{
    snmp::{
        bounded_command, credential_config, interface_oids, oid, parse_values, requested_oids,
        storage_oids, SnmpCollector, SnmpConfig, SnmpRequest, BATCH_SIZE, HOST_UPTIME, INTERFACE,
        MAX_OUTPUT, SYS_UPTIME,
    },
    Collector,
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    net::UdpSocket,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

type RecordedCalls = Arc<Mutex<Vec<(Vec<String>, PathBuf)>>>;

struct Fixture {
    _directory: tempfile::TempDir,
    config: SnmpConfig,
    values: Arc<Mutex<BTreeMap<String, Value>>>,
    calls: RecordedCalls,
    fail: Arc<AtomicBool>,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let credentials = directory.path().join("credentials.json");
        fs::write(
            &credentials,
            r#"{"version":"2c","community":"private-snmp-community"}"#,
        )
        .unwrap();
        let config: SnmpConfig = serde_json::from_value(json!({"id":"router","name":"Gateway","address":"192.0.2.1","credential_file":credentials,
            "interfaces":[{"id":"wan","index":1}],"storage":[{"id":"flash","name":"Flash","index":2}],"memory_index":3,
            "cpu_oids":["1.3.6.1.2.1.25.3.3.1.2.1",".1.3.6.1.2.1.25.3.3.1.2.2"],
            "sensors":[{"id":"board","name":"Board","oid":".1.3.6.1.4.1.99999.1.0","scale":0.1,"offset":-2,"kind":"temperature","unit":"°C","high":60,"crit":80}]})).unwrap();
        config.validate().unwrap();
        let mut values = BTreeMap::from([
            (SYS_UPTIME.into(), json!(123456)),
            (HOST_UPTIME.into(), json!(234567)),
            (oid(&config.cpu_oids[0]).unwrap(), json!(20)),
            (config.cpu_oids[1].clone(), json!(40)),
            (config.sensors[0].oid.clone(), json!(700)),
        ]);
        for (key, value) in storage_oids(2).into_iter().zip([4096, 100, 50]) {
            values.insert(key, json!(value));
        }
        for (key, value) in storage_oids(3).into_iter().zip([1024, 1000, 400]) {
            values.insert(key, json!(value));
        }
        for key in interface_oids(&config.interfaces[0]).into_values() {
            values.insert(key, json!(0));
        }
        let ids = interface_oids(&config.interfaces[0]);
        values.insert(ids["rx"].clone(), json!(1u64 << 63));
        values.insert(ids["tx"].clone(), json!(1000));
        values.insert(ids["status"].clone(), json!(1));
        values.insert(ids["speed_mbps"].clone(), json!(1000));
        Self {
            _directory: directory,
            config,
            values: Arc::new(Mutex::new(values)),
            calls: Arc::new(Mutex::new(vec![])),
            fail: Arc::new(AtomicBool::new(false)),
        }
    }
    fn collector(&self) -> SnmpCollector {
        let values = self.values.clone();
        let calls = self.calls.clone();
        let fail = self.fail.clone();
        SnmpCollector::with_runner(self.config.clone(), move |request: &SnmpRequest| {
            if fail.load(Ordering::Acquire) {
                anyhow::bail!("private-snmp-community");
            }
            let directory = PathBuf::from(&request.env["SNMPCONFPATH"]);
            assert!(fs::read_to_string(directory.join("snmp.conf"))
                .unwrap()
                .contains("defCommunity private-snmp-community"));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(directory.join("snmp.conf"))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
                assert_eq!(
                    fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
            assert_eq!(request.args[..2], ["-Cf", "-OnqteU"]);
            assert_eq!(request.args[4..6], ["-r", "0"]);
            assert_eq!(request.max_output, MAX_OUTPUT);
            assert_eq!(request.timeout, Duration::from_secs(3));
            assert!(!format!("{:?}", request.env).contains("private-snmp-community"));
            assert!(!format!("{:?}", request.args).contains("private-snmp-community"));
            calls
                .lock()
                .unwrap()
                .push((request.args.clone(), directory));
            let values = values.lock().unwrap();
            Ok(request
                .args
                .iter()
                .skip(7)
                .map(|key| {
                    format!(
                        "{key} {}",
                        values.get(key).map_or(
                            "No Such Object available on this agent".to_string(),
                            Value::to_string
                        )
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"))
        })
        .unwrap()
    }
}

#[test]
fn router_vendor_metrics_and_counter64_deltas_preserve_precision() {
    let fixture = Fixture::new();
    let mut collector = fixture.collector();
    let first = collector.sample(1000., 20.).unwrap();
    assert_eq!(first["node"]["platform"], "router");
    assert_eq!(first["node"]["type"], "server");
    assert_eq!(first["host"]["uptime_s"], 2345.67);
    assert_eq!(first["platform"]["uptime_scope"], "host");
    assert_eq!(first["host"]["cpu_pct"], 30.);
    assert_eq!(first["host"]["mem_used_bytes"], 409600.);
    assert_eq!(first["host"]["mem_total_bytes"], 1024000.);
    assert_eq!(first["storage"][0]["used_bytes"], 204800.);
    assert_eq!(first["sensors"][0]["value"], 68.);
    assert_eq!(first["sensors"][0]["unit"], "°C");
    assert_eq!(first["alerts"][0]["severity"], "warning");
    assert!(first["host"]["net_rx_bps"].is_null());
    let ids = interface_oids(&fixture.config.interfaces[0]);
    {
        let mut values = fixture.values.lock().unwrap();
        values.insert(ids["rx"].clone(), json!((1u64 << 63) + 30));
        values.insert(ids["tx"].clone(), json!(1060));
    }
    let second = collector.sample(1003., 23.).unwrap();
    assert_eq!(second["host"]["net_rx_bps"], 10.);
    assert_eq!(second["host"]["net_tx_bps"], 20.);
    assert_eq!(second["limits"]["network_interfaces"], json!(["wan"]));
    assert_eq!(second["platform"]["interfaces"][0]["status"], "up");
    assert!(!second.to_string().contains("private-snmp-community"));
    assert!(!second.to_string().contains("credentials.json"));
    assert!(fixture
        .calls
        .lock()
        .unwrap()
        .iter()
        .all(|(_, directory)| !directory.exists()));
}

#[test]
fn failure_recovery_reboots_and_resets_clear_rate_history() {
    let fixture = Fixture::new();
    let mut collector = fixture.collector();
    collector.sample(1000., 20.).unwrap();
    fixture.fail.store(true, Ordering::Release);
    let failed = collector.sample(1003., 23.).unwrap();
    assert_eq!(failed["node"]["status"], "offline");
    assert!(failed["host"]["cpu_pct"].is_null());
    assert_eq!(failed["sensors"], json!([]));
    assert_eq!(failed["storage"], json!([]));
    assert!(!failed.to_string().contains("private-snmp-community"));
    fixture.fail.store(false, Ordering::Release);
    let ids = interface_oids(&fixture.config.interfaces[0]);
    fixture
        .values
        .lock()
        .unwrap()
        .insert(ids["rx"].clone(), json!((1u64 << 63) + 60));
    assert!(collector.sample(1006., 26.).unwrap()["host"]["net_rx_bps"].is_null());
    {
        let mut values = fixture.values.lock().unwrap();
        values.insert(HOST_UPTIME.into(), json!(10));
        values.insert(ids["rx"].clone(), json!(100));
    }
    assert!(collector.sample(1009., 29.).unwrap()["host"]["net_rx_bps"].is_null());
    fixture
        .values
        .lock()
        .unwrap()
        .insert(ids["rx"].clone(), json!(50));
    assert!(collector.sample(1012., 32.).unwrap()["host"]["net_rx_bps"].is_null());
}

#[test]
fn unsupported_oids_preserve_supported_platform_data() {
    let fixture = Fixture::new();
    {
        let mut values = fixture.values.lock().unwrap();
        values.remove(HOST_UPTIME);
        values.remove(&fixture.config.sensors[0].oid);
    }
    let snapshot = fixture.collector().sample(1000., 20.).unwrap();
    assert_eq!(snapshot["host"]["uptime_s"], 1234.56);
    assert_eq!(snapshot["platform"]["uptime_scope"], "snmp-agent");
    assert!(snapshot["sensors"][0]["value"].is_null());
    assert!(snapshot["sensors"][0]["updated_at"].is_null());
    assert_eq!(snapshot["host"]["cpu_pct"], 30.);
    assert_eq!(snapshot["node"]["status"], "degraded");
}

#[test]
fn numeric_partial_response_without_standard_uptime_is_degraded() {
    let fixture = Fixture::new();
    fixture
        .values
        .lock()
        .unwrap()
        .retain(|key, _| key == &oid(&fixture.config.cpu_oids[0]).unwrap());
    let snapshot = fixture.collector().sample(1000., 20.).unwrap();
    assert_eq!(snapshot["node"]["status"], "degraded");
    assert_eq!(snapshot["sources"]["snmp"]["updated_at"], 1000.);
    assert_eq!(snapshot["host"]["cpu_cores"], json!([20.0, null]));
    assert!(snapshot["host"]["cpu_pct"].is_null());
}

#[test]
fn vendor_host_fields_scale_values_and_reject_out_of_range_percentage() {
    let mut fixture = Fixture::new();
    fixture.config.host_metrics = serde_json::from_value(json!({"mem_total_bytes":{"oid":".1.3.6.1.4.1.99999.2.0","scale":1024},"cpu_pct":{"oid":".1.3.6.1.4.1.99999.3.0"}})).unwrap();
    {
        let mut values = fixture.values.lock().unwrap();
        values.insert(".1.3.6.1.4.1.99999.2.0".into(), json!(5000));
        values.insert(".1.3.6.1.4.1.99999.3.0".into(), json!(101));
    }
    let snapshot = fixture.collector().sample(1000., 20.).unwrap();
    assert_eq!(snapshot["host"]["mem_total_bytes"], 5120000.);
    assert!(snapshot["host"]["cpu_pct"].is_null());
}

#[test]
fn ipv6_and_counter32_mapping_and_fixed_batches() {
    let mut fixture = Fixture::new();
    fixture.config.address = "2001:db8::1".into();
    fixture.config.interfaces[0].counter_bits = 32;
    fixture.config.interfaces[0].index = 5;
    let ids = interface_oids(&fixture.config.interfaces[0]);
    assert_eq!(ids["rx"], format!("{INTERFACE}.10.5"));
    assert_eq!(ids["tx"], format!("{INTERFACE}.16.5"));
    fixture.config.sensors = serde_json::from_value(json!((0..32)
        .map(|i| json!({"id":format!("sensor{i}"),"oid":format!(".1.3.6.1.4.1.99999.10.{i}")}))
        .collect::<Vec<_>>()))
    .unwrap();
    fixture.collector().sample(1000., 20.).unwrap();
    let calls = fixture.calls.lock().unwrap();
    assert!(calls.len() > 1);
    let mut requested = Vec::new();
    for (args, _) in calls.iter() {
        assert!(args.contains(&"udp6:[2001:db8::1]:161".to_string()));
        assert!(args.len() - 7 <= BATCH_SIZE);
        requested.extend_from_slice(&args[7..]);
    }
    assert_eq!(requested, requested_oids(&fixture.config).unwrap());
}

#[test]
fn config_rejects_commands_invalid_oids_duplicates_and_injection() {
    let fixture = Fixture::new();
    let base = serde_json::to_value(&fixture.config).unwrap();
    for update in [
        json!({"address":"-c secret"}),
        json!({"address":"router; reboot"}),
        json!({"address":"http://router"}),
        json!({"port":true}),
        json!({"timeout_s":"2"}),
        json!({"credential_file":"relative"}),
        json!({"cpu_oids":["sysUpTime.0"]}),
        json!({"memory_index":-1}),
        json!({"command":"snmpset"}),
        json!({"sensors":[{"id":"bad","oid":".1.3.6.1","scale":"2"}]}),
        json!({"interfaces":[{"id":"wan","index":1},{"id":"wan2","index":1}]}),
    ] {
        let mut value = base.clone();
        for (key, item) in update.as_object().unwrap() {
            value[key] = item.clone();
        }
        assert!(
            serde_json::from_value::<SnmpConfig>(value)
                .and_then(|c| c.validate().map_err(serde::de::Error::custom))
                .is_err(),
            "{update}"
        );
    }
    for value in [
        "1.40.1",
        "1.3.4294967296",
        "1.3.6 --write",
        "-x",
        "",
        "1..3",
        "3.1.1",
    ] {
        assert!(oid(value).is_err());
    }
}

#[test]
fn authenticated_v3_credentials_and_config_injection() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("credentials.json");
    let good = json!({"version":"3","username":"monitor","auth_password":"private-auth-password","priv_password":"private-privacy-password"});
    fs::write(&path, good.to_string()).unwrap();
    let config = credential_config(&path).unwrap();
    assert!(config.contains("defSecurityLevel authPriv"));
    assert!(config.contains("defAuthType SHA"));
    assert!(config.contains("defPrivType AES"));
    for update in [
        json!({"security_level":"noAuthNoPriv"}),
        json!({"auth_password":"short"}),
        json!({"auth_password":"password\ndefCommunity public"}),
        json!({"username":"user#comment"}),
        json!({"auth_password":"password\"quote"}),
        json!({"priv_protocol":"DES"}),
        json!({"version":"1"}),
    ] {
        let mut bad = good.clone();
        for (key, value) in update.as_object().unwrap() {
            bad[key] = value.clone();
        }
        fs::write(&path, bad.to_string()).unwrap();
        assert!(credential_config(&path).is_err(), "{update}");
    }
    fs::write(
        &path,
        r#"{"version":"2c","community":"first","community":"second"}"#,
    )
    .unwrap();
    assert!(credential_config(&path).is_err());
}

#[test]
fn response_rejects_unknown_duplicate_oversized_and_keeps_counter64_exact() {
    let expected = vec![SYS_UPTIME.into()];
    for raw in [
        format!("{SYS_UPTIME} 1\n{SYS_UPTIME} 2"),
        ".1.3.6.1.2.1.1.4.0 1".into(),
        "not a response".into(),
        "x".repeat(MAX_OUTPUT + 1),
    ] {
        assert!(parse_values(&raw, &expected).is_err());
    }
    assert_eq!(
        parse_values(&format!("{SYS_UPTIME} 9223372036854775809"), &expected).unwrap()[SYS_UPTIME]
            .as_u64(),
        Some(9223372036854775809)
    );
    assert_eq!(
        parse_values(&format!("{SYS_UPTIME} 18446744073709551615"), &expected).unwrap()[SYS_UPTIME]
            .as_u64(),
        Some(u64::MAX)
    );
    assert!(
        parse_values(&format!("{SYS_UPTIME} secret-private-text"), &expected).unwrap()[SYS_UPTIME]
            .is_null()
    );
}

// Spawn this native Rust test executable as the process fixture. No shell or
// interpreter is involved, including the timeout and malicious-output checks.
#[test]
fn native_process_fixture() {
    match std::env::var("GLIMDOCK_SNMP_PROCESS_FIXTURE").as_deref() {
        Ok("ok") => {
            println!("fixture-ok");
            eprintln!("private-process-secret");
        }
        Ok("overflow") => println!("{}", "x".repeat(10000)),
        Ok("timeout") => thread::sleep(Duration::from_secs(10)),
        Ok("fail") => {
            eprintln!("private-process-secret");
            std::process::exit(1);
        }
        _ => {}
    }
}

#[test]
fn subprocess_runtime_and_retained_output_are_bounded_and_stderr_is_private() {
    let program = std::env::current_exe().unwrap();
    let args = vec![
        "--exact".into(),
        "native_process_fixture".into(),
        "--nocapture".into(),
    ];
    let run = |mode: &str, timeout, limit| {
        bounded_command(
            program.as_os_str(),
            &args,
            &BTreeMap::from([("GLIMDOCK_SNMP_PROCESS_FIXTURE".into(), OsString::from(mode))]),
            timeout,
            limit,
        )
    };
    let output = run("ok", Duration::from_secs(2), 2048).unwrap();
    assert!(output.contains("fixture-ok"));
    assert!(!output.contains("private-process-secret"));
    assert!(run("overflow", Duration::from_secs(2), 2048)
        .unwrap_err()
        .to_string()
        .contains("capacity"));
    assert!(run("timeout", Duration::from_millis(50), 2048)
        .unwrap_err()
        .to_string()
        .contains("timed out"));
    assert_eq!(
        run("fail", Duration::from_secs(2), 2048)
            .unwrap_err()
            .to_string(),
        "SNMP request failed"
    );
}

fn tlv(tag: u8, data: &[u8]) -> Vec<u8> {
    let mut result = vec![tag];
    if data.len() < 128 {
        result.push(data.len() as u8);
    } else {
        result.extend_from_slice(&[0x82, (data.len() >> 8) as u8, data.len() as u8]);
    }
    result.extend_from_slice(data);
    result
}
fn read_tlv(data: &[u8], offset: &mut usize) -> (u8, Vec<u8>) {
    let tag = data[*offset];
    let mut size = data[*offset + 1] as usize;
    *offset += 2;
    if size & 128 != 0 {
        let count = size & 127;
        size = 0;
        for _ in 0..count {
            size = (size << 8) | data[*offset] as usize;
            *offset += 1;
        }
    }
    let value = data[*offset..*offset + size].to_vec();
    *offset += size;
    (tag, value)
}
fn decode_oid(raw: &[u8]) -> String {
    let first = raw[0] as u32;
    let mut arcs = vec![first / 40, first % 40];
    let mut value = 0u32;
    for part in &raw[1..] {
        value = (value << 7) | (part & 127) as u32;
        if part & 128 == 0 {
            arcs.push(value);
            value = 0;
        }
    }
    format!(
        ".{}",
        arcs.iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(".")
    )
}

#[test]
fn real_snmpget_reads_private_credentials_and_numeric_udp_peer() {
    if std::process::Command::new("snmpget")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("Native Net-SNMP CLI unavailable; fixture integration skipped");
        return;
    }
    let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
    peer.set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let port = peer.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let communities = Arc::new(Mutex::new(Vec::new()));
    let received = communities.clone();
    let sensor = ".1.3.6.1.4.1.99999.1.0";
    let server = thread::spawn(move || {
        let values = BTreeMap::from([(SYS_UPTIME, 12345u32), (HOST_UPTIME, 67890), (sensor, 705)]);
        let mut buffer = [0u8; 65535];
        while !thread_stop.load(Ordering::Acquire) {
            let Ok((count, remote)) = peer.recv_from(&mut buffer) else {
                continue;
            };
            let (_, message) = read_tlv(&buffer[..count], &mut 0);
            let mut pos = 0;
            let (_, version) = read_tlv(&message, &mut pos);
            let (_, community) = read_tlv(&message, &mut pos);
            received
                .lock()
                .unwrap()
                .push(String::from_utf8(community.clone()).unwrap());
            let (tag, pdu) = read_tlv(&message, &mut pos);
            assert_eq!(tag, 0xa0);
            let mut pos = 0;
            let (_, identity) = read_tlv(&pdu, &mut pos);
            read_tlv(&pdu, &mut pos);
            read_tlv(&pdu, &mut pos);
            let (_, bindings) = read_tlv(&pdu, &mut pos);
            let mut rows = Vec::new();
            let mut pos = 0;
            while pos < bindings.len() {
                let (_, binding) = read_tlv(&bindings, &mut pos);
                let (_, encoded_oid) = read_tlv(&binding, &mut 0);
                let key = decode_oid(&encoded_oid);
                let encoded = values.get(key.as_str()).map_or_else(
                    || tlv(0x80, &[]),
                    |value| {
                        tlv(
                            if key == SYS_UPTIME || key == HOST_UPTIME {
                                0x43
                            } else {
                                0x02
                            },
                            &value.to_be_bytes(),
                        )
                    },
                );
                let mut row = tlv(0x06, &encoded_oid);
                row.extend(encoded);
                rows.extend(tlv(0x30, &row));
            }
            let mut response = tlv(0x02, &identity);
            response.extend(tlv(0x02, &[0]));
            response.extend(tlv(0x02, &[0]));
            response.extend(tlv(0x30, &rows));
            let mut message = tlv(0x02, &version);
            message.extend(tlv(0x04, &community));
            message.extend(tlv(0xa2, &response));
            peer.send_to(&tlv(0x30, &message), remote).unwrap();
        }
    });
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("credentials.json");
    fs::write(
        &path,
        r#"{"version":"2c","community":"fixture-private-community"}"#,
    )
    .unwrap();
    let config=serde_json::from_value(json!({"address":"127.0.0.1","port":port,"credential_file":path,"sensors":[{"id":"temperature","oid":sensor,"kind":"temperature","unit":"°C","scale":0.1}]})).unwrap();
    let snapshot = SnmpCollector::new(config)
        .unwrap()
        .sample(1000., 20.)
        .unwrap();
    stop.store(true, Ordering::Release);
    server.join().unwrap();
    assert_eq!(
        *communities.lock().unwrap(),
        vec!["fixture-private-community"]
    );
    assert_eq!(snapshot["node"]["status"], "healthy");
    assert_eq!(snapshot["host"]["uptime_s"], 678.9);
    assert_eq!(snapshot["sensors"][0]["value"], 70.5);
    assert!(!snapshot.to_string().contains("fixture-private-community"));
}

#[cfg(unix)]
#[test]
fn real_authenticated_v3_reads_isolated_native_snmpd() {
    use std::process::{Child, Command, Stdio};
    struct Daemon(Child);
    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    if Command::new("snmpd")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
        || Command::new("snmpget")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
    {
        eprintln!("Native snmpd/snmpget unavailable; SNMPv3 integration skipped");
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let reserve = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = reserve.local_addr().unwrap().port();
    drop(reserve);
    let config = directory.path().join("snmpd.conf");
    fs::write(&config,"createUser fixture SHA fixture-auth-password AES fixture-privacy-password\nrouser fixture priv .1.3.6.1.2.1\nsysName isolated-fixture\n").unwrap();
    let child = Command::new("snmpd")
        .args(["-f", "-Ln", "-C", "-r", "-c"])
        .arg(&config)
        .arg("-p")
        .arg(directory.path().join("snmpd.pid"))
        .arg(format!("udp:127.0.0.1:{port}"))
        .env("MIBS", "")
        .env("SNMPCONFPATH", directory.path())
        .env("SNMP_PERSISTENT_DIR", directory.path())
        .env(
            "SNMP_PERSISTENT_FILE",
            directory.path().join("snmpd.persistent.conf"),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut daemon = Daemon(child);
    let credentials = directory.path().join("credentials.json");
    fs::write(&credentials,json!({"version":"3","username":"fixture","auth_password":"fixture-auth-password","priv_password":"fixture-privacy-password"}).to_string()).unwrap();
    let mut collector=SnmpCollector::new(serde_json::from_value(json!({"address":"127.0.0.1","port":port,"credential_file":credentials,"timeout_s":0.5})).unwrap()).unwrap();
    let mut snapshot = Value::Null;
    for _ in 0..4 {
        thread::sleep(Duration::from_millis(50));
        assert!(
            daemon.0.try_wait().unwrap().is_none(),
            "Isolated native snmpd exited before SNMPv3 validation"
        );
        snapshot = collector.sample(1000., 20.).unwrap();
        if snapshot["host"]["uptime_s"].is_number() {
            break;
        }
    }
    assert_eq!(snapshot["node"]["status"], "healthy", "{snapshot}");
    assert!(snapshot["host"]["uptime_s"].is_number());
    assert!(!snapshot.to_string().contains("fixture-auth-password"));
    assert!(!snapshot.to_string().contains("fixture-privacy-password"));
}
