use anyhow::{bail, Result};
use clap::{ArgGroup, Parser};
use glimdock_agent::{
    config,
    host::{HostCollector, HostConfig},
    json_device::{JsonDeviceCollector, JsonDeviceConfig},
    protocol, push,
    server::{self, ServerState},
    snmp::{SnmpCollector, SnmpConfig},
    Collector,
};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version,about="Native read-only OS, SNMP and vendor JSON telemetry for Glimdock",group(ArgGroup::new("source").args(["config","snmp_config","json_config"]).multiple(false)))]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    snmp_config: Option<PathBuf>,
    #[arg(long)]
    json_config: Option<PathBuf>,
    /// Push telemetry to the central collector without opening a listener.
    #[arg(long, requires = "state_dir", conflicts_with_all = ["serve_http", "token_file", "cert", "key", "once", "generate_token"])]
    collector_url: Option<String>,
    /// Private durable pairing and identity directory (absolute path).
    #[arg(long, requires = "collector_url")]
    state_dir: Option<PathBuf>,
    /// Single-use collector pairing key, read only during first enrollment.
    #[arg(long, requires = "collector_url")]
    enrollment_key_file: Option<PathBuf>,
    /// Pair again with a new key, retaining identity and rotating publishing access.
    #[arg(long, requires_all = ["collector_url", "enrollment_key_file"])]
    re_enroll: bool,
    /// Allow unencrypted HTTP only on a trusted network.
    #[arg(long, requires = "collector_url")]
    allow_insecure_http: bool,
    /// Private collector CA certificate; TLS verification remains enabled.
    #[arg(long, requires = "collector_url")]
    collector_ca_cert: Option<PathBuf>,
    /// Explicit legacy HTTP endpoint for collectors that still poll agents.
    #[arg(long, requires = "token_file")]
    serve_http: bool,
    #[arg(long, default_value = "127.0.0.1")]
    bind: IpAddr,
    #[arg(long, default_value_t = 8765)]
    port: u16,
    #[arg(long)]
    token_file: Option<PathBuf>,
    #[arg(long, requires = "key")]
    cert: Option<PathBuf>,
    #[arg(long, requires = "cert")]
    key: Option<PathBuf>,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    generate_token: bool,
}
fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    if let Err(error) = run(Cli::parse()) {
        eprintln!("Glimdock agent failed: {error}");
        std::process::exit(1);
    }
}
fn run(args: Cli) -> Result<()> {
    if args.generate_token {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes)
            .map_err(|_| anyhow::anyhow!("Secure randomness unavailable"))?;
        println!(
            "{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        return Ok(());
    }
    if args.collector_url.is_none() && (args.port == 0 || !args.bind.is_ipv4()) {
        bail!("Use an IPv4 listener and nonzero port");
    }
    let mut collector: Box<dyn Collector> = if let Some(path) = args.snmp_config {
        Box::new(SnmpCollector::new(config::read_config::<SnmpConfig>(
            &path,
        )?)?)
    } else if let Some(path) = args.json_config {
        Box::new(JsonDeviceCollector::new(config::read_config::<
            JsonDeviceConfig,
        >(&path)?)?)
    } else {
        Box::new(HostCollector::new(if let Some(path) = args.config {
            config::read_config::<HostConfig>(&path)?
        } else {
            HostConfig::default()
        })?)
    };
    if let Some(collector_url) = args.collector_url {
        return push::run(
            push::Options {
                collector_url,
                state_dir: args.state_dir.unwrap(),
                enrollment_key_file: args.enrollment_key_file,
                re_enroll: args.re_enroll,
                allow_insecure_http: args.allow_insecure_http,
                collector_ca_cert: args.collector_ca_cert,
            },
            collector,
        );
    }
    if !args.once && args.token_file.is_none() {
        bail!("Use --collector-url with --state-dir to push to your collector, or --serve-http with --token-file for the legacy HTTP endpoint");
    }
    let clock = Instant::now();
    let initial = collector.sample(glimdock_agent::epoch(), clock.elapsed().as_secs_f64())?;
    if args.once {
        std::thread::sleep(Duration::from_secs(1));
        let sample = collector.sample(glimdock_agent::epoch(), clock.elapsed().as_secs_f64())?;
        println!("{}", serde_json::to_string(&sample)?);
        if protocol::snapshot_status(&sample) == "offline" {
            bail!("Telemetry unavailable");
        }
        return Ok(());
    }
    let path = args
        .token_file
        .ok_or_else(|| anyhow::anyhow!("--token-file is required for HTTP service"))?;
    let token = server::load_token(&path)?;
    let interval = collector.interval_s();
    let descriptor = collector.descriptor();
    let state = ServerState::new(initial, token, interval)?;
    let stop = Arc::new(AtomicBool::new(false));
    let running = stop.clone();
    let published = state.clone();
    let worker = std::thread::spawn(move || {
        let mut next = Instant::now() + Duration::from_secs_f64(interval);
        while !running.load(Ordering::Relaxed) {
            let remaining = next.saturating_duration_since(Instant::now());
            if !remaining.is_zero() {
                std::thread::sleep(remaining.min(Duration::from_millis(100)));
                continue;
            }
            let began = Instant::now();
            let now = glimdock_agent::epoch();
            let result = collector
                .sample(now, clock.elapsed().as_secs_f64())
                .or_else(|_| {
                    let mut safe = protocol::empty_snapshot(&descriptor, 0, now);
                    safe["sources"]["collector"] =
                        protocol::source_status(now, Some("Telemetry unavailable"), true);
                    protocol::finish_snapshot(safe, &descriptor)
                });
            if let Ok(snapshot) = result {
                let _ = published.publish(snapshot);
            }
            next =
                Instant::now() + Duration::from_secs_f64(interval).saturating_sub(began.elapsed());
        }
    });
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let address = SocketAddr::new(args.bind, args.port);
        let app = server::router(state);
        let handle = axum_server::Handle::new();
        let quit = handle.clone();
        tokio::spawn(async move {
            push::shutdown_signal().await;
            quit.graceful_shutdown(Some(Duration::from_secs(5)));
        });
        if let (Some(cert), Some(key)) = (args.cert, args.key) {
            let tls = axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
            let mut listener = axum_server::bind_rustls(address, tls)
                .map(server::BoundedAcceptor::new)
                .http1_only()
                .handle(handle);
            listener
                .http_builder()
                .http1()
                .timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(Duration::from_secs(5))
                .max_buf_size(8192);
            listener.serve(app.into_make_service()).await?;
        } else {
            let mut listener = axum_server::bind(address)
                .map(server::BoundedAcceptor::new)
                .http1_only()
                .handle(handle);
            listener
                .http_builder()
                .http1()
                .timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(Duration::from_secs(5))
                .max_buf_size(8192);
            listener.serve(app.into_make_service()).await?;
        }
        Ok::<(), anyhow::Error>(())
    });
    stop.store(true, Ordering::Relaxed);
    let _ = worker.join();
    result
}
