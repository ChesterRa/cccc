use super::{
    DaemonSlot, different_daemon_version, is_compatible, ownership_line, resolve_slot,
    wait_for_loss, wait_until_ready_or_owner_exits,
};
use crate::PRODUCT_VERSION;
use crate::daemon_takeover::{LockState, probe_lock};
use cccc_client::DaemonClient;
use cccc_contracts::{DaemonAddress, DaemonResponse, Transport};
use cccc_core::HomeLayout;
use fs2::FileExt;
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::watch;

const ISOLATED_DAEMON_CASE: &str = "CCCC_TEST_ISOLATED_DAEMON_CASE";

fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    (temp, home)
}

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
    let line = ownership_line(true, None);
    assert!(line.contains("started by this process"), "{line}");
    assert!(line.contains("stops when this process exits"), "{line}");
    assert!(!line.contains("cccc daemon stop"), "{line}");
}

#[test]
fn an_external_daemon_is_announced_with_its_pid_and_how_to_stop_it() {
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
    let (_temp, home) = home();
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = DaemonClient::new(home.clone());
    // Hold the sender: dropping it reads as "the daemon exited" and cuts
    // the wait short of what a slow machine needs to start one.
    let (_alive, mut daemon_exited) = watch::channel(false);
    assert!(
        wait_until_ready_or_owner_exits(&client, &mut daemon_exited, Duration::from_secs(30)).await,
        "the test daemon never became ready"
    );

    let slot = resolve_slot(&home, &client)
        .await
        .expect("adopting a running daemon is not a failure");

    daemon.abort();
    let _ = daemon.await;
    // The adopted daemon is this test's own process, and its PID must come
    // from the reply rather than a second round trip.
    assert_eq!(
        slot,
        DaemonSlot::Adopted {
            pid: Some(std::process::id())
        },
        "a daemon that answers must be adopted, not started over"
    );
}

/// The regression this guards: the Web host watched its daemon with a ping
/// deadline, so a daemon that answered more slowly than the deadline read as
/// gone and the Web server closed itself while that daemon was still there.
#[tokio::test]
async fn a_daemon_too_slow_to_answer_is_not_mistaken_for_a_departed_one() {
    let (_temp, home) = home();
    // A daemon that holds the lock and answers nothing at all -- the most
    // extreme version of "slower than any deadline".
    let held_lock = hold_daemon_lock(&home);

    let watched = home.clone();
    let watcher = tokio::spawn(async move { wait_for_loss(&watched).await });
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        !watcher.is_finished(),
        "a silent daemon that still holds the lock must not read as departed"
    );

    // Releasing the lock is what a departing daemon actually does, and the
    // operating system does it too when a daemon dies.
    FileExt::unlock(&held_lock).expect("release daemon lock");
    tokio::time::timeout(Duration::from_secs(5), watcher)
        .await
        .expect("a released lock must be noticed")
        .expect("watcher task");
}

#[tokio::test]
async fn an_empty_lock_means_this_process_starts_the_daemon() {
    let (_temp, home) = home();
    let client = DaemonClient::new(home.clone()).with_timeout(Duration::from_millis(50));

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
#[tokio::test]
async fn a_held_lock_without_a_daemon_to_ask_is_reported_not_seized() {
    let (_temp, home) = home();
    let _held_lock = hold_daemon_lock(&home);
    let client = DaemonClient::new(home.clone()).with_timeout(Duration::from_millis(50));

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
    let (_temp, home) = home();
    let _held_lock = hold_daemon_lock(&home);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("daemon listener");
    let published = DaemonAddress {
        v: 1,
        transport: Transport::Tcp,
        path: String::new(),
        host: "127.0.0.1".into(),
        port: listener.local_addr().expect("daemon address").port(),
        pid: std::process::id(),
        version: PRODUCT_VERSION.into(),
        source_id: String::new(),
        ts: String::new(),
    };
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        std::fs::write(
            daemon_home.daemon_dir().join("ccccd.addr.json"),
            serde_json::to_vec(&published).expect("address JSON"),
        )
        .expect("publish address");
        let (stream, _) = listener.accept().await.expect("ping connection");
        let mut stream = BufReader::new(stream);
        let mut request = String::new();
        stream.read_line(&mut request).await.expect("ping request");
        let request = serde_json::from_str(&request).expect("ping JSON");
        let mut reply = serde_json::to_vec(&cccc_daemon::handle_request(&daemon_home, &request))
            .expect("ping response");
        reply.push(b'\n');
        stream.write_all(&reply).await.expect("ping reply");
    });
    let client = DaemonClient::new(home.clone()).with_timeout(Duration::from_millis(500));

    let slot = resolve_slot(&home, &client)
        .await
        .expect("a daemon that answers is not stuck");

    daemon.await.expect("starting daemon");
    assert!(matches!(slot, DaemonSlot::Adopted { .. }), "{slot:?}");
}

#[tokio::test]
async fn daemon_startup_wait_stops_when_the_embedded_daemon_exits() {
    let (_temp, home) = home();
    let client = DaemonClient::new(home).with_timeout(Duration::from_millis(20));
    let (exit_tx, mut exit_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(25)).await;
        exit_tx.send(true).expect("publish embedded daemon exit");
    });

    let ready = tokio::time::timeout(
        Duration::from_secs(3),
        wait_until_ready_or_owner_exits(&client, &mut exit_rx, Duration::from_secs(5)),
    )
    .await
    .expect("startup wait ignored the embedded daemon exit signal");
    assert!(!ready, "an exited embedded daemon cannot become ready");
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
    let (_temp, home) = home();
    let client = DaemonClient::new(home.clone()).with_timeout(Duration::from_millis(50));
    let (exit_tx, mut exit_rx) = watch::channel(false);
    let daemon_home = home.clone();
    let external = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(75)).await;
        cccc_daemon::run(daemon_home).await
    });
    exit_tx.send(true).expect("publish losing embedded owner");

    let ready = tokio::time::timeout(
        Duration::from_secs(3),
        wait_until_ready_or_owner_exits(&client, &mut exit_rx, Duration::from_secs(5)),
    )
    .await
    .expect("startup handoff timed out");

    external.abort();
    let _ = external.await;
    assert!(ready, "a ready competing daemon should be adopted");
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

#[cfg(unix)]
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

    // Only a stop the daemon confirmed, naming no owner, suggests stopping again.
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

    // An owner kept for review is named with its command, in either mode, once per owner.
    let owner = "2cabc7ed-cb39-40ea-a8e2-c06dd258b4a1";
    let review = "`cccc daemon process-recovery --owner session --owner-id 2cabc7edcb3940eaa8e2c06dd258b4a1 list`";
    for (intent, mode) in [
        (stop, ShutdownMode::Stop),
        (keep, ShutdownMode::PreserveAgents),
    ] {
        let line = super::receipt_line(
            &home,
            intent,
            &report(
                mode,
                vec![],
                vec![],
                vec![
                    held(&group.group_id, "c1", owner),
                    held(&group.group_id, "c2", owner),
                ],
            ),
        )
        .expect("an owner-held agent is reported");
        assert!(
            line.contains("could not confirm 2 agents (c1, c2 in Night shift), which may still be running; to review, run ")
                && line.contains(review)
                && line.matches("process-recovery").count() == 1
                && !line.contains("daemon stop"),
            "{line}"
        );
    }
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
            "to review, run `cccc daemon process-recovery --owner session --owner-id 2cabc7edcb3940eaa8e2c06dd258b4a1 list` and `cccc daemon process-recovery --owner session --owner-id 9f0e8d7c6b5a49388271605f4e3d2c1b list`"
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
