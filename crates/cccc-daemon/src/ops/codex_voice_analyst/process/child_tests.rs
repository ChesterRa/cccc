use super::*;
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

#[test]
fn forced_exit_bypasses_managed_child_lock_and_kills_descendants() {
    const CHILD: &str = "CCCC_TEST_FORCE_MANAGED_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = Command::new(std::env::current_exe().expect("locate test executable"))
            .args(["--exact", "ops::codex_voice_analyst::process::child::tests::forced_exit_bypasses_managed_child_lock_and_kills_descendants", "--nocapture"])
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
                panic!("forced cleanup waited for the managed child lock");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let (mut process, tree) = OwnedProcessTree::spawn(
        Command::new("sh")
            .args(["-c", "sleep 60 & echo ready; wait"])
            .stdout(Stdio::piped()),
    )
    .expect("spawn child process");
    let mut stdout = BufReader::new(process.stdout.take().expect("take owned test resource"));
    stdout
        .read_line(&mut String::new())
        .expect("read child readiness");
    let owner = ChildOwner::new(process, tree);
    let mut held_child = owner.child.lock().expect("lock test state");
    cccc_runtime::force_terminate_owned().expect("terminate all owned processes");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut output = String::new();
        tx.send(stdout.read_to_string(&mut output))
            .expect("send test event");
    });
    rx.recv_timeout(Duration::from_secs(5))
        .expect("descendant survived forced termination")
        .expect("complete expect in fixture");
    assert!(
        !held_child
            .as_mut()
            .expect("access retained child")
            .wait()
            .expect("reap child process")
            .success()
    );
}

#[test]
fn failed_stop_retains_identity_and_can_be_retried() {
    let (process, tree) = OwnedProcessTree::spawn(Command::new("sh").args(["-c", "sleep 60"]))
        .expect("spawn child process");
    let pid = process.id();
    let owner = ChildOwner::new(process, tree);
    // Inject an OS-operation failure at the stop-operation boundary. The
    // child/ownership state and the following retry use the real process path.
    for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::Interrupted] {
        let error = owner
            .stop_with(|_, _| Err(io::Error::new(kind, "injected stop failure")))
            .expect_err("operation must fail in this scenario");
        assert_eq!(error.kind(), kind);
        assert_eq!(owner.id(), Some(pid));
        assert!(owner.running(), "failed stop must retain its running child");
    }
    // A failure after termination but before reaping must also retain Child.
    let error = owner
        .stop_with(|_, tree| {
            tree.terminate()?;
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "injected reap failure",
            ))
        })
        .expect_err("operation must fail in this scenario");
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert_eq!(owner.id(), Some(pid));
    owner.stop().expect("stop owned runtime");
    assert_eq!(owner.id(), None);
    assert_eq!(
        nix::sys::wait::waitpid(
            nix::unistd::Pid::from_raw(pid as i32),
            Some(nix::sys::wait::WaitPidFlag::WNOHANG)
        ),
        Err(nix::errno::Errno::ECHILD),
        "successful retry must reap the original child"
    );
    assert!(!owner.running());
    owner.stop().expect("stop owned runtime");
}
