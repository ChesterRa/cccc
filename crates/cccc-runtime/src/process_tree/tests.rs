use super::*;

#[test]
fn pty_spawn_failure_does_not_claim_the_child_never_started() {
    let result = OwnedProcessTree::spawn_pty(|| {
        Err(CommandError::Started(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "injected PTY failure",
        )))
    });
    let error = match result {
        Ok(_) => panic!("the fixture must fail"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), std::io::ErrorKind::Other);
    assert!(
        error
            .get_ref()
            .is_some_and(|source| source.is::<ProcessStartUnconfirmed>())
    );
}

#[test]
fn pty_resource_failure_waits_only_after_a_successful_kill() {
    use portable_pty::{Child, ChildKiller, ExitStatus};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[derive(Clone, Debug)]
    struct FixtureChild {
        waits: Arc<AtomicUsize>,
        polls: Arc<AtomicUsize>,
        kill_succeeds: bool,
    }
    impl ChildKiller for FixtureChild {
        fn kill(&mut self) -> std::io::Result<()> {
            if self.kill_succeeds {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected signal failure",
                ))
            }
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(self.clone())
        }
    }
    impl Child for FixtureChild {
        fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
            self.polls.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        }

        fn wait(&mut self) -> std::io::Result<ExitStatus> {
            self.waits.fetch_add(1, Ordering::Relaxed);
            Err(std::io::Error::other("fixture is still running"))
        }

        fn process_id(&self) -> Option<u32> {
            None
        }

        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
            None
        }
    }

    for kill_succeeds in [false, true] {
        let waits = Arc::new(AtomicUsize::new(0));
        let polls = Arc::new(AtomicUsize::new(0));
        let child = FixtureChild {
            waits: Arc::clone(&waits),
            polls: Arc::clone(&polls),
            kill_succeeds,
        };
        let error = match OwnedProcessTree::spawn_pty(|| Ok(Box::new(child))) {
            Ok(_) => panic!("missing PTY resource must fail"),
            Err(error) => error,
        };
        assert!(
            error
                .get_ref()
                .is_some_and(|source| source.is::<ProcessStartUnconfirmed>())
        );
        assert_eq!(waits.load(Ordering::Relaxed), usize::from(kill_succeeds));
        assert_eq!(polls.load(Ordering::Relaxed), usize::from(!kill_succeeds));
    }
}

use std::io::{BufRead, BufReader, Read};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn shell(script: &str) -> (Child, OwnedProcessTree) {
    OwnedProcessTree::spawn(
        Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped()),
    )
    .expect("spawn child process")
}

fn expect_pipe_closes(stdout: impl Read + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut stdout = stdout;
        let mut output = Vec::new();
        let _ = tx.send(stdout.read_to_end(&mut output));
    });
    rx.recv_timeout(Duration::from_secs(5))
        .expect("parent or descendant still owns stdout")
        .expect("complete expect in fixture");
}

#[test]
fn natural_exit_terminates_descendants_before_reaping_and_revokes_old_owner() {
    let (mut child, owner) = shell("sleep 60 & echo ready; exit 0");
    let mut stdout = BufReader::new(child.stdout.take().expect("take owned test resource"));
    let mut ready = String::new();
    stdout.read_line(&mut ready).expect("read child readiness");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if owner
            .try_wait(|| child.try_wait())
            .expect("poll child process")
            .is_some()
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    expect_pipe_closes(stdout);
    let (mut replacement, replacement_owner) = shell("sleep 60");
    owner.terminate().expect("terminate owned process tree");
    drop(owner);
    assert!(
        replacement_owner
            .try_wait(|| replacement.try_wait())
            .expect("poll child process")
            .is_none()
    );
    replacement_owner
        .terminate()
        .expect("terminate owned process tree");
    replacement.wait().expect("reap child process");
}

#[test]
fn force_termination_bypasses_session_locks_and_closes_admission() {
    const CHILD: &str = "CCCC_TEST_FORCE_PROCESS_TREES";
    if std::env::var_os(CHILD).is_none() {
        let mut child = Command::new(std::env::current_exe().expect("locate test executable"))
            .args(["--exact", "process_tree::tests::force_termination_bypasses_session_locks_and_closes_admission", "--nocapture"])
            .env(CHILD, "1").spawn().expect("spawn child process");
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = child.try_wait().expect("poll child process") {
                assert!(status.success());
                return;
            }
            if Instant::now() >= deadline {
                child.kill().expect("terminate child process");
                child.wait().expect("reap child process");
                panic!("force termination blocked on session shutdown");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let temp = tempfile::tempdir().expect("create test directory");
    crate::start(crate::test_support::spec(
        &temp,
        "force",
        "pty",
        "sleep 60 & echo $! > descendant.pid; wait",
    ))
    .expect("complete start in fixture");
    let session = crate::registry::lookup("force", "pty").expect("complete lookup in fixture");
    // Hold the real PTY session lock throughout forced termination.
    let mut held_session = session.lock().expect("lock test state");
    let (mut child, owner) = shell("sleep 60 & echo ready; wait");
    let mut stdout = BufReader::new(child.stdout.take().expect("take owned test resource"));
    stdout
        .read_line(&mut String::new())
        .expect("read child readiness");
    let mut deepseek = crate::deepseek_supervisor::DeepSeekSupervisor::default();
    deepseek
        .start(
            &["sh".into(), "-c".into(), "sleep 60".into()],
            temp.path(),
            &[],
        )
        .expect("complete start in fixture");
    let held_deepseek = Mutex::new(deepseek);
    let mut supervisor = held_deepseek.lock().expect("lock test state");
    force_terminate_owned().expect("terminate all owned processes");
    expect_pipe_closes(stdout);
    assert!(!child.wait().expect("reap child process").success());
    assert!(OwnedProcessTree::spawn(&mut Command::new("sh")).is_err());
    let deadline = Instant::now() + Duration::from_secs(5);
    while supervisor.is_running() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    while held_session.status().running {
        assert!(Instant::now() < deadline, "PTY survived forced termination");
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(owner);
    drop(held_session);
    crate::stop("force", "pty").expect("stop owned runtime");
    supervisor.stop().expect("stop owned runtime");
}
