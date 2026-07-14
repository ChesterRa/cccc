use anyhow::{Context, Result, bail};
use cccc_contracts::{DaemonAddress, Transport, utc_now};
use cccc_core::HomeLayout;
use cccc_core::fs::write_json;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use tokio::net::TcpListener;
use tokio::sync::watch;

#[cfg(unix)]
use tokio::net::UnixListener;

use crate::paths::DaemonPaths;
use crate::server_connection::{DispatchLock, spawn_connection};

pub async fn run(home: HomeLayout) -> Result<()> {
    home.initialize().context("initialize Rust home")?;
    let paths = DaemonPaths::new(home);
    std::fs::create_dir_all(&paths.daemon_dir)?;
    let lock = acquire_daemon_lock(&paths.lock)?;
    cleanup_stale(&paths);
    std::fs::write(&paths.pid, format!("{}\n", std::process::id()))?;
    crate::ops::runtime_restore::restore_running(&paths.home)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let dispatch_lock: DispatchLock = Default::default();

    let result = if use_tcp() {
        serve_tcp(&paths, shutdown_tx, shutdown_rx, dispatch_lock).await
    } else {
        serve_platform_default(&paths, shutdown_tx, shutdown_rx, dispatch_lock).await
    };
    let stop_result = cccc_runtime::stop_all();
    cleanup_stale(&paths);
    drop(lock);
    if let Err(error) = stop_result {
        if result.is_ok() {
            return Err(error.into());
        }
        tracing::warn!(%error, "failed to stop every runtime during daemon shutdown");
    }
    result
}

async fn serve_tcp(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
    dispatch_lock: DispatchLock,
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
    let mut automation = tokio::time::interval(std::time::Duration::from_secs(5));
    automation.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                spawn_connection(stream, paths.home.clone(), shutdown_tx.clone(), dispatch_lock.clone());
            }
            changed = shutdown_rx.changed() => {
                changed?;
                if *shutdown_rx.borrow() { break; }
            }
            _ = automation.tick() => {
                let _guard = dispatch_lock.lock().await;
                tick_automation(&paths.home);
            },
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn serve_platform_default(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
    dispatch_lock: DispatchLock,
) -> Result<()> {
    let listener = UnixListener::bind(&paths.socket)?;
    write_address(
        paths,
        Transport::Unix,
        &paths.socket.to_string_lossy(),
        String::new(),
        0,
    )?;
    let mut automation = tokio::time::interval(std::time::Duration::from_secs(5));
    automation.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                spawn_connection(stream, paths.home.clone(), shutdown_tx.clone(), dispatch_lock.clone());
            }
            changed = shutdown_rx.changed() => {
                changed?;
                if *shutdown_rx.borrow() { break; }
            }
            _ = automation.tick() => {
                let _guard = dispatch_lock.lock().await;
                tick_automation(&paths.home);
            },
        }
    }
    Ok(())
}

#[cfg(not(unix))]
async fn serve_platform_default(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    dispatch_lock: DispatchLock,
) -> Result<()> {
    serve_tcp(paths, shutdown_tx, shutdown_rx, dispatch_lock).await
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

fn cleanup_stale(paths: &DaemonPaths) {
    for path in [&paths.socket, &paths.address, &paths.pid] {
        if let Err(error) = std::fs::remove_file(path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %path.display(), %error, "failed to remove daemon state");
        }
    }
}

fn use_tcp() -> bool {
    cfg!(not(unix)) || std::env::var("CCCC_DAEMON_TRANSPORT").is_ok_and(|value| value == "tcp")
}

fn tick_automation(home: &HomeLayout) {
    crate::ops::automation_runtime::tick(home);
}
