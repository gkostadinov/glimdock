use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use glimdock_collector::{
    config::{Config, SOCKET_PATH},
    hub, runtime, server,
};
use std::{net::SocketAddr, path::PathBuf};
#[derive(Parser)]
#[command(
    name = "glimdock-collector",
    version,
    about = "Glimdock host-agnostic telemetry hub and web console"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Run the central collector, node management, and web console together (Linux/macOS).
    Run {
        /// Private saved configuration, keys, and live snapshots.
        #[arg(long, default_value = ".glimdock")]
        state_dir: PathBuf,
        /// Use an existing private configuration instead of STATE_DIR/config.json.
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
        #[arg(long, default_value_t = 8765)]
        port: u16,
        #[arg(long, requires = "key")]
        cert: Option<PathBuf>,
        #[arg(long, requires = "cert")]
        key: Option<PathBuf>,
    },
    /// Collect atomic schema2 snapshots with independent source freshness.
    Collect {
        #[arg(long, default_value = "/etc/homelab-monitor/config.json")]
        config: PathBuf,
        #[arg(long, default_value = "/run/homelab-monitor/snapshot.json")]
        output: PathBuf,
        #[arg(long)]
        snapshot_group: Option<String>,
        #[arg(long)]
        once: bool,
        /// Reload validated node edits without a service manager (useful on macOS).
        #[arg(long)]
        watch_config: bool,
    },
    /// Collect one initialized snapshot, then exit.
    Once {
        #[arg(long, default_value = "/etc/homelab-monitor/config.json")]
        config: PathBuf,
        #[arg(long, default_value = "/run/homelab-monitor/snapshot.json")]
        output: PathBuf,
        #[arg(long)]
        snapshot_group: Option<String>,
    },
    /// Serve snapshots as the unprivileged reader (display and setup roles differ).
    Serve {
        #[arg(long, default_value = "/run/homelab-monitor/snapshot.json")]
        snapshot: PathBuf,
        #[arg(long, default_value = "127.0.0.1")]
        bind: String,
        #[arg(long, default_value_t = 8765)]
        port: u16,
        #[arg(long)]
        token_file: Option<PathBuf>,
        /// Serve the same console on loopback and bridge its fixed APIs to this hub origin.
        #[arg(long)]
        upstream: Option<String>,
        #[arg(long)]
        setup_token_file: Option<PathBuf>,
        #[arg(long,default_value=SOCKET_PATH)]
        config_socket: PathBuf,
        #[arg(long)]
        demo: bool,
        #[arg(long)]
        cert: Option<PathBuf>,
        #[arg(long)]
        key: Option<PathBuf>,
    },
    /// Root-only node configuration on a private Unix socket; no VM/printer controls.
    ConfigService {
        #[arg(long, default_value = "/etc/homelab-monitor/config.json")]
        config: PathBuf,
        #[arg(long,default_value=SOCKET_PATH)]
        socket: PathBuf,
        #[arg(long, default_value = "homelab-monitor")]
        socket_group: Option<String>,
        #[arg(long)]
        no_apply: bool,
    },
    ValidateConfig {
        #[arg(long, default_value = "/etc/homelab-monitor/config.json")]
        config: PathBuf,
    },
    Version,
}
fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(12)
        .enable_all()
        .build()
        .expect("Runtime initialization failed");
    if let Err(error) = runtime.block_on(run()) {
        eprintln!("Glimdock failed: {error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    match Cli::parse().command {
        Commands::Run {
            state_dir,
            config,
            bind,
            port,
            cert,
            key,
        } => {
            let ip = if bind == "localhost" {
                "127.0.0.1"
            } else {
                &bind
            }
            .parse()?;
            hub::run(hub::RunOptions {
                state_dir,
                config,
                address: SocketAddr::new(ip, port),
                cert,
                key,
            })
            .await
        }
        Commands::Collect {
            config,
            output,
            snapshot_group,
            once,
            watch_config,
        } => {
            runtime::run(
                Config::read(&config)?,
                &output,
                once,
                snapshot_group.as_deref(),
                if watch_config { Some(&config) } else { None },
            )
            .await
        }
        Commands::Once {
            config,
            output,
            snapshot_group,
        } => {
            runtime::run(
                Config::read(&config)?,
                &output,
                true,
                snapshot_group.as_deref(),
                None,
            )
            .await
        }
        Commands::Serve {
            snapshot,
            bind,
            port,
            token_file,
            upstream,
            setup_token_file,
            config_socket,
            demo,
            cert,
            key,
        } => {
            if (demo || upstream.is_some())
                && !matches!(bind.as_str(), "127.0.0.1" | "::1" | "localhost")
            {
                bail!("Demo and credential bridge require loopback bind");
            }
            let ip = if bind == "localhost" {
                "127.0.0.1"
            } else {
                &bind
            }
            .parse()?;
            let display = server::load_token(token_file.as_deref(), false)?.unwrap();
            let setup = if demo {
                None
            } else {
                server::load_token(setup_token_file.as_deref(), true)?
            };
            let mut state = server::HttpState::new(
                server::SnapshotStore::new(snapshot, demo),
                display,
                setup,
                config_socket,
            )?;
            if let Some(origin) = upstream {
                state = state.with_upstream(&origin)?;
            }
            server::serve(
                SocketAddr::new(ip, port),
                state,
                cert.as_deref(),
                key.as_deref(),
            )
            .await
        }
        Commands::ConfigService {
            config,
            socket,
            socket_group,
            no_apply,
        } => server::config_service(config, socket, socket_group.as_deref(), !no_apply).await,
        Commands::ValidateConfig { config } => {
            let config = Config::read(&config)?;
            println!(
                "Valid configuration: {} enabled nodes",
                config.enabled_node_count()
            );
            Ok(())
        }
        Commands::Version => {
            println!("glimdock-collector {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
