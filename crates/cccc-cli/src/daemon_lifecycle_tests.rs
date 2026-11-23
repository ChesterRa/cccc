use super::{
    DaemonSlot, different_daemon_version, is_compatible, ownership_line, resolve_slot,
    wait_for_loss, wait_until_ready_or_owner_exits,
};
use crate::PRODUCT_VERSION;
use crate::daemon_takeover::{LockState, probe_lock};
use cccc_client::DaemonClient;
use cccc_contracts::DaemonResponse;
use cccc_core::HomeLayout;
use fs2::FileExt;
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::task::Poll;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;

#[path = "daemon_lifecycle_test_io.rs"]
mod observed;
use observed::{ISOLATED_DAEMON_CASE, home};

// A daemon installs a process-wide guard that intentionally lasts for the
// host's lifetime. Keep the test daemon in its own process so its guard cannot
// outlive the fixture home in the test runner.
async fn run_daemon_case_in_child(test: &str) -> bool {
    if std::env::var_os(ISOLATED_DAEMON_CASE).as_deref() == Some(std::ffi::OsStr::new(test)) {
        return false;
    }

    let temp = tempfile::tempdir().expect("isolated test process");
    let root = temp.path();
    for name in [
        "home", "tmp", "codex", "claude", "pi", "grok", "config", "data", "cache",
    ] {
        std::fs::create_dir(root.join(name)).expect("isolated process directory");
    }
    let mut command =
        tokio::process::Command::new(std::env::current_exe().expect("test executable"));
    command
        .args(["--exact", test, "--nocapture"])
        .env_clear()
        .env(ISOLATED_DAEMON_CASE, test)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("TMPDIR", root.join("tmp"))
        .env("TMP", root.join("tmp"))
        .env("TEMP", root.join("tmp"))
        .env("CCCC_HOME", root.join("cccc"))
        .env("CODEX_HOME", root.join("codex"))
        .env("CLAUDE_CONFIG_DIR", root.join("claude"))
        .env("PI_CODING_AGENT_DIR", root.join("pi"))
        .env("GROK_HOME", root.join("grok"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .kill_on_drop(true);
    // Windows needs its system directory to start the test executable.
    #[cfg(windows)]
    for name in ["SYSTEMROOT", "WINDIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }

    let output = tokio::time::timeout(Duration::from_secs(45), command.output())
        .await
        .expect("isolated daemon case timed out")
        .expect("start isolated daemon case");
    assert!(
        output.status.success(),
        "isolated daemon case failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    true
}

fn hold_daemon_lock(home: &HomeLayout) -> File {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(home.daemon_dir().join("ccccd.lock"))
        .expect("open held daemon lock");
    lock.try_lock_exclusive().expect("hold daemon lock");
    lock
}

#[test]
fn an_owned_daemon_is_announced_as_stopping_with_this_process() {
    assert_eq!(
        crate::host_state::DaemonState::for_launch(Some(37), || {
            panic!("A retained owner must not wait for external process inspection")
        }),
        crate::host_state::DaemonState::StartedHere { pid: 37 }
    );
    let line = ownership_line(true, None);
    assert!(line.contains("started by this process"), "{line}");
    assert!(line.contains("stops when this process exits"), "{line}");
    assert!(!line.contains("cccc daemon stop"), "{line}");
}

#[test]
fn an_external_daemon_is_announced_with_its_pid_and_how_to_stop_it() {
    let inspected = std::cell::Cell::new(false);
    assert_eq!(
        crate::host_state::DaemonState::for_launch(None, || {
            inspected.set(true);
            Some(35681)
        }),
        crate::host_state::DaemonState::External { pid: Some(35681) }
    );
    assert!(inspected.get(), "External identity must still be inspected");
    let line = ownership_line(false, Some(35681));
    assert!(line.contains("external pid 35681"), "{line}");
    assert!(line.contains("keeps running after exit"), "{line}");
    assert!(line.contains("cccc daemon stop"), "{line}");
}

#[test]
fn an_external_daemon_without_a_pid_still_warns_that_it_survives() {
    let line = ownership_line(false, None);
    assert!(line.contains("external"), "{line}");
    assert!(line.contains("keeps running after exit"), "{line}");
    assert!(line.contains("cccc daemon stop"), "{line}");
}

#[test]
fn distinguishes_rust_from_legacy_daemon_ping() {
    let response = |value: serde_json::Value| {
        DaemonResponse::success(value.as_object().cloned().expect("object"))
    };
    let rust = response(json!({
        "implementation":"rust",
        "version":PRODUCT_VERSION,
        "compatibility":cccc_contracts::RUST_DAEMON_COMPATIBILITY,
    }));
    let legacy = response(json!({"version":"0.4.31"}));
    let stale_rust = response(json!({"implementation":"rust","version":PRODUCT_VERSION}));
    let compatible_other_version = response(json!({
        "implementation":"rust",
        "version":"0.4.999",
        "compatibility":cccc_contracts::RUST_DAEMON_COMPATIBILITY,
    }));
    assert!(is_compatible(&rust));
    assert!(is_compatible(&compatible_other_version));
    assert!(!is_compatible(&legacy));
    assert!(!is_compatible(&stale_rust));
}

#[test]
fn different_known_versions_are_reported_without_assuming_compatibility() {
    let response = |value: serde_json::Value| {
        DaemonResponse::success(value.as_object().cloned().expect("object"))
    };
    let same = response(json!({"version":PRODUCT_VERSION}));
    assert_eq!(different_daemon_version(&same), None);
    let other = response(json!({"version":"0.4.44"}));
    assert_eq!(different_daemon_version(&other), Some("0.4.44"));
    let newer = response(json!({"version":"9.0.0-rc2"}));
    assert_eq!(different_daemon_version(&newer), Some("9.0.0-rc2"));
    for value in [
        json!({}),
        json!({"version":null}),
        json!({"version":"broken"}),
        json!({"version":5}),
    ] {
        assert_eq!(different_daemon_version(&response(value)), None);
    }
}

/// The launch bug this guards: a healthy daemon busy enough to miss a short
/// ping deadline was read as no daemon at all, so the launcher tried to
/// start a second one into the lock the first still held.
#[tokio::test]
async fn a_running_daemon_is_adopted_rather_than_started_over() {
    if run_daemon_case_in_child(
        "daemon_lifecycle::tests::a_running_daemon_is_adopted_rather_than_started_over",
    )
    .await
    {
        return;
    }
    tokio::time::pause();
    let (_temp, home) = home();
    let mut daemon = Box::pin(cccc_daemon::run(home.clone()));
    observed::daemon_published(daemon.as_mut(), &home).await;
    let daemon = tokio::spawn(daemon);

    let mut completed = observed::task(daemon);
    let client = DaemonClient::new(home.clone());
    let (_alive, mut daemon_exited) = watch::channel(false);
    let ready = tokio::select! {
        _ = &mut completed => panic!("daemon ended before its readiness reply"),
        ready = wait_until_ready_or_owner_exits(&client, &mut daemon_exited, Duration::from_secs(30)) => ready,
    };
    assert!(ready, "the actual daemon must answer compatibly");
    let slot = resolve_slot(&home, &client)
        .await
        .expect("adopt a responding owner");
    completed.abort();
    assert!(
        completed
            .await
            .expect_err("daemon task aborted")
            .is_cancelled()
    );
    assert_eq!(
        slot,
        DaemonSlot::Adopted {
            pid: Some(std::process::id())
        },
        "the adopted PID must come from the actual reply"
    );
}

/// The regression this guards: the Web host watched its daemon with a ping
/// deadline, so a daemon that answered more slowly than the deadline read as
/// gone and the Web server closed itself while that daemon was still there.
#[tokio::test]
async fn a_daemon_too_slow_to_answer_is_not_mistaken_for_a_departed_one() {
    tokio::time::pause();
    let (_temp, home) = home();
    let held_lock = hold_daemon_lock(&home);
    let mut watcher = Box::pin(wait_for_loss(&home));
    assert!(observed::poll_once(watcher.as_mut()).await.is_pending());
    observed::advance_past(super::DAEMON_LIVENESS_POLL_INTERVAL).await;
    assert!(
        observed::poll_once(watcher.as_mut()).await.is_pending(),
        "an unanswered daemon that holds the real lock has not departed"
    );
    FileExt::unlock(&held_lock).expect("release daemon lock");
    observed::advance_past(super::DAEMON_LIVENESS_POLL_INTERVAL).await;
    assert!(
        matches!(observed::poll_once(watcher.as_mut()).await, Poll::Ready(())),
        "the released lock must be observed"
    );
}

#[tokio::test]
async fn an_empty_lock_means_this_process_starts_the_daemon() {
    let (_temp, home) = home();
    let client = DaemonClient::new(home.clone());

    assert_eq!(
        resolve_slot(&home, &client)
            .await
            .expect("an unheld lock is not a failure"),
        DaemonSlot::Vacant,
        "no daemon holds the lock, so this process must start one"
    );
}

/// Tests run without a terminal, which is also how an MCP bridge, a service
/// manager, and CI run. None of them can be asked, and none of them may
/// have a daemon terminated on their behalf.
#[tokio::test(start_paused = true)]
async fn a_held_lock_without_a_daemon_to_ask_is_reported_not_seized() {
    let (_temp, home) = home();
    let _held_lock = hold_daemon_lock(&home);
    let client = DaemonClient::new(home.clone());

    let error = resolve_slot(&home, &client)
        .await
        .expect_err("a lock held by an unreachable daemon must not be taken silently");

    let detail = format!("{error:#}");
    assert!(detail.contains("not answering"), "{detail}");
    assert!(detail.contains("cccc daemon stop"), "{detail}");
    // The lock is still held: nothing was terminated to get past it.
    assert_eq!(probe_lock(&home).expect("probe"), LockState::Held);
}

/// A daemon takes the lock before it can answer, so one unanswered ping must
/// not turn a healthy start into a report that it may be stuck.
#[tokio::test]
async fn a_daemon_still_starting_is_adopted_once_it_answers() {
    tokio::time::pause();
    let (_temp, home) = home();
    let _held_lock = hold_daemon_lock(&home);
    let listener = observed::listener(&home).await;
    let client = DaemonClient::new(home.clone());
    let mut slot = Box::pin(resolve_slot(&home, &client));
    let peer = observed::pending_until(slot.as_mut(), observed::ping_read(&listener)).await;
    observed::advance_past(super::DAEMON_SLOW_PROBE_NOTICE).await;
    assert!(observed::poll_once(slot.as_mut()).await.is_pending());
    assert_eq!(probe_lock(&home).expect("owner lock"), LockState::Held);
    observed::pending_until(slot.as_mut(), observed::reject(peer)).await;
    let peer = observed::pending_until(slot.as_mut(), observed::ping_read(&listener)).await;
    assert!(observed::poll_once(slot.as_mut()).await.is_pending());
    let (written, closed) = observed::reply(peer, compatible_ping(json!({})));
    written.await.expect("reply written");
    assert!(matches!(
        slot.await.expect("adopt the responding daemon"),
        DaemonSlot::Adopted { .. }
    ));
    closed.await.expect("reply connection closed");
    assert_eq!(probe_lock(&home).expect("same owner lock"), LockState::Held);
}

#[tokio::test]
async fn daemon_startup_wait_stops_when_the_embedded_daemon_exits() {
    tokio::time::pause();
    let (_temp, home) = home();
    let listener = observed::listener(&home).await;
    let client = DaemonClient::new(home).with_timeout(super::DAEMON_READINESS_POLL_TIMEOUT);
    let (exit_tx, mut exit_rx) = watch::channel(false);
    let budget = super::DAEMON_OWNER_HANDOFF_GRACE + 2 * super::DAEMON_READINESS_POLL_TIMEOUT;
    let deadline = tokio::time::Instant::now() + budget;
    let mut ready = Box::pin(wait_until_ready_or_owner_exits(
        &client,
        &mut exit_rx,
        budget,
    ));
    let peer = observed::pending_until(ready.as_mut(), observed::ping_read(&listener)).await;
    exit_tx.send(true).expect("publish the actual exit event");
    observed::pending_until(ready.as_mut(), observed::reject(peer)).await;
    drop(listener);
    assert!(
        !ready.await,
        "an exited owner with no competitor cannot become ready"
    );
    assert!(
        tokio::time::Instant::now() < deadline,
        "the observed owner exit must end the wait before the full startup budget"
    );
}

#[tokio::test]
async fn startup_readiness_retries_a_silent_probe_and_rejects_a_late_answer() {
    tokio::time::pause();
    let (_temp, home) = home();
    let listener = observed::listener(&home).await;
    let client = DaemonClient::new(home);
    let probe_budget = super::DAEMON_READINESS_POLL_TIMEOUT;

    let deadline = tokio::time::Instant::now() + 3 * probe_budget;
    let mut silent_probe = Box::pin(super::startup_ping(&client, deadline));
    let peer = observed::pending_until(silent_probe.as_mut(), observed::ping_read(&listener)).await;
    assert!(
        observed::poll_once(silent_probe.as_mut())
            .await
            .is_pending()
    );
    observed::advance_past(probe_budget).await;
    assert!(
        matches!(
            observed::poll_once(silent_probe.as_mut()).await,
            Poll::Ready(false)
        ),
        "one silent probe must stop at its logical attempt boundary"
    );
    observed::closed(peer).await;

    // Supply an actual compatible reply after the logical total deadline,
    // before polling the client again. Wall-clock scheduling cannot decide it.
    let deadline = tokio::time::Instant::now() + probe_budget / 2;
    let mut late = Box::pin(super::startup_ping(&client, deadline));
    let peer = observed::pending_until(late.as_mut(), observed::ping_read(&listener)).await;
    assert!(observed::poll_once(late.as_mut()).await.is_pending());
    observed::advance_past(deadline - tokio::time::Instant::now()).await;
    let (written, closed) = observed::reply(peer, compatible_ping(json!({})));
    written.await.expect("late reply written");
    assert!(
        !late.await,
        "a compatible reply delivered after expiry cannot become ready"
    );
    closed.await.expect("late connection closed");

    let deadline = tokio::time::Instant::now() + 3 * probe_budget;
    let mut ready = Box::pin(super::wait_until_ready(&client, deadline));
    let silent = observed::pending_until(ready.as_mut(), observed::ping_read(&listener)).await;
    assert!(observed::poll_once(ready.as_mut()).await.is_pending());
    observed::advance_past(probe_budget).await;
    assert!(observed::poll_once(ready.as_mut()).await.is_pending());
    observed::advance_past(probe_budget).await;
    assert!(observed::poll_once(ready.as_mut()).await.is_pending());
    // An unbounded first call will accept this response and finish before
    // a retry exists. A bounded call already closed it and must send again.
    let (attempted, first_closed) = observed::late_reply(silent, compatible_ping(json!({})));
    attempted.await.expect("late first reply attempted");
    let retry = observed::pending_until(ready.as_mut(), observed::ping_read(&listener)).await;
    first_closed.await.expect("first connection closed");
    assert!(observed::poll_once(ready.as_mut()).await.is_pending());
    let (written, closed) = observed::reply(retry, compatible_ping(json!({})));
    written.await.expect("compatible retry reply written");
    assert!(
        ready.await,
        "the observed retry's compatible answer must be adopted"
    );
    closed.await.expect("retry connection closed");
}

#[tokio::test]
async fn daemon_startup_wait_adopts_a_competing_owner() {
    if run_daemon_case_in_child(
        "daemon_lifecycle::tests::daemon_startup_wait_adopts_a_competing_owner",
    )
    .await
    {
        return;
    }
    tokio::time::pause();
    let (_temp, home) = home();
    let listener = observed::listener(&home).await;
    let client = DaemonClient::new(home.clone());
    let (exit_tx, mut exit_rx) = watch::channel(false);
    let budget = 3 * super::DAEMON_OWNER_HANDOFF_GRACE;
    let mut ready = Box::pin(wait_until_ready_or_owner_exits(
        &client,
        &mut exit_rx,
        budget,
    ));
    let losing_peer = observed::pending_until(ready.as_mut(), observed::ping_read(&listener)).await;
    drop(listener);
    exit_tx.send(true).expect("publish the losing owner's exit");
    let mut external = Box::pin(cccc_daemon::run(home.clone()));
    observed::daemon_published(external.as_mut(), &home).await;
    let external = tokio::spawn(external);

    let mut completed = observed::task(external);
    // EOF on the reply side makes the first call fail; waiting for the client
    // to close its side proves that failure has actually been consumed.
    observed::pending_until(ready.as_mut(), observed::reject(losing_peer)).await;
    assert!(observed::poll_once(ready.as_mut()).await.is_pending());
    observed::advance_past(super::DAEMON_OWNER_HANDOFF_GRACE / 2).await;
    let adopted = tokio::select! {
        _ = &mut completed => panic!("competing daemon ended before adoption"),
        ready = ready.as_mut() => ready,
    };
    assert!(adopted, "the real competing owner must be adopted");
    assert_eq!(probe_lock(&home).expect("external lock"), LockState::Held);
    completed.abort();
    assert!(
        completed
            .await
            .expect_err("daemon task aborted")
            .is_cancelled()
    );
}

#[test]
fn daemon_fixtures_do_not_break_a_later_process_launch() {
    a_running_daemon_is_adopted_rather_than_started_over();
    daemon_startup_wait_adopts_a_competing_owner();

    // Listing the test binary starts no daemon or provider. It does pass through
    // the same guarded process launcher used by other tests in this process.
    let mut command = std::process::Command::new(std::env::current_exe().expect("test executable"));
    command.args(["--list", "--exact", "a_nonexistent_test"]);
    let output =
        cccc_runtime::capture_command_blocking(&mut command, None, Duration::from_secs(10), 1024)
            .expect("daemon fixtures must not poison later launches");
    assert!(output.status.success());
}

/// A stand-in daemon on the address file: answers each op from `reply` and records what it
/// was asked, so a test can prove which requests a command did or did not send.
#[cfg(unix)]
fn fake_daemon(
    home: &HomeLayout,
    reply: fn(&str, &serde_json::Map<String, serde_json::Value>) -> DaemonResponse,
) -> std::sync::Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>> {
    let socket = home.daemon_dir().join("fake.sock");
    let listener = tokio::net::UnixListener::bind(&socket).expect("bind fake daemon");
    let address = json!({
        "v": 1, "transport": "unix", "path": socket, "pid": 4242,
        "version": PRODUCT_VERSION, "ts": "2026-11-17T00:00:00Z",
    });
    std::fs::write(
        cccc_client::address_path(home),
        serde_json::to_vec(&address).expect("address json"),
    )
    .expect("write fake address");
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = seen.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let record = record.clone();
            tokio::spawn(async move {
                let mut stream = BufReader::new(stream);
                let mut line = String::new();
                while stream.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let request: cccc_contracts::DaemonRequest =
                        serde_json::from_str(&line).expect("request");
                    record.lock().expect("record").push((
                        request.op.clone(),
                        serde_json::Value::Object(request.args.clone()),
                    ));
                    let mut bytes =
                        serde_json::to_vec(&reply(&request.op, &request.args)).expect("reply");
                    bytes.push(b'\n');
                    if stream.get_mut().write_all(&bytes).await.is_err() {
                        return;
                    }
                    line.clear();
                }
            });
        }
    });
    seen
}

fn compatible_ping(extra: serde_json::Value) -> DaemonResponse {
    let mut result = json!({
        "implementation": "rust",
        "compatibility": cccc_contracts::RUST_DAEMON_COMPATIBILITY,
        "pid": 4242,
        "version": PRODUCT_VERSION,
    });
    if let (Some(result), Some(extra)) = (result.as_object_mut(), extra.as_object()) {
        result.extend(extra.clone());
    }
    serde_json::from_value(json!({"v":1,"ok":true,"result":result})).expect("ping")
}

#[cfg(unix)]
fn old_daemon(op: &str, _args: &serde_json::Map<String, serde_json::Value>) -> DaemonResponse {
    match op {
        "ping" => compatible_ping(json!({})),
        "shutdown" => {
            serde_json::from_value(json!({"v":1,"ok":true,"result":{"shutting_down":true}}))
                .expect("shutdown")
        }
        _ => serde_json::from_value(json!({"v":1,"ok":false,"error":{
            "code":"unknown_op","message":format!("unknown operation: {op}"),"details":{}}}))
        .expect("unknown op"),
    }
}

/// The first migration: a new CLI meets the old daemon. Asking to keep agents must not
/// turn into the plain stop the old daemon understands, which would end every agent.
#[cfg(unix)]
#[tokio::test]
async fn keeping_agents_on_an_old_daemon_is_refused_and_stops_nothing() {
    let (_temp, home) = home();
    let seen = fake_daemon(&home, old_daemon);
    let client = DaemonClient::new(home.clone());

    let error = super::stop_for_user(&client, &home, super::StopIntent::KeepAgents)
        .await
        .expect_err("an old daemon cannot keep agents");

    let detail = format!("{error:#}");
    assert!(detail.contains("nothing was stopped"), "{detail}");
    assert!(detail.contains("cccc daemon stop"), "{detail}");
    let ops: Vec<String> = seen
        .lock()
        .expect("seen")
        .iter()
        .map(|(op, _)| op.clone())
        .collect();
    assert!(
        ops.contains(&"shutdown_preserving_agents".to_owned()),
        "{ops:?}"
    );
    assert!(
        !ops.contains(&"shutdown".to_owned()),
        "no fallback stop: {ops:?}"
    );
}

/// An old daemon ignores the request ID on a plain stop and writes no receipt; the reply
/// and the absence of a summary line stay what they were.
#[cfg(unix)]
#[tokio::test]
async fn a_plain_stop_on_an_old_daemon_reports_only_its_own_reply() {
    let (_temp, home) = home();
    let seen = fake_daemon(&home, old_daemon);
    let client = DaemonClient::new(home.clone());

    let (response, receipt) = super::stop_for_user(&client, &home, super::StopIntent::StopAgents)
        .await
        .expect("plain stop");

    assert!(response.ok);
    assert!(
        matches!(receipt, super::Receipt::NotSupported),
        "{receipt:?}"
    );
    assert!(super::receipt_line(&home, super::StopIntent::StopAgents, &receipt).is_none());
    let shutdown = seen
        .lock()
        .expect("seen")
        .iter()
        .find(|(op, _)| op == "shutdown")
        .map(|(_, args)| args.clone())
        .expect("plain shutdown sent");
    assert!(
        shutdown
            .get("request_id")
            .and_then(|id| id.as_str())
            .is_some()
    );
    assert!(
        shutdown.get("expected_pid").is_none(),
        "a plain stop is not narrowed to one pid"
    );
}

fn agent(group_id: &str, actor_id: &str) -> cccc_daemon::session_owner::AgentIdentity {
    cccc_daemon::session_owner::AgentIdentity {
        group_id: group_id.into(),
        actor_id: actor_id.into(),
        actor_created_at: "2026-11-17T00:00:00Z".into(),
        generation: None,
        owner_id: None,
    }
}

/// An unconfirmed agent as the receipt names it: with the session owner kept for review.
fn held(
    group_id: &str,
    actor_id: &str,
    owner_id: &str,
) -> cccc_daemon::session_owner::AgentIdentity {
    cccc_daemon::session_owner::AgentIdentity {
        owner_id: Some(owner_id.into()),
        ..agent(group_id, actor_id)
    }
}

fn report(
    mode: cccc_daemon::session_owner::ShutdownMode,
    confirmed_stopped: Vec<cccc_daemon::session_owner::AgentIdentity>,
    retained: Vec<cccc_daemon::session_owner::AgentIdentity>,
    unconfirmed: Vec<cccc_daemon::session_owner::AgentIdentity>,
) -> super::Receipt {
    super::Receipt::Confirmed(cccc_daemon::session_owner::ShutdownReport {
        v: 1,
        daemon_pid: 4242,
        request_id: "request".into(),
        mode,
        confirmed_stopped,
        retained,
        unconfirmed,
    })
}

#[test]
fn receipt_lines_count_only_confirmed_agents_and_name_the_rest_as_possibly_running() {
    use cccc_daemon::session_owner::ShutdownMode;
    let (_temp, home) = home();
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("Night shift", "")
        .expect("group");
    let keep = super::StopIntent::KeepAgents;
    let stop = super::StopIntent::StopAgents;

    let kept = super::receipt_line(
        &home,
        keep,
        &report(
            ShutdownMode::PreserveAgents,
            vec![],
            vec![agent(&group.group_id, "a1"), agent(&group.group_id, "b2")],
            vec![],
        ),
    )
    .expect("kept agents are reported");
    assert!(
        kept.contains("kept 2 agents running (a1, b2 in Night shift)"),
        "{kept}"
    );
    assert!(kept.contains("start CCCC again to reattach them"), "{kept}");

    let partly = super::receipt_line(
        &home,
        stop,
        &report(
            ShutdownMode::Stop,
            vec![agent(&group.group_id, "a1")],
            vec![],
            vec![agent("g_missing", "c3")],
        ),
    )
    .expect("a partial stop is reported");
    assert!(
        partly.contains("stopped 1 agent (a1 in Night shift)"),
        "{partly}"
    );
    assert!(
        partly.contains("could not confirm 1 agent (c3 in g_missing)"),
        "{partly}"
    );
    assert!(partly.contains("cccc daemon stop"), "{partly}");

    // A plain stop never keeps agents on purpose: a retained one is uncertain, not kept.
    let leftover = super::receipt_line(
        &home,
        stop,
        &report(
            ShutdownMode::Stop,
            vec![],
            vec![agent(&group.group_id, "d4")],
            vec![],
        ),
    )
    .expect("a leftover is reported");
    assert!(
        leftover.contains("could not confirm 1 agent (d4 in Night shift)"),
        "{leftover}"
    );
    assert!(!leftover.contains("kept"), "{leftover}");

    assert!(
        super::receipt_line(
            &home,
            stop,
            &report(ShutdownMode::Stop, vec![], vec![], vec![])
        )
        .is_none()
    );
    let none_running = super::receipt_line(
        &home,
        keep,
        &report(ShutdownMode::PreserveAgents, vec![], vec![], vec![]),
    )
    .expect("an empty maintenance stop says so");
    assert!(
        none_running.contains("no agents were running"),
        "{none_running}"
    );
    let only_uncertain = super::receipt_line(
        &home,
        keep,
        &report(
            ShutdownMode::PreserveAgents,
            vec![],
            vec![],
            vec![agent(&group.group_id, "e5")],
        ),
    )
    .expect("an unconfirmed agent is reported");
    assert!(
        only_uncertain.contains("could not confirm 1 agent (e5 in Night shift)"),
        "{only_uncertain}"
    );
    assert!(
        !only_uncertain.contains("no agents were running"),
        "{only_uncertain}"
    );
    // A maintenance stop that stopped an agent it could not keep says so, beside any kept.
    let only_stopped = super::receipt_line(
        &home,
        keep,
        &report(
            ShutdownMode::PreserveAgents,
            vec![agent(&group.group_id, "f6")],
            vec![],
            vec![],
        ),
    )
    .expect("a stopped agent is reported");
    assert!(
        only_stopped.contains("stopped 1 agent (f6 in Night shift)"),
        "{only_stopped}"
    );
    assert!(
        !only_stopped.contains("no agents were running"),
        "{only_stopped}"
    );
    let mixed = super::receipt_line(
        &home,
        keep,
        &report(
            ShutdownMode::PreserveAgents,
            vec![agent(&group.group_id, "f6")],
            vec![agent(&group.group_id, "a1")],
            vec![],
        ),
    )
    .expect("a mixed maintenance stop is reported");
    assert!(
        mixed.contains("kept 1 agent running (a1 in Night shift)"),
        "{mixed}"
    );
    assert!(
        mixed.contains("stopped 1 agent (f6 in Night shift)"),
        "{mixed}"
    );

    // A plain stop suggests stopping again; naming no owner, only when the daemon confirmed it.
    assert!(
        partly.contains("run `cccc daemon stop` again to retry"),
        "{partly}"
    );
    assert!(
        !only_uncertain.contains("daemon stop"),
        "a maintenance stop must not suggest stopping the agents it kept: {only_uncertain}"
    );
    let offline = super::receipt_line(
        &home,
        stop,
        &super::Receipt::WithoutDaemon(cccc_daemon::session_owner::ShutdownReport {
            v: 1,
            daemon_pid: 4242,
            request_id: "request".into(),
            mode: ShutdownMode::Stop,
            confirmed_stopped: vec![],
            retained: vec![],
            unconfirmed: vec![agent(&group.group_id, "c3")],
        }),
    )
    .expect("an offline stop is reported");
    assert!(
        offline.contains("could not confirm 1 agent (c3 in Night shift)")
            && !offline.contains("again"),
        "an offline stop already was the retry: {offline}"
    );

    // An agent a live session owner still holds: after a plain stop, another stop reaches
    // that owner directly and can confirm it, so it comes first and the owner's review
    // command, once per owner, is the fallback. After a kept stop, another plain stop would
    // also stop the kept agents, so only the review command is named.
    let owner = "2cabc7ed-cb39-40ea-a8e2-c06dd258b4a1";
    let review = "`cccc daemon process-recovery --owner session --owner-id 2cabc7edcb3940eaa8e2c06dd258b4a1 list`";
    let held_two = |mode| {
        report(
            mode,
            vec![],
            vec![],
            vec![
                held(&group.group_id, "c1", owner),
                held(&group.group_id, "c2", owner),
            ],
        )
    };
    let plain = super::receipt_line(&home, stop, &held_two(ShutdownMode::Stop))
        .expect("an owner-held agent is reported");
    assert!(
        plain.contains("could not confirm 2 agents (c1, c2 in Night shift), which may still be running; run `cccc daemon stop` again to retry; if it still cannot confirm them, run ")
            && plain.contains(review)
            && plain.matches("process-recovery").count() == 1,
        "{plain}"
    );
    let kept = super::receipt_line(&home, keep, &held_two(ShutdownMode::PreserveAgents))
        .expect("an owner-held agent is reported");
    assert!(
        kept.contains("could not confirm 2 agents (c1, c2 in Night shift), which may still be running; to review, run ")
            && kept.contains(review)
            && kept.matches("process-recovery").count() == 1
            && !kept.contains("daemon stop"),
        "{kept}"
    );
    let two = super::receipt_line(
        &home,
        stop,
        &report(
            ShutdownMode::Stop,
            vec![],
            vec![],
            vec![
                held(&group.group_id, "c1", owner),
                held(&group.group_id, "c2", "9f0e8d7c6b5a49388271605f4e3d2c1b"),
            ],
        ),
    )
    .expect("two owners are reported");
    assert!(
        two.contains(
            "if it still cannot confirm them, run `cccc daemon process-recovery --owner session --owner-id 2cabc7edcb3940eaa8e2c06dd258b4a1 list` and `cccc daemon process-recovery --owner session --owner-id 9f0e8d7c6b5a49388271605f4e3d2c1b list`"
        ),
        "{two}"
    );
    // Only a value that parses as a UUID goes into a command.
    let odd = super::receipt_line(
        &home,
        stop,
        &report(
            ShutdownMode::Stop,
            vec![],
            vec![],
            vec![held(&group.group_id, "c1", "x; rm -rf ~")],
        ),
    )
    .expect("an unparsable owner is reported");
    assert!(
        !odd.contains("rm -rf") && !odd.contains("process-recovery"),
        "{odd}"
    );

    let missing_keep =
        super::receipt_line(&home, keep, &super::Receipt::Missing).expect("missing receipt");
    assert!(
        missing_keep.contains("could not confirm which agents kept running"),
        "{missing_keep}"
    );
    let missing_stop =
        super::receipt_line(&home, stop, &super::Receipt::Missing).expect("missing receipt");
    assert!(
        missing_stop.contains("some may still be running"),
        "{missing_stop}"
    );
}

#[cfg(unix)]
fn daemon_with_retained_agents(
    op: &str,
    _args: &serde_json::Map<String, serde_json::Value>,
) -> DaemonResponse {
    assert_eq!(op, "ping");
    compatible_ping(json!({"retained_agents":[
        {"group_id":"g_x","actor_id":"k1","actor_created_at":"2026-11-17T00:00:00Z","generation":null}
    ]}))
}

#[cfg(unix)]
fn quiet_daemon(op: &str, _args: &serde_json::Map<String, serde_json::Value>) -> DaemonResponse {
    assert_eq!(op, "ping");
    compatible_ping(json!({}))
}

#[cfg(unix)]
#[tokio::test]
async fn startup_names_retained_agents_without_claiming_web_has_reconnected() {
    let (_temp, home) = home();
    fake_daemon(&home, daemon_with_retained_agents);
    let line = super::retained_line(&DaemonClient::new(home.clone()), &home)
        .await
        .expect("retained agents are reported");
    assert_eq!(line, "[cccc] Found 1 agent that kept running (k1 in g_x).");

    let (_temp, quiet) = self::home();
    fake_daemon(&quiet, quiet_daemon);
    assert!(
        super::retained_line(&DaemonClient::new(quiet.clone()), &quiet)
            .await
            .is_none()
    );
}

#[test]
fn the_terminal_summary_keeps_the_stop_mode_and_the_owners_of_every_uncertain_agent() {
    use crate::host_state::AgentsResult;
    use cccc_daemon::session_owner::ShutdownMode;
    // A plain stop keeps nothing on purpose: an agent left with its owner is uncertain,
    // and that owner is named for review.
    assert_eq!(
        super::agents_result(&report(
            ShutdownMode::Stop,
            vec![agent("g", "a1")],
            vec![held("g", "b2", "1b2b3b4b5b6b4b7b8b9b0b1b2b3b4b5b")],
            vec![held("g", "c3", "2c3c4c5c6c7c4c8c9c0c1c2c3c4c5c6c")],
        )),
        AgentsResult::Confirmed {
            plain: true,
            stopped: 1,
            kept: 0,
            uncertain: 2,
            review_owners: vec![
                "1b2b3b4b5b6b4b7b8b9b0b1b2b3b4b5b".into(),
                "2c3c4c5c6c7c4c8c9c0c1c2c3c4c5c6c".into()
            ],
        }
    );
    // A kept stop, possibly asked for from another terminal, counts what it kept.
    assert_eq!(
        super::agents_result(&report(
            ShutdownMode::PreserveAgents,
            vec![],
            vec![held("g", "b2", "1b2b3b4b5b6b4b7b8b9b0b1b2b3b4b5b")],
            vec![held("g", "c3", "2c3c4c5c6c7c4c8c9c0c1c2c3c4c5c6c")],
        )),
        AgentsResult::Confirmed {
            plain: false,
            stopped: 0,
            kept: 1,
            uncertain: 1,
            review_owners: vec!["2c3c4c5c6c7c4c8c9c0c1c2c3c4c5c6c".into()],
        }
    );
}
