use super::DetachedDaemon;
use cccc_core::HomeLayout;
use std::ffi::OsStr;
use std::path::Path;

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
    assert!(
        !crate::DaemonPaths::new(home).log.exists(),
        "the expired start did not open the child's log"
    );
}

#[tokio::test]
async fn reports_a_child_that_exits_before_becoming_ready() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let error = failing_daemon()
        .start(&home)
        .await
        .expect_err("failed child must not be reported as started");
    let detail = format!("{error:#}");
    let exit = detail
        .split_once("; see ")
        .expect("child exit and log hint")
        .0;
    assert!(
        exit.contains("exited before becoming ready") && exit.ends_with(" 23"),
        "{detail}"
    );
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
#[tokio::test(start_paused = true)]
async fn cancelled_supervised_start_terminates_process_before_readiness() -> anyhow::Result<()> {
    use nix::{
        errno::Errno,
        sys::{signal::Signal, wait::WaitStatus},
    };
    use std::time::Duration;
    use tokio::time::Instant;

    if supervised_fixture::run_if_child()? {
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let home = HomeLayout::from_path(temp.path().join("home"))?;
    let control = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let launch = supervised_fixture::launch(control.local_addr()?)?;
    // Tokio keeps paused time stationary while a blocking task is active. Unwinding
    // drops the sender too, so a failed assertion cannot strand the runtime.
    let (release_clock, held) = std::sync::mpsc::channel::<()>();
    let clock_guard = tokio::task::spawn_blocking(move || held.recv());

    let mut startup = Box::pin(launch.start_supervised(&home));
    let mut child = supervised_fixture::observe(&control, startup.as_mut()).await?;
    drop(startup);
    assert_eq!(
        child.release_and_wait(),
        Ok(WaitStatus::Signaled(child.pid, Signal::SIGKILL, false)),
        "cancelling readiness must kill its child"
    );

    let started = Instant::now();
    let budget = Duration::from_secs(1);
    let deadline = started + budget;
    let mut startup = Box::pin(launch.start_supervised_until(&home, deadline));
    let mut child = supervised_fixture::observe(&control, startup.as_mut()).await?;
    assert_eq!(
        Instant::now(),
        started,
        "the child handshake must not advance virtual time"
    );
    tokio::time::advance(budget).await;
    release_clock.send(())?;
    let error = startup.await.expect_err("the unready child cannot start");
    assert!(
        error.to_string().contains("failed to become ready"),
        "{error:#}"
    );
    assert_eq!(
        Instant::now(),
        deadline,
        "startup must use its caller's deadline"
    );
    assert_eq!(
        child.release_and_wait(),
        Err(Errno::ECHILD),
        "deadline cleanup must reap the child"
    );
    clock_guard.await??;
    Ok(())
}

#[cfg(unix)]
mod supervised_fixture {
    use super::DetachedDaemon;
    use anyhow::{Result, bail, ensure};
    use nix::{
        errno::Errno,
        sys::wait::{WaitStatus, waitpid},
        unistd::Pid,
    };
    use std::{
        ffi::OsString,
        fmt::Debug,
        future::Future,
        io::{Read, Write},
        net::SocketAddr,
        pin::Pin,
    };
    use tokio::{
        io::AsyncReadExt,
        net::{TcpListener, TcpStream},
    };

    const CONTROL: &str = "CCCC_TEST_SUPERVISED_START_CONTROL";
    const TEST: &str =
        "process::tests::cancelled_supervised_start_terminates_process_before_readiness";

    pub fn run_if_child() -> Result<bool> {
        let Ok(address) = std::env::var(CONTROL) else {
            return Ok(false);
        };
        let address: SocketAddr = address.parse()?;
        ensure!(
            address.ip().is_loopback(),
            "expected the test control listener"
        );
        let mut stream = std::net::TcpStream::connect(address)?;
        stream.write_all(&std::process::id().to_be_bytes())?;
        ensure!(
            stream.read(&mut [0])? == 0,
            "expected parent to close the control connection"
        );
        Ok(true)
    }

    pub fn launch(address: SocketAddr) -> Result<DetachedDaemon> {
        let control_env = OsString::from(format!("{CONTROL}={address}"));
        let program = std::env::current_exe()?.into_os_string();
        Ok(DetachedDaemon::new(
            "env",
            [control_env, program, "--exact".into(), TEST.into()],
        ))
    }

    pub async fn observe<F: Future<Output: Debug>>(
        control: &TcpListener,
        mut startup: Pin<&mut F>,
    ) -> Result<Child> {
        let connected = async {
            let (mut stream, _) = control.accept().await?;
            let pid = i32::try_from(stream.read_u32().await?)?;
            ensure!(pid > 1, "invalid fixture PID");
            Ok::<_, anyhow::Error>(Child {
                pid: Pid::from_raw(pid),
                stream: Some(stream),
            })
        };
        let child = tokio::select! {
            result = &mut startup => bail!("startup completed before the child handshake: {result:?}"),
            child = connected => child?,
        };
        // The real child is alive. Startup owns it and must remain pending on this unready Home.
        ensure!(
            futures_util::poll!(startup).is_pending(),
            "startup completed before readiness"
        );
        Ok(child)
    }

    pub struct Child {
        pub pid: Pid,
        stream: Option<TcpStream>,
    }

    impl Child {
        pub fn release_and_wait(&mut self) -> Result<WaitStatus, Errno> {
            drop(self.stream.take()); // A surviving child exits normally on EOF.
            loop {
                match waitpid(self.pid, None) {
                    Err(Errno::EINTR) => continue,
                    result => return result,
                }
            }
        }
    }

    impl Drop for Child {
        fn drop(&mut self) {
            if self.stream.is_some() {
                let _ = self.release_and_wait();
            }
        }
    }
}
