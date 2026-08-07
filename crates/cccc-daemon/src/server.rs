use anyhow::{Context, Result, bail};
use cccc_contracts::{DaemonAddress, Transport, utc_now};
use cccc_core::HomeLayout;
use cccc_core::fs::write_json;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::watch;

#[cfg(unix)]
use tokio::net::UnixListener;

use crate::dispatch_concurrency::DispatchLocks;
use crate::paths::DaemonPaths;
use crate::server_actor_activity::ActorActivityService;
use crate::server_automation::AutomationScheduler;
use crate::server_connection::spawn_connection;
use crate::server_connections::ConnectionTasks;
use crate::server_lifecycle::{DaemonLifecycle, cleanup_stale};

pub async fn run(home: HomeLayout) -> Result<()> {
    home.initialize().context("initialize Rust home")?;
    let paths = DaemonPaths::new(home);
    std::fs::create_dir_all(&paths.daemon_dir)?;
    let lock = acquire_daemon_lock(&paths.lock)?;
    crate::runtime_start_gate::allow(&paths.home).map_err(anyhow::Error::msg)?;
    cleanup_stale(&paths);
    let mut lifecycle = DaemonLifecycle::new(paths, lock);
    std::fs::write(&lifecycle.paths.pid, format!("{}\n", std::process::id()))?;
    crate::ops::runtime_restore::restore_running(&lifecycle.paths.home)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let actor_activity = ActorActivityService::start(lifecycle.paths.home.clone());
    let dispatch_locks = DispatchLocks::default();
    let group_bridge_sessions =
        crate::group_bridge_sessions::SessionManager::start(lifecycle.paths.home.clone());

    let result = if use_tcp() {
        serve_tcp(&lifecycle.paths, shutdown_tx, shutdown_rx, dispatch_locks).await
    } else {
        serve_platform_default(&lifecycle.paths, shutdown_tx, shutdown_rx, dispatch_locks).await
    };
    actor_activity.finish().await;
    group_bridge_sessions.shutdown().await;
    lifecycle.finish(result)
}

async fn serve_tcp(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
    dispatch_locks: DispatchLocks,
) -> Result<()> {
    let host = std::env::var("CCCC_DAEMON_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port = std::env::var("CCCC_DAEMON_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    let local = listener.local_addr()?;
    write_address(
        paths,
        Transport::Tcp,
        "",
        local.ip().to_string(),
        local.port(),
    )?;
    let mut automation_interval = tokio::time::interval(Duration::from_secs(5));
    automation_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut automation = AutomationScheduler::new();
    let mut connections = ConnectionTasks::default();
    let signal = shutdown_signal();
    tokio::pin!(signal);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                connections.push(spawn_connection(stream, paths.home.clone(), shutdown_tx.clone(), dispatch_locks.clone()));
            }
            changed = shutdown_rx.changed() => {
                changed?;
                if *shutdown_rx.borrow() { break; }
            }
            signal = &mut signal => {
                signal?;
                break;
            }
            _ = automation_interval.tick() => {
                automation.trigger(paths.home.clone(), dispatch_locks.clone());
            },
        }
    }
    begin_runtime_shutdown(&paths.home);
    automation.finish().await;
    connections.finish().await;
    Ok(())
}

#[cfg(unix)]
async fn serve_platform_default(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
    dispatch_locks: DispatchLocks,
) -> Result<()> {
    let listener = UnixListener::bind(&paths.socket)?;
    write_address(
        paths,
        Transport::Unix,
        &paths.socket.to_string_lossy(),
        String::new(),
        0,
    )?;
    let mut automation_interval = tokio::time::interval(Duration::from_secs(5));
    automation_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut automation = AutomationScheduler::new();
    let mut connections = ConnectionTasks::default();
    let signal = shutdown_signal();
    tokio::pin!(signal);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                connections.push(spawn_connection(stream, paths.home.clone(), shutdown_tx.clone(), dispatch_locks.clone()));
            }
            changed = shutdown_rx.changed() => {
                changed?;
                if *shutdown_rx.borrow() { break; }
            }
            signal = &mut signal => {
                signal?;
                break;
            }
            _ = automation_interval.tick() => {
                automation.trigger(paths.home.clone(), dispatch_locks.clone());
            },
        }
    }
    begin_runtime_shutdown(&paths.home);
    automation.finish().await;
    connections.finish().await;
    Ok(())
}

#[cfg(not(unix))]
async fn serve_platform_default(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    dispatch_locks: DispatchLocks,
) -> Result<()> {
    serve_tcp(paths, shutdown_tx, shutdown_rx, dispatch_locks).await
}

fn acquire_daemon_lock(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)?;
    if file.try_lock_exclusive().is_err() {
        bail!("another Rust daemon already owns {}", path.display());
    }
    Ok(file)
}

fn write_address(
    paths: &DaemonPaths,
    transport: Transport,
    path: &str,
    host: String,
    port: u16,
) -> Result<()> {
    write_json(
        &paths.address,
        &DaemonAddress {
            v: 1,
            transport,
            path: path.into(),
            host,
            port,
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").into(),
            ts: utc_now(),
        },
    )?;
    Ok(())
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await?;
    Ok(())
}

fn use_tcp() -> bool {
    cfg!(not(unix)) || std::env::var("CCCC_DAEMON_TRANSPORT").is_ok_and(|value| value == "tcp")
}

fn begin_runtime_shutdown(home: &HomeLayout) {
    let _ = crate::runtime_start_gate::prevent(home);
    crate::ops::actor_runtime::cancel_resume_verifications();
}
