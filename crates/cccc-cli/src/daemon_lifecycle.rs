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
// A missed readiness probe is retried; only the shared startup deadline ends the wait.
pub(crate) const DAEMON_READINESS_POLL_TIMEOUT: Duration = Duration::from_secs(2);
// Long enough for a daemon that is exiting to drop the lock, or one that is
// starting to answer, short enough that a lock held by a daemon that does
// neither still reports its own error fast.
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
    let answer = match probe_existing(client).await {
        Some(response) => Some(response),
        None => answer_or_release(home, client).await?,
    };
    match answer {
        Some(response) if is_compatible(&response) => {
            warn_version_difference(&response);
            Ok(DaemonSlot::Adopted {
                pid: response_pid(&response),
            })
        }
        Some(_) => {
            replace(client, home).await?;
            Ok(DaemonSlot::Vacant)
        }
        None => Ok(DaemonSlot::Vacant),
    }
}

/// A daemon holds the lock before it can answer and after it stops, so one
/// unanswered ping says neither. An answer or a free lock decides; `None` means
/// the lock is free, by itself or by a takeover.
async fn answer_or_release(
    home: &HomeLayout,
    client: &DaemonClient,
) -> Result<Option<cccc_contracts::DaemonResponse>> {
    let deadline = tokio::time::Instant::now() + DAEMON_LOCK_HANDOFF_TIMEOUT;
    loop {
        if daemon_takeover::probe_lock(home)? == LockState::Free {
            return Ok(None);
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            daemon_takeover::confirm_and_take_over(home).await?;
            return Ok(None);
        }
        if let Ok(Ok(response)) =
            tokio::time::timeout(remaining, call(client, "ping", json!({}))).await
        {
            return Ok(Some(response));
        }
        tokio::time::sleep(daemon_takeover::LOCK_POLL_INTERVAL).await;
    }
}

async fn probe_existing(client: &DaemonClient) -> Option<cccc_contracts::DaemonResponse> {
    let probe = call(client, "ping", json!({}));
    tokio::pin!(probe);
    match tokio::time::timeout(DAEMON_SLOW_PROBE_NOTICE, &mut probe).await {
        Ok(result) => result.ok(),
        Err(_) => {
            cccc_runtime::diagnostic(format_args!(
                "[cccc] Waiting for the CCCC daemon that already holds the lock..."
            ));
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

/// Different version numbers identify different builds, not whether an
/// operation is supported. Missing or malformed version data stays unknown.
fn different_daemon_version(response: &cccc_contracts::DaemonResponse) -> Option<&str> {
    if !response.ok {
        return None;
    }
    let version = response.result.get("version")?.as_str()?.trim();
    if version.len() > 128 || semver::Version::parse(version).is_err() {
        return None;
    }
    (version != crate::PRODUCT_VERSION).then_some(version)
}

fn warn_version_difference(response: &cccc_contracts::DaemonResponse) {
    if let Some(version) = different_daemon_version(response) {
        cccc_runtime::diagnostic(format_args!(
            "[cccc] This CCCC is version {}, but the running daemon is version {version}; some features may not work. When current work is done, run cccc daemon stop and start CCCC again from this installation.",
            crate::PRODUCT_VERSION,
        ));
    }
}

#[cfg(any(windows, test))]
pub(crate) async fn startup_ping(client: &DaemonClient, deadline: tokio::time::Instant) -> bool {
    let now = tokio::time::Instant::now();
    if now >= deadline {
        return false;
    }
    let probe_deadline = deadline.min(now + DAEMON_READINESS_POLL_TIMEOUT);
    let ready = tokio::time::timeout_at(probe_deadline, ping(client))
        .await
        .unwrap_or(false);
    ready && tokio::time::Instant::now() < probe_deadline
}

#[cfg(any(windows, test))]
pub(crate) async fn wait_until_ready(
    client: &DaemonClient,
    deadline: tokio::time::Instant,
) -> bool {
    loop {
        if startup_ping(client, deadline).await {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep_until(
            deadline.min(tokio::time::Instant::now() + Duration::from_millis(100)),
        )
        .await;
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
                cccc_runtime::diagnostic(format_args!(
                    "warning: cannot read the CCCC daemon lock: {error:#}"
                ));
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

/// What `cccc daemon stop` was asked to do with running agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopIntent {
    StopAgents,
    KeepAgents,
}

impl StopIntent {
    pub(crate) fn operation(self) -> &'static str {
        match self {
            Self::StopAgents => "shutdown",
            Self::KeepAgents => "shutdown_preserving_agents",
        }
    }
}

/// The daemon's final account of a stop request, read after it released the lock.
#[derive(Debug)]
pub(crate) enum Receipt {
    /// The daemon predates exit receipts: nothing is known beyond today's reply.
    NotSupported,
    /// The daemon writes receipts, but none matches this request: nothing is confirmed.
    Missing,
    Confirmed(cccc_daemon::session_owner::ShutdownReport),
    /// No daemon was running; agents an earlier daemon left running were stopped directly.
    WithoutDaemon(cccc_daemon::session_owner::ShutdownReport),
}

/// No daemon holds the lock. Agents an earlier daemon kept running under their session
/// owner are stopped directly, under the home lock, without starting a daemon. With none
/// running, the answer is the one from before the split.
async fn stop_without_daemon(
    client: &DaemonClient,
    home: &HomeLayout,
) -> Result<(cccc_contracts::DaemonResponse, Receipt)> {
    let Some(report) = cccc_daemon::session_owner::stop_without_daemon(home.clone()).await? else {
        // A ping has no side effect; its error is today's "not running" answer.
        call(client, "ping", json!({})).await?;
        bail!("a CCCC daemon started while this stop was running; run `cccc daemon stop` again");
    };
    let mut result = serde_json::Map::new();
    result.insert("shutting_down".into(), false.into());
    result.insert("daemon_running".into(), false.into());
    Ok((
        cccc_contracts::DaemonResponse::success(result),
        Receipt::WithoutDaemon(report),
    ))
}

/// Stop for the user command. The request ID ties the receipt to this request, so an
/// earlier exit's receipt is never read as this one's. An old daemon ignores the ID on a
/// plain stop; it answers `unknown_op` to the separate keep-agents operation, which is
/// then refused here instead of falling back to a stop that would end every agent.
pub(crate) async fn stop_for_user(
    client: &DaemonClient,
    home: &HomeLayout,
    intent: StopIntent,
) -> Result<(cccc_contracts::DaemonResponse, Receipt)> {
    let pid = daemon_pid(client).await;
    // Agents outlive their daemon only under a session owner.
    if cccc_daemon::session_owner::platform_supported()
        && pid.is_none()
        && intent == StopIntent::StopAgents
        // A lock that cannot even be opened (a home never started) has no agents behind
        // it; today's "not running" answer below stays the reply.
        && matches!(daemon_takeover::probe_lock(home), Ok(LockState::Free))
    {
        return stop_without_daemon(client, home).await;
    }
    let request_id = uuid::Uuid::new_v4().to_string();
    let args = match (intent, pid) {
        (StopIntent::KeepAgents, Some(pid)) => {
            json!({"request_id": request_id, "expected_pid": pid})
        }
        _ => json!({"request_id": request_id}),
    };
    let response = call(client, intent.operation(), args).await?;
    if !response.ok {
        if intent == StopIntent::KeepAgents {
            match response.error.as_ref().map(|error| error.code.as_str()) {
                Some("unknown_op") => bail!(
                    "the running CCCC daemon cannot keep agents across a restart; nothing was stopped. \
                     Update CCCC first, or stop the daemon and its agents with `cccc daemon stop`"
                ),
                Some("unsupported_platform") => bail!(
                    "CCCC cannot keep agents across a restart on this platform; nothing was stopped. \
                     Stop the daemon and its agents with `cccc daemon stop`"
                ),
                _ => {}
            }
        }
        return Ok((response, Receipt::NotSupported));
    }
    // Only a daemon that writes receipts names the mode it accepted. Without session owners
    // a receipt would list none of the agents the daemon stopped itself.
    let writes_receipts =
        cccc_daemon::session_owner::platform_supported() && response.result.contains_key("mode");
    daemon_takeover::wait_for_lock_release(home, DAEMON_SHUTDOWN_TIMEOUT).await?;
    let receipt = match pid {
        _ if !writes_receipts => Receipt::NotSupported,
        None => Receipt::Missing,
        Some(pid) => {
            match cccc_daemon::session_owner::read_shutdown_report(home, pid, Some(&request_id)) {
                Ok(Some(report)) => Receipt::Confirmed(report),
                Ok(None) => Receipt::Missing,
                Err(error) => {
                    cccc_runtime::diagnostic(format_args!(
                        "warning: cannot read the CCCC exit receipt: {error}"
                    ));
                    Receipt::Missing
                }
            }
        }
    };
    Ok((response, receipt))
}

/// One line for the person who ran the stop, or `None` when there is nothing beyond
/// today's reply to say. Only confirmed sets are counted; anything unconfirmed is named
/// as possibly still running, never as stopped.
pub(crate) fn receipt_line(
    home: &HomeLayout,
    intent: StopIntent,
    receipt: &Receipt,
) -> Option<String> {
    use cccc_daemon::session_owner::ShutdownMode;
    let (report, prefix) = match receipt {
        Receipt::NotSupported => return None,
        Receipt::Missing => {
            return Some(match intent {
                StopIntent::KeepAgents => "[cccc] CCCC stopped, but it could not confirm which agents kept running. \
                     Start CCCC again to reattach any that are, or stop them with `cccc daemon stop`."
                    .into(),
                StopIntent::StopAgents => "[cccc] CCCC stopped, but it could not confirm that every agent stopped; \
                     some may still be running. Run `cccc daemon stop` again to stop them."
                    .into(),
            });
        }
        Receipt::Confirmed(report) => (report, "[cccc] CCCC stopped"),
        Receipt::WithoutDaemon(report) => (report, "[cccc] CCCC was not running"),
    };
    let mut parts = Vec::new();
    if report.mode == ShutdownMode::PreserveAgents && !report.retained.is_empty() {
        parts.push(format!(
            "kept {} running ({}); start CCCC again to reattach them",
            agent_count(report.retained.len()),
            describe_agents(home, &report.retained)
        ));
    }
    // A maintenance stop can still stop an agent it could not keep; say so in either mode.
    if !report.confirmed_stopped.is_empty() {
        parts.push(format!(
            "stopped {} ({})",
            agent_count(report.confirmed_stopped.len()),
            describe_agents(home, &report.confirmed_stopped)
        ));
    }
    // Zero confirmed is not zero running: say "none" only when nothing is uncertain either.
    if report.mode == ShutdownMode::PreserveAgents
        && report.retained.is_empty()
        && report.confirmed_stopped.is_empty()
        && report.unconfirmed.is_empty()
    {
        parts.push("no agents were running".into());
    }
    // A plain stop never retains on purpose; anything left is reported, not hidden.
    let mut uncertain = report.unconfirmed.clone();
    if report.mode == ShutdownMode::Stop {
        uncertain.extend(report.retained.iter().cloned());
    }
    if !uncertain.is_empty() {
        // A plain stop is worth running again: once the daemon is gone, it reaches a live
        // session owner directly and can confirm the agent it still holds. The owner's
        // review command lists only records the owner gave up on, so it comes second.
        // After a kept stop, another plain stop would also stop the agents just kept.
        let owners = review_owners(&uncertain);
        let next = if report.mode == ShutdownMode::Stop && !owners.is_empty() {
            format!(
                "; run `cccc daemon stop` again to retry; if it still cannot confirm them, run {}",
                review_commands(&owners)
            )
        } else if !owners.is_empty() {
            format!("; to review, run {}", review_commands(&owners))
        } else if report.mode == ShutdownMode::Stop && matches!(receipt, Receipt::Confirmed(_)) {
            "; run `cccc daemon stop` again to retry".to_owned()
        } else {
            String::new()
        };
        parts.push(format!(
            "could not confirm {} ({}), which may still be running{next}",
            agent_count(uncertain.len()),
            describe_agents(home, &uncertain)
        ));
    }
    if parts.is_empty() && matches!(receipt, Receipt::WithoutDaemon(_)) {
        return Some(format!("{prefix}. Finished stopping retained resources."));
    }
    (!parts.is_empty()).then(|| format!("{prefix}: {}.", parts.join("; ")))
}

/// The session owners kept because they hold an agent they could not confirm stopped, as
/// simple UUIDs. Only a value that parses as one goes into a command the person may run.
pub(crate) fn review_owners(agents: &[cccc_daemon::session_owner::AgentIdentity]) -> Vec<String> {
    agents
        .iter()
        .filter_map(|agent| agent.owner_id.as_deref())
        .filter_map(|id| uuid::Uuid::try_parse(id).ok())
        .map(|id| id.simple().to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn review_commands(owners: &[String]) -> String {
    owners
        .iter()
        .map(|owner| {
            format!("`cccc daemon process-recovery --owner session --owner-id {owner} list`")
        })
        .collect::<Vec<_>>()
        .join(" and ")
}

/// The receipt as counts for the terminal interface's exit summary. A plain stop never
/// keeps agents on purpose, so anything it left is counted as uncertain, not kept.
pub(crate) fn agents_result(receipt: &Receipt) -> crate::host_state::AgentsResult {
    use crate::host_state::AgentsResult;
    use cccc_daemon::session_owner::ShutdownMode;
    match receipt {
        Receipt::NotSupported | Receipt::Missing => AgentsResult::Unconfirmed,
        Receipt::Confirmed(report) | Receipt::WithoutDaemon(report) => {
            let plain = report.mode == ShutdownMode::Stop;
            let mut uncertain = report.unconfirmed.clone();
            if plain {
                uncertain.extend(report.retained.iter().cloned());
            }
            AgentsResult::Confirmed {
                plain,
                stopped: report.confirmed_stopped.len(),
                kept: if plain { 0 } else { report.retained.len() },
                uncertain: uncertain.len(),
                review_owners: review_owners(&uncertain),
            }
        }
    }
}

/// Startup discovers retained resources. A browser page can still be waiting for Web.
pub(crate) async fn retained_line(client: &DaemonClient, home: &HomeLayout) -> Option<String> {
    let response = call(client, "ping", json!({})).await.ok()?;
    let agents: Vec<cccc_daemon::session_owner::AgentIdentity> =
        serde_json::from_value(response.result.get("retained_agents")?.clone()).ok()?;
    (!agents.is_empty()).then(|| {
        format!(
            "[cccc] Found {} that kept running ({}).",
            agent_count(agents.len()),
            describe_agents(home, &agents)
        )
    })
}

fn agent_count(count: usize) -> String {
    if count == 1 {
        "1 agent".into()
    } else {
        format!("{count} agents")
    }
}

/// Agent IDs grouped by Group title, falling back to the Group ID when the title cannot be read.
fn describe_agents(
    home: &HomeLayout,
    agents: &[cccc_daemon::session_owner::AgentIdentity],
) -> String {
    let store = cccc_core::GroupStore::new(home.clone()).ok();
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
    for agent in agents {
        match groups
            .iter_mut()
            .find(|(group, _)| *group == agent.group_id)
        {
            Some((_, actors)) => actors.push(&agent.actor_id),
            None => groups.push((agent.group_id.clone(), vec![&agent.actor_id])),
        }
    }
    groups
        .into_iter()
        .map(|(group_id, actors)| {
            let title = store
                .as_ref()
                .and_then(|store| store.load(&group_id).ok())
                .map(|group| group.title)
                .filter(|title| !title.trim().is_empty())
                .unwrap_or(group_id);
            format!("{} in {title}", actors.join(", "))
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Stop a daemon that answered but cannot serve this build.
async fn replace(client: &DaemonClient, home: &HomeLayout) -> Result<()> {
    cccc_runtime::diagnostic(format_args!(
        "Replacing a legacy or incompatible CCCC daemon..."
    ));
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
        warn_version_difference(&response);
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
    cccc_runtime::diagnostic(format_args!("{}", ownership_line(owned, pid)));
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
