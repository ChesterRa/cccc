use anyhow::{Context, Result, bail};
use cccc_contracts::{DaemonAddress, DaemonRequest, DaemonResponse, Transport, utc_now};
use cccc_core::HomeLayout;
use cccc_core::fs::write_json;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::watch;

#[cfg(unix)]
use tokio::net::UnixListener;

use crate::dispatch::dispatch;
use crate::paths::DaemonPaths;

const MAX_REQUEST_BYTES: usize = 2_000_000;

pub async fn run(home: HomeLayout) -> Result<()> {
    home.initialize().context("initialize Rust home")?;
    let paths = DaemonPaths::new(home);
    std::fs::create_dir_all(&paths.daemon_dir)?;
    let lock = acquire_daemon_lock(&paths.lock)?;
    cleanup_stale(&paths);
    std::fs::write(&paths.pid, format!("{}\n", std::process::id()))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let result = if use_tcp() {
        serve_tcp(&paths, shutdown_tx, shutdown_rx).await
    } else {
        serve_platform_default(&paths, shutdown_tx, shutdown_rx).await
    };
    cleanup_stale(&paths);
    drop(lock);
    result
}

async fn serve_tcp(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
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
                handle_stream(stream, paths.home.clone(), &shutdown_tx).await?;
            }
            changed = shutdown_rx.changed() => {
                changed?;
                if *shutdown_rx.borrow() { break; }
            }
            _ = automation.tick() => tick_automation(&paths.home),
        }
    }
    Ok(())
}

#[cfg(unix)]
async fn serve_platform_default(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
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
                handle_stream(stream, paths.home.clone(), &shutdown_tx).await?;
            }
            changed = shutdown_rx.changed() => {
                changed?;
                if *shutdown_rx.borrow() { break; }
            }
            _ = automation.tick() => tick_automation(&paths.home),
        }
    }
    Ok(())
}

#[cfg(not(unix))]
async fn serve_platform_default(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    serve_tcp(paths, shutdown_tx, shutdown_rx).await
}

async fn handle_stream<S>(stream: S, home: HomeLayout, shutdown: &watch::Sender<bool>) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut line = String::new();
    BufReader::new(read).read_line(&mut line).await?;
    let response = if line.len() > MAX_REQUEST_BYTES {
        DaemonResponse::failure("request_too_large", "request exceeds 2 MB")
    } else {
        match serde_json::from_str::<DaemonRequest>(&line) {
            Ok(request) => {
                let should_shutdown = request.op == "shutdown";
                let response = dispatch(&home, &request);
                if should_shutdown && response.ok {
                    shutdown.send(true).ok();
                }
                response
            }
            Err(error) => DaemonResponse::failure("invalid_request", error.to_string()),
        }
    };
    let mut payload = serde_json::to_vec(&response)?;
    payload.push(b'\n');
    write.write_all(&payload).await?;
    write.shutdown().await?;
    Ok(())
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
    if let Err(error) = crate::ops::actor_runtime::reconcile(home) {
        tracing::warn!(message = %error.message, "runtime reconciliation failed");
    }
    if let Err(error) = cccc_core::automation::tick(home) {
        tracing::warn!(%error, "automation tick failed");
    }
}
