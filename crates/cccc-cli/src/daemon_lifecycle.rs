//! Daemon discovery, compatibility, readiness, liveness, and shutdown policy.

use anyhow::{Result, bail};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::json;
use std::time::Duration;

use crate::commands::common::call;
use crate::daemon_takeover::{self, LockState};

// Stopping a daemon means stopping every runtime under it, which on a populated
// home is measured in seconds, not milliseconds. Cutting this short does not
// make shutdown faster -- it just moves the same work to the forced path, which
// is the one that leaves processes behind.
pub(crate) const DAEMON_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);
// Long enough for a daemon that is already exiting to drop the lock, short
// enough that a lock held by a live daemon still reports its own error fast.
const DAEMON_LOCK_HANDOFF_TIMEOUT: Duration = Duration::from_secs(5);
#[cfg(any(not(windows), test))]
const DAEMON_OWNER_HANDOFF_GRACE: Duration = Duration::from_secs(2);
// How long an already-running daemon is given to answer before this process
// says out loud that it is waiting. It governs when the operator is told and
// then asked -- never whether a daemon is stopped.
const DAEMON_SLOW_PROBE_NOTICE: Duration = Duration::from_secs(3);
// How often the daemon lock is re-read while serving Web. Only the polling
// rate: the lock itself says whether the daemon is alive, so a slow poll costs
// a little delay in noticing, never a wrong answer.
const DAEMON_LIVENESS_POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DaemonSlot {
    Vacant,
    Adopted { pid: Option<u32> },
}

pub(crate) async fn resolve_slot(home: &HomeLayout, client: &DaemonClient) -> Result<DaemonSlot> {
    if daemon_takeover::probe_lock(home)? == LockState::Free {
        return Ok(DaemonSlot::Vacant);
    }
    match probe_existing(client).await {
        Some(response) if is_compatible(&response) => Ok(DaemonSlot::Adopted {
            pid: response_pid(&response),
        }),
        Some(_) => {
            replace(client, home).await?;
            Ok(DaemonSlot::Vacant)
        }
        None if daemon_takeover::wait_for_lock_release(home, DAEMON_LOCK_HANDOFF_TIMEOUT)
            .await
            .is_ok() =>
        {
            Ok(DaemonSlot::Vacant)
        }
        None => {
            daemon_takeover::confirm_and_take_over(home).await?;
            Ok(DaemonSlot::Vacant)
        }
    }
}

async fn probe_existing(client: &DaemonClient) -> Option<cccc_contracts::DaemonResponse> {
    let probe = call(client, "ping", json!({}));
    tokio::pin!(probe);
    match tokio::time::timeout(DAEMON_SLOW_PROBE_NOTICE, &mut probe).await {
        Ok(result) => result.ok(),
        Err(_) => {
            eprintln!("[cccc] Waiting for the CCCC daemon that already holds the lock...");
            probe.await.ok()
        }
    }
}

fn response_pid(response: &cccc_contracts::DaemonResponse) -> Option<u32> {
    response
        .result
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
}

/// Whether anything at all answers on the daemon socket, compatible or not.
pub(crate) async fn answers(client: &DaemonClient) -> bool {
    call(client, "ping", json!({})).await.is_ok()
}

/// Whether a compatible Rust daemon answers.
pub(crate) async fn ping(client: &DaemonClient) -> bool {
    call(client, "ping", json!({}))
        .await
        .is_ok_and(|response| is_compatible(&response))
}

pub(crate) fn is_compatible(response: &cccc_contracts::DaemonResponse) -> bool {
    response.ok
        && response
            .result
            .get("implementation")
            .and_then(|v| v.as_str())
            == Some("rust")
        && response
            .result
            .get("compatibility")
            .and_then(|v| v.as_str())
            == Some(cccc_contracts::RUST_DAEMON_COMPATIBILITY)
}

#[cfg(windows)]
pub(crate) async fn wait_until_ready(
    client: &DaemonClient,
    deadline: tokio::time::Instant,
) -> bool {
    loop {
        if ping(client).await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(any(not(windows), test))]
pub(crate) async fn wait_until_ready_or_owner_exits(
    client: &DaemonClient,
    daemon_exited: &mut tokio::sync::watch::Receiver<bool>,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut handoff_deadline = None;
    loop {
        if ping(client).await {
            return true;
        }
        if *daemon_exited.borrow() {
            handoff_deadline
                .get_or_insert_with(|| tokio::time::Instant::now() + DAEMON_OWNER_HANDOFF_GRACE);
        }
        let now = tokio::time::Instant::now();
        if now >= deadline || handoff_deadline.is_some_and(|handoff| now >= handoff) {
            return false;
        }
        tokio::select! {
            () = tokio::time::sleep(Duration::from_millis(100)) => {}
            changed = daemon_exited.changed(), if handoff_deadline.is_none() => {
                if changed.is_err() || *daemon_exited.borrow() {
                    handoff_deadline.get_or_insert_with(
                        || tokio::time::Instant::now() + DAEMON_OWNER_HANDOFF_GRACE,
                    );
                }
            }
        }
    }
}

/// Resolve once the daemon lock is no longer held by anyone.
pub(crate) async fn wait_for_loss(home: &HomeLayout) {
    let mut reported = false;
    loop {
        tokio::time::sleep(DAEMON_LIVENESS_POLL_INTERVAL).await;
        match daemon_takeover::probe_lock(home) {
            Ok(LockState::Free) => return,
            Ok(LockState::Held) => reported = false,
            Err(error) if !reported => {
                reported = true;
                eprintln!("warning: cannot read the CCCC daemon lock: {error:#}");
            }
            Err(_) => {}
        }
    }
}

pub(crate) async fn stop(
    client: &DaemonClient,
    home: &HomeLayout,
) -> Result<cccc_contracts::DaemonResponse> {
    let response = call(client, "shutdown", json!({})).await?;
    if response.ok {
        daemon_takeover::wait_for_lock_release(home, DAEMON_SHUTDOWN_TIMEOUT).await?;
    }
    Ok(response)
}

/// Stop a daemon that answered but cannot serve this build.
async fn replace(client: &DaemonClient, home: &HomeLayout) -> Result<()> {
    eprintln!("Replacing a legacy or incompatible CCCC daemon...");
    if !stop(client, home).await?.ok {
        bail!("failed to stop incompatible CCCC daemon");
    }
    Ok(())
}

pub(crate) async fn replace_incompatible(home: &HomeLayout, client: &DaemonClient) -> Result<()> {
    let Ok(response) = call(client, "ping", json!({})).await else {
        return Ok(());
    };
    if is_compatible(&response) {
        return Ok(());
    }
    replace(client, home).await
}

pub(crate) async fn announce_ownership(client: &DaemonClient, owned: bool, slot: DaemonSlot) {
    let pid = if owned {
        None
    } else {
        match slot {
            DaemonSlot::Adopted { pid } => pid,
            DaemonSlot::Vacant => daemon_pid(client).await,
        }
    };
    eprintln!("{}", ownership_line(owned, pid));
}

async fn daemon_pid(client: &DaemonClient) -> Option<u32> {
    let response = call(client, "ping", json!({})).await.ok()?;
    is_compatible(&response)
        .then(|| response_pid(&response))
        .flatten()
}

pub(crate) fn ownership_line(owned: bool, pid: Option<u32>) -> String {
    if owned {
        return "[cccc] Daemon: started by this process (stops when this process exits)".into();
    }
    pid.map_or_else(
        || "[cccc] Daemon: external (keeps running after exit; stop it with `cccc daemon stop`)".into(),
        |pid| format!("[cccc] Daemon: external pid {pid} (keeps running after exit; stop it with `cccc daemon stop`)"),
    )
}

#[cfg(test)]
#[path = "daemon_lifecycle_tests.rs"]
mod tests;
