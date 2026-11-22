use super::DetachedDaemon;
use cccc_core::HomeLayout;
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

#[test]
fn startup_recovery_hint_uses_only_this_start_and_never_echoes_log_secrets() {
    let temp = tempfile::tempdir().expect("isolated test directory");
    let log = temp.path().join("daemon.log");
    let id = uuid::Uuid::new_v4().simple().to_string();
    let old = format!(
        "Error: run cccc daemon process-recovery --owner session --owner-id {id} list; private=DO_NOT_ECHO\n"
    );
    std::fs::write(&log, &old).expect("write fixture file");
    assert!(super::session_recovery_hint(&log, old.len() as u64).is_none());
    let hint = super::session_recovery_hint(&log, 0).expect("recovery command in log tail");
    assert!(hint.contains(&format!("--owner-id {id} list")));
    assert!(!hint.contains("DO_NOT_ECHO"));
    std::fs::write(
        &log,
        format!("{old}cccc daemon process-recovery --owner session --owner-id incomplete"),
    )
    .expect("write fixture file");
    assert!(
        super::session_recovery_hint(&log, 0)
            .expect("recovery command in log tail")
            .contains(&id)
    );
    std::fs::write(&log, old.replace(&id, "invalid-owner")).expect("write fixture file");
    assert!(super::session_recovery_hint(&log, 0).is_none());
}

#[test]
fn builds_cli_self_launch_command() {
    let home = HomeLayout::from_path(Path::new("test-home").to_path_buf()).expect("home");
    let launch = DetachedDaemon::new("cccc", ["daemon", "run"]);
    let command = launch.command(&home);
    let args = command.get_args().collect::<Vec<_>>();

    #[cfg(unix)]
    {
        assert_eq!(command.get_program(), OsStr::new("nohup"));
        assert_eq!(
            args,
            [OsStr::new("cccc"), OsStr::new("daemon"), OsStr::new("run")]
        );
    }
    #[cfg(windows)]
    {
        assert_eq!(command.get_program(), OsStr::new("cccc"));
        assert_eq!(args, [OsStr::new("daemon"), OsStr::new("run")]);
    }

    assert!(command.get_envs().any(|(key, value)| {
        key == OsStr::new("CCCC_HOME") && value == Some(home.root().as_os_str())
    }));
}

#[test]
fn an_update_handoff_is_an_explicit_daemon_argument() {
    let home = HomeLayout::from_path("test-home").expect("home");
    let transaction = home.root().join("update/transaction");
    let launch =
        DetachedDaemon::new("cccc", ["daemon", "run"]).with_update_resume(transaction.clone());
    let command = launch.command(&home);
    let args = command.get_args().collect::<Vec<_>>();
    assert_eq!(args[args.len() - 2], OsStr::new("--update-resume"));
    assert_eq!(args[args.len() - 1], transaction.as_os_str());
    assert!(!command.get_envs().any(|(key, _)| key == "CCCC_UPDATE_TXN"));
}

#[tokio::test]
async fn an_active_update_blocks_start_before_ping_spawn_and_process_tree_cleanup() {
    use cccc_core::attended_update::{BuildIdentity, Intent, arm};
    let temp = tempfile::tempdir().expect("fixture directory");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize home");
    let id = uuid::Uuid::new_v4().simple().to_string();
    std::fs::create_dir_all(home.root().join("update").join(&id)).expect("transaction");
    let intent = Intent::new(
        &home,
        &id,
        BuildIdentity {
            version: "0.4.59".into(),
            source_id: "1".repeat(64),
        },
        cccc_core::attended_update::Source::Release {
            install_dir: temp.path().to_path_buf(),
        },
    )
    .expect("intent");
    arm(&home, &intent).expect("arm");
    let error = DetachedDaemon::new("must-not-be-executed", ["daemon", "run"])
        .start(&home)
        .await
        .expect_err("ordinary detached start blocked");
    assert!(error.to_string().contains("update_in_progress"));
    let paths = crate::DaemonPaths::new(home.clone());
    assert!(
        !paths.log.exists(),
        "a rejected start must not even open its log"
    );
    let error = crate::server::claim_home(&paths).expect_err("direct server start blocked");
    assert!(error.to_string().contains("update_in_progress"));
    assert!(
        !paths.lock.exists(),
        "the first check precedes daemon locking"
    );
    assert_eq!(
        cccc_core::attended_update::active(&home).expect("intent"),
        Some(intent)
    );
}

#[tokio::test]
async fn an_expired_startup_budget_is_rejected_before_attempting_process_creation() {
    let temp = tempfile::tempdir().expect("fixture directory");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let launch = DetachedDaemon::new("must-not-be-executed", ["daemon", "run"]);
    let error = launch
        .start_supervised_until(&home, tokio::time::Instant::now())
        .await
        .expect_err("an expired request cannot attempt to launch its executable");
    assert!(
        error
            .to_string()
            .contains("startup deadline elapsed before process creation"),
        "{error:#}"
    );
}

#[tokio::test]
async fn reports_a_child_that_exits_before_becoming_ready() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let result = tokio::time::timeout(Duration::from_secs(3), failing_daemon().start(&home))
        .await
        .expect("failed child must be reported without waiting for the ready timeout");
    let error = result.expect_err("failed child must not be reported as started");
    let detail = format!("{error:#}");
    assert!(detail.contains("exited before becoming ready"), "{detail}");
    assert!(detail.contains("23"), "{detail}");
}

#[cfg(unix)]
fn failing_daemon() -> DetachedDaemon {
    DetachedDaemon::new("/bin/sh", ["-c", "exit 23"])
}

#[cfg(windows)]
fn failing_daemon() -> DetachedDaemon {
    let command = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
    DetachedDaemon::new(command, ["/C", "exit 23"])
}

#[cfg(unix)]
async fn supervised_fixture_pid(home: &HomeLayout) -> i32 {
    loop {
        if let Ok(text) = std::fs::read_to_string(home.root().join("alive.pid"))
            && let Ok(pid) = text.trim().parse::<i32>()
        {
            return pid;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_supervised_start_terminates_process_before_readiness() {
    use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
    use nix::unistd::Pid;
    let temp = tempfile::tempdir().expect("create test directory");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("create home layout");
    let launch = DetachedDaemon::new("sh", ["-c", "echo $$ > alive.pid; exec sleep 60"]);
    let mut startup = Box::pin(launch.start_supervised(&home));
    let pid = tokio::select! {
        result = &mut startup => panic!("fake daemon must not become ready: {result:?}"),
        pid = tokio::time::timeout(Duration::from_secs(5), supervised_fixture_pid(&home)) => pid.expect("fixture value must be available"),
    };
    drop(startup);
    // The resource was registered before the asynchronous readiness loop.
    // Cancelling it must kill the real child, not just drop the readiness task.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match waitpid(Pid::from_raw(pid), Some(WaitPidFlag::WNOHANG))
            .expect("complete waitpid in fixture")
        {
            WaitStatus::Signaled(_, nix::sys::signal::Signal::SIGKILL, _) => break,
            WaitStatus::StillAlive if std::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            status => panic!("supervised child survived cancellation: {status:?}"),
        }
    }

    std::fs::remove_file(home.root().join("alive.pid")).expect("discard the previous child marker");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let mut startup = Box::pin(launch.start_supervised_until(&home, deadline));
    let pid = tokio::select! {
        result = &mut startup => panic!("the deadline fixture must start a real child: {result:?}"),
        pid = tokio::time::timeout(Duration::from_secs(5), supervised_fixture_pid(&home)) => pid.expect("new fixture PID"),
    };
    let error = tokio::time::timeout(Duration::from_secs(3), startup)
        .await
        .expect("the inherited budget must not become a fresh 30 seconds")
        .expect_err("the silent child cannot become ready");
    assert!(
        error.to_string().contains("failed to become ready"),
        "{error:#}"
    );
    assert_eq!(
        waitpid(Pid::from_raw(pid), Some(WaitPidFlag::WNOHANG)),
        Err(nix::errno::Errno::ECHILD),
        "deadline cleanup must terminate and reap its own child"
    );
}
