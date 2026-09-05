use super::{
    DaemonSlot, is_compatible, ownership_line, resolve_slot, wait_for_loss,
    wait_until_ready_or_owner_exits,
};
use crate::PRODUCT_VERSION;
use crate::daemon_takeover::{LockState, probe_lock};
use cccc_client::DaemonClient;
use cccc_contracts::DaemonResponse;
use cccc_core::HomeLayout;
use fs2::FileExt;
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::time::Duration;
use tokio::sync::watch;

fn home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    (temp, home)
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

/// The launch bug this guards: a healthy daemon busy enough to miss a short
/// ping deadline was read as no daemon at all, so the launcher tried to
/// start a second one into the lock the first still held.
#[tokio::test]
async fn a_running_daemon_is_adopted_rather_than_started_over() {
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
