use anyhow::{Context, Result, bail};
use cccc_contracts::{DaemonAddress, Transport, utc_now};
use cccc_core::HomeLayout;
use cccc_core::fs::write_json;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

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
    let mut lifecycle = DaemonLifecycle::new(paths, lock);
    std::fs::write(&lifecycle.paths.pid, format!("{}\n", std::process::id()))?;
    crate::ops::runtime_restore::restore_running(&lifecycle.paths.home)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let dispatch_lock: DispatchLock = Default::default();

    let result = if use_tcp() {
        serve_tcp(&lifecycle.paths, shutdown_tx, shutdown_rx, dispatch_lock).await
    } else {
        serve_platform_default(&lifecycle.paths, shutdown_tx, shutdown_rx, dispatch_lock).await
    };
    lifecycle.finish(result)
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
    let mut automation = tokio::time::interval(Duration::from_secs(5));
    automation.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut unread_cadence = UnreadCadence::new();
    let mut connections = ConnectionTasks::default();
    let signal = shutdown_signal();
    tokio::pin!(signal);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                connections.push(spawn_connection(stream, paths.home.clone(), shutdown_tx.clone(), dispatch_lock.clone()));
            }
            changed = shutdown_rx.changed() => {
                changed?;
                if *shutdown_rx.borrow() { break; }
            }
            signal = &mut signal => {
                signal?;
                break;
            }
            _ = automation.tick() => {
                let _guard = dispatch_lock.lock().await;
                tick_automation(&paths.home, unread_cadence.take_due());
            },
        }
    }
    connections.finish().await;
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
    let mut automation = tokio::time::interval(Duration::from_secs(5));
    automation.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut unread_cadence = UnreadCadence::new();
    let mut connections = ConnectionTasks::default();
    let signal = shutdown_signal();
    tokio::pin!(signal);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                connections.push(spawn_connection(stream, paths.home.clone(), shutdown_tx.clone(), dispatch_lock.clone()));
            }
            changed = shutdown_rx.changed() => {
                changed?;
                if *shutdown_rx.borrow() { break; }
            }
            signal = &mut signal => {
                signal?;
                break;
            }
            _ = automation.tick() => {
                let _guard = dispatch_lock.lock().await;
                tick_automation(&paths.home, unread_cadence.take_due());
            },
        }
    }
    connections.finish().await;
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

#[derive(Default)]
struct ConnectionTasks(Vec<JoinHandle<()>>);

impl ConnectionTasks {
    fn push(&mut self, task: JoinHandle<()>) {
        self.0.retain(|task| !task.is_finished());
        self.0.push(task);
    }

    async fn finish(&mut self) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while let Some(mut task) = self.0.pop() {
            if tokio::time::timeout_at(deadline, &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
                break;
            }
        }
        self.abort_all();
        for task in self.0.drain(..) {
            let _ = task.await;
        }
    }

    fn abort_all(&self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

impl Drop for ConnectionTasks {
    fn drop(&mut self) {
        self.abort_all();
    }
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

struct DaemonLifecycle {
    paths: DaemonPaths,
    lock: Option<File>,
    active: bool,
}

impl DaemonLifecycle {
    fn new(paths: DaemonPaths, lock: File) -> Self {
        Self {
            paths,
            lock: Some(lock),
            active: true,
        }
    }

    fn finish(&mut self, result: Result<()>) -> Result<()> {
        let stop_result = self.cleanup();
        if let Err(error) = stop_result {
            if result.is_ok() {
                return Err(error.into());
            }
            tracing::warn!(%error, "failed to stop every runtime during daemon shutdown");
        }
        result
    }

    fn cleanup(&mut self) -> Result<Vec<cccc_runtime::SessionStatus>, cccc_runtime::RuntimeError> {
        if !self.active {
            return Ok(Vec::new());
        }
        self.active = false;
        crate::ops::actor_delivery::shutdown_all();
        let result = cccc_runtime::stop_all();
        cleanup_stale(&self.paths);
        self.lock.take();
        result
    }
}

impl Drop for DaemonLifecycle {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            tracing::warn!(%error, "failed to stop every runtime during cancelled daemon shutdown");
        }
    }
}

fn use_tcp() -> bool {
    cfg!(not(unix)) || std::env::var("CCCC_DAEMON_TRANSPORT").is_ok_and(|value| value == "tcp")
}

fn tick_automation(home: &HomeLayout, include_unread: bool) {
    crate::ops::automation_runtime::tick(home, include_unread);
}

struct UnreadCadence {
    last_tick: Instant,
}

impl UnreadCadence {
    const INTERVAL: Duration = Duration::from_secs(60);

    fn new() -> Self {
        Self {
            last_tick: Instant::now(),
        }
    }

    fn take_due(&mut self) -> bool {
        if self.last_tick.elapsed() < Self::INTERVAL {
            return false;
        }
        self.last_tick = Instant::now();
        true
    }
}

#[cfg(test)]
mod cadence_tests {
    use super::*;

    #[test]
    fn unread_work_runs_at_most_once_per_interval() {
        let mut cadence = UnreadCadence {
            last_tick: Instant::now() - UnreadCadence::INTERVAL,
        };
        assert!(cadence.take_due());
        assert!(!cadence.take_due());
    }
}
