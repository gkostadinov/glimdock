//! One process for collection, node configuration, and the embedded web console.
//! Linux and macOS use a private Unix socket; remote Windows devices run agents.
use crate::{
    config::{self, Config},
    runtime, server,
};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::Serialize;
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::SocketAddr,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    sync::{oneshot, watch},
    task::JoinSet,
};

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub state_dir: PathBuf,
    pub config: Option<PathBuf>,
    pub address: SocketAddr,
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
pub struct Ready {
    pub pid: u32,
    pub address: SocketAddr,
    pub url: String,
    pub config: PathBuf,
    pub snapshot: PathBuf,
    pub config_socket: PathBuf,
}

pub struct State {
    pub directory: PathBuf,
    pub config: PathBuf,
    pub snapshot: PathBuf,
    pub display_token: PathBuf,
    pub setup_token: PathBuf,
    _lock: File,
}

fn private_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        bail!("Hub state must be an owned directory, not a symbolic link");
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn private_existing_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_file()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.permissions().mode() & 0o077 != 0
            {
                bail!(
                    "Private hub file must be owned, regular, and inaccessible to other users: {}",
                    path.display()
                );
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn create_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("Hub file requires a parent directory")?;
    let mut pending = tempfile::NamedTempFile::new_in(parent)?;
    pending
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending
        .persist_noclobber(path)
        .map_err(|_| anyhow::anyhow!("Private hub file creation failed"))?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn initialize_token(path: &Path, setup: bool) -> Result<()> {
    if !private_existing_file(path)? {
        let mut random = [0u8; 32];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let token = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        create_private_file(path, format!("{token}\n").as_bytes())?;
    }
    server::load_token(Some(path), setup)?.context("Hub token cannot be empty")?;
    Ok(())
}

impl State {
    /// Initialization never replaces saved configuration or credential bytes.
    pub fn open(directory: &Path, explicit_config: Option<&Path>) -> Result<Self> {
        private_directory(directory)?;
        let directory = fs::canonicalize(directory)?;
        let lock = OpenOptions::new()
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(directory.join(".hub.lock"))?;
        lock.try_lock_exclusive()
            .context("Another hub is already running in this state directory")?;
        let config = explicit_config
            .map(|p| {
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    std::env::current_dir().unwrap_or_default().join(p)
                }
            })
            .unwrap_or_else(|| directory.join("config.json"));
        let display_token = directory.join("display.token");
        let setup_token = directory.join("setup.token");
        if !private_existing_file(&config)? {
            if fs::symlink_metadata(&display_token).is_ok()
                || fs::symlink_metadata(&setup_token).is_ok()
            {
                bail!(
                    "Saved hub configuration is missing; restore it or use a fresh state directory"
                );
            }
            if let Some(parent) = config.parent() {
                fs::create_dir_all(parent)?;
            }
            let fresh = json!({"enable_local":true,"local_type":"server","node":config::hostname(),
                "host_ip":"127.0.0.1","interval_s":3.0,"printers":[],"remote_collectors":[]});
            Config::from_value(fresh.clone())?;
            create_private_file(&config, &serde_json::to_vec_pretty(&fresh)?)?;
        }
        Config::read(&config)?;
        initialize_token(&display_token, false)?;
        initialize_token(&setup_token, true)?;
        let display = server::load_token(Some(&display_token), false)?.unwrap();
        let setup = server::load_token(Some(&setup_token), true)?.unwrap();
        if display == setup {
            bail!("Setup and display tokens must differ");
        }
        Ok(Self {
            snapshot: directory.join("snapshot.json"),
            directory,
            config,
            display_token,
            setup_token,
            _lock: lock,
        })
    }
}

struct RecordCleanup(PathBuf);
impl Drop for RecordCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub struct Hub {
    state: State,
    socket_directory: tempfile::TempDir,
    configuration: server::ConfigurationService,
    http: server::HttpServer,
    ready: Ready,
}

impl Hub {
    /// All immutable startup checks and listener binds happen before workers.
    pub async fn prepare(options: RunOptions) -> Result<Self> {
        if options.cert.is_some() != options.key.is_some() {
            bail!("Certificate and key must be supplied together");
        }
        let state = State::open(&options.state_dir, options.config.as_deref())?;
        // Keep sun_path within macOS's short bound even for a long state path.
        let socket_directory = tempfile::Builder::new().prefix("gdh-").tempdir_in("/tmp")?;
        private_directory(socket_directory.path())?;
        let socket = socket_directory.path().join("config.sock");
        let configuration =
            server::ConfigurationService::bind(state.config.clone(), &socket, None, false)?;
        let display = server::load_token(Some(&state.display_token), false)?.unwrap();
        let setup = server::load_token(Some(&state.setup_token), true)?;
        let http_state = server::HttpState::new(
            server::SnapshotStore::new(state.snapshot.clone(), false),
            display,
            setup,
            socket.clone(),
        )?
        .with_local_console();
        let http = server::HttpServer::bind(
            options.address,
            http_state,
            options.cert.as_deref(),
            options.key.as_deref(),
        )
        .await?;
        let address = http.local_addr()?;
        let url_address = if address.ip().is_unspecified() {
            SocketAddr::new(
                if address.is_ipv6() {
                    "::1".parse()?
                } else {
                    "127.0.0.1".parse()?
                },
                address.port(),
            )
        } else {
            address
        };
        let ready = Ready {
            pid: std::process::id(),
            address,
            url: format!(
                "{}://{url_address}/",
                if options.cert.is_some() {
                    "https"
                } else {
                    "http"
                }
            ),
            config: state.config.clone(),
            snapshot: state.snapshot.clone(),
            config_socket: socket,
        };
        Ok(Self {
            state,
            socket_directory,
            configuration,
            http,
            ready,
        })
    }

    pub async fn run(
        self,
        mut shutdown: watch::Receiver<bool>,
        ready: Option<oneshot::Sender<Ready>>,
    ) -> Result<()> {
        let Self {
            state,
            socket_directory,
            configuration,
            http,
            ready: details,
        } = self;
        let config = Config::read(&state.config)?;
        let (stop, workers_stop) = watch::channel(false);
        let mut workers = JoinSet::new();
        workers.spawn(configuration.run(workers_stop.clone()));
        let (first_sent, first) = oneshot::channel();
        let output = state.snapshot.clone();
        let config_path = state.config.clone();
        let collector_stop = workers_stop.clone();
        workers.spawn(async move {
            runtime::run_controlled(
                config,
                &output,
                false,
                None,
                Some(&config_path),
                collector_stop,
                Some(first_sent),
            )
            .await
        });
        let startup = tokio::select! {
            first = tokio::time::timeout(Duration::from_secs(30), first) => match first {
                Ok(Ok(())) => Ok(true),
                _ => Err(anyhow::anyhow!("Collector could not publish its first snapshot")),
            },
            failure = workers.join_next() => worker_result(failure).map(|_| false),
            _ = runtime::wait_for_shutdown(&mut shutdown) => Ok(false),
        };
        let record = state.directory.join("run.json");
        let _cleanup = RecordCleanup(record.clone());
        let outcome = match startup {
            Ok(true) => {
                match config::atomic_write(&record, &serde_json::to_vec_pretty(&details)?, 0o600) {
                    Ok(()) => {
                        workers.spawn(http.run(workers_stop));
                        println!("Glimdock collector and web console: {}", details.url);
                        println!("Configuration: {}", details.config.display());
                        println!("Device display key: {}", state.display_token.display());
                        println!("Management key: {}", state.setup_token.display());
                        if let Some(ready) = ready {
                            let _ = ready.send(details);
                        }
                        tokio::select! {
                            failure = workers.join_next() => worker_result(failure),
                            _ = runtime::wait_for_shutdown(&mut shutdown) => Ok(()),
                        }
                    }
                    Err(error) => Err(error),
                }
            }
            Ok(false) => Ok(()),
            Err(error) => Err(error),
        };
        let _ = stop.send(true);
        if tokio::time::timeout(Duration::from_secs(6), async {
            while workers.join_next().await.is_some() {}
        })
        .await
        .is_err()
        {
            workers.abort_all();
            while workers.join_next().await.is_some() {}
        }
        drop(socket_directory);
        outcome
    }
}

fn worker_result(
    result: Option<std::result::Result<Result<()>, tokio::task::JoinError>>,
) -> Result<()> {
    match result {
        Some(Ok(Err(error))) => Err(error),
        Some(Err(_)) => bail!("A hub worker failed"),
        _ => bail!("A hub worker stopped unexpectedly"),
    }
}

pub async fn run(options: RunOptions) -> Result<()> {
    let hub = Hub::prepare(options).await?;
    let (stop, shutdown) = watch::channel(false);
    let work = hub.run(shutdown, None);
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => result,
        _ = runtime::shutdown_signal() => {
            let _ = stop.send(true);
            work.await
        }
    }
}
