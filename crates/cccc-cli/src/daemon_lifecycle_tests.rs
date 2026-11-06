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
