use super::DetachedDaemon;
use cccc_core::HomeLayout;
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

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
#[tokio::test]
async fn cancelled_supervised_start_terminates_process_before_readiness() {
    use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
    use nix::unistd::Pid;
    let temp = tempfile::tempdir().expect("create test directory");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("create home layout");
    let launch = DetachedDaemon::new("sh", ["-c", "echo $$ > alive.pid; exec sleep 60"]);
    let mut startup = Box::pin(launch.start_supervised(&home));
    let ready = async {
        loop {
            if let Ok(text) = std::fs::read_to_string(home.root().join("alive.pid"))
                && let Ok(pid) = text.trim().parse::<i32>()
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    let pid = tokio::select! {
        result = &mut startup => panic!("fake daemon must not become ready: {result:?}"),
        pid = tokio::time::timeout(Duration::from_secs(5), ready) => pid.expect("fixture value must be available"),
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
}
