use anyhow::{Context, Result, bail};
use cccc_contracts::{DaemonAddress, Transport, utc_now};
use cccc_core::HomeLayout;
use cccc_core::fs::write_json;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::net::IpAddr;
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

type RuntimeRestoreSpawner = fn(HomeLayout, DispatchLocks);

pub async fn run(home: HomeLayout) -> Result<()> {
    run_with_restore(home, crate::ops::runtime_restore::spawn).await
}

async fn run_with_restore(home: HomeLayout, restore: RuntimeRestoreSpawner) -> Result<()> {
    crate::process_tree::protect_daemon_host().context("protect daemon process tree")?;
    home.initialize().context("initialize Rust home")?;
    let paths = DaemonPaths::new(home);
    std::fs::create_dir_all(&paths.daemon_dir)?;
    let lock = acquire_daemon_lock(&paths.lock)?;
    crate::runtime_start_gate::allow(&paths.home).map_err(anyhow::Error::msg)?;
    cleanup_stale(&paths);
    let mut lifecycle = DaemonLifecycle::new(paths, lock);
    std::fs::write(&lifecycle.paths.pid, format!("{}\n", std::process::id()))?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let actor_activity = ActorActivityService::start(lifecycle.paths.home.clone());
    let dispatch_locks = DispatchLocks::default();
    let group_bridge_sessions =
        crate::group_bridge_sessions::SessionManager::start(lifecycle.paths.home.clone());

    let result = if use_tcp() {
        serve_tcp(
            &lifecycle.paths,
            shutdown_tx,
            shutdown_rx,
            dispatch_locks,
            restore,
        )
        .await
    } else {
        serve_platform_default(
            &lifecycle.paths,
            shutdown_tx,
            shutdown_rx,
            dispatch_locks,
            restore,
        )
        .await
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
    restore: RuntimeRestoreSpawner,
) -> Result<()> {
    let host =
        daemon_tcp_host(&std::env::var("CCCC_DAEMON_HOST").unwrap_or_else(|_| "127.0.0.1".into()))?;
    let port = std::env::var("CCCC_DAEMON_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let listener = TcpListener::bind((host.as_str(), port)).await?;
    let local = listener.local_addr()?;
    publish_address_and_restore(
        paths,
        Transport::Tcp,
        "",
        local.ip().to_string(),
        local.port(),
        dispatch_locks.clone(),
        restore,
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

fn daemon_tcp_host(value: &str) -> Result<String> {
    let value = value.trim();
    let normalized = if value.is_empty() || value.eq_ignore_ascii_case("localhost") {
        "127.0.0.1"
    } else {
        value.trim_matches(['[', ']'])
    };
    let address = normalized.parse::<IpAddr>().with_context(|| {
        format!("CCCC_DAEMON_HOST must be a loopback IP address, got {value:?}")
    })?;
    if !address.is_loopback() {
        bail!(
            "refusing unauthenticated daemon IPC binding on non-loopback address {address}; use a Unix socket or a loopback TCP address"
        );
    }
    Ok(address.to_string())
}

#[cfg(unix)]
async fn serve_platform_default(
    paths: &DaemonPaths,
    shutdown_tx: watch::Sender<bool>,
    mut shutdown_rx: watch::Receiver<bool>,
    dispatch_locks: DispatchLocks,
    restore: RuntimeRestoreSpawner,
) -> Result<()> {
    let listener = UnixListener::bind(&paths.socket)?;
    publish_address_and_restore(
        paths,
        Transport::Unix,
        &paths.socket.to_string_lossy(),
        String::new(),
        0,
        dispatch_locks.clone(),
        restore,
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
    restore: RuntimeRestoreSpawner,
) -> Result<()> {
    serve_tcp(paths, shutdown_tx, shutdown_rx, dispatch_locks, restore).await
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

fn publish_address_and_restore(
    paths: &DaemonPaths,
    transport: Transport,
    path: &str,
    host: String,
    port: u16,
    dispatch_locks: DispatchLocks,
    restore: RuntimeRestoreSpawner,
) -> Result<()> {
    // Claims from the previous process must be settled before publishing an
    // address that lets this process accept new claims. Runtime recreation can
    // remain asynchronous once that ownership boundary is established.
    crate::ops::runtime_restore::settle_stranded(&paths.home)
        .map_err(|error| anyhow::anyhow!(error.message.clone()))?;
    write_address(paths, transport, path, host, port)?;
    restore(paths.home.clone(), dispatch_locks);
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
    #[cfg(windows)]
    {
        use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, ctrl_logoff, ctrl_shutdown};
        let mut interrupt = ctrl_c()?;
        let mut r#break = ctrl_break()?;
        let mut close = ctrl_close()?;
        let mut logoff = ctrl_logoff()?;
        let mut shutdown = ctrl_shutdown()?;
        tokio::select! {
            _ = interrupt.recv() => {},
            _ = r#break.recv() => {},
            _ = close.recv() => {},
            _ = logoff.recv() => {},
            _ = shutdown.recv() => {},
        }
    }
    #[cfg(not(any(unix, windows)))]
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

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::Actor;
    use cccc_core::{GroupStore, ledger};

    fn assert_address_is_published(home: HomeLayout, _locks: DispatchLocks) {
        let paths = DaemonPaths::new(home);
        assert!(paths.address.exists());
        std::fs::write(paths.daemon_dir.join("restore.started"), b"").expect("mark restore start");
    }

    #[test]
    fn reclaims_an_unlocked_stale_daemon_lock_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("ccccd.lock");
        std::fs::write(&path, "stale pid\n").expect("write stale lock");

        let lock = acquire_daemon_lock(&path).expect("claim stale lock");

        assert!(path.exists());
        drop(lock);
        acquire_daemon_lock(&path).expect("reclaim released lock");
    }

    #[test]
    fn daemon_tcp_binding_is_loopback_only() {
        assert_eq!(daemon_tcp_host("").expect("default"), "127.0.0.1");
        assert_eq!(
            daemon_tcp_host("localhost").expect("localhost"),
            "127.0.0.1"
        );
        assert_eq!(daemon_tcp_host("::1").expect("IPv6 loopback"), "::1");
        for host in ["0.0.0.0", "192.168.1.10", "::", "daemon.internal"] {
            assert!(daemon_tcp_host(host).is_err(), "{host} must be rejected");
        }
    }

    #[test]
    fn publishes_ipc_address_before_starting_runtime_restore() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize home");
        let paths = DaemonPaths::new(home.clone());

        publish_address_and_restore(
            &paths,
            Transport::Tcp,
            "",
            "127.0.0.1".into(),
            4242,
            DispatchLocks::default(),
            assert_address_is_published,
        )
        .expect("publish daemon address");

        assert!(paths.daemon_dir.join("restore.started").exists());
    }

    #[test]
    fn settles_prior_process_claims_before_publishing_ipc() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut group = store.create("stranded claim", "").expect("group");
        let actor = Actor::new("peer1");
        group.actors.push(actor.clone());
        store.save(&group).expect("save actor");
        crate::ops::runtime_delivery::claim(&home, &group, &actor, "source-1", "pty", false)
            .expect("claim");
        let paths = DaemonPaths::new(home);

        publish_address_and_restore(
            &paths,
            Transport::Tcp,
            "",
            "127.0.0.1".into(),
            4242,
            DispatchLocks::default(),
            assert_address_is_published,
        )
        .expect("publish daemon address");

        let states = ledger::read_all(&store.ledger_path(&group.group_id).expect("ledger"))
            .expect("read ledger")
            .into_iter()
            .filter(|event| event.kind == "runtime.delivery")
            .filter_map(|event| event.data["state"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(states, ["claimed", "ambiguous"]);
    }
}
