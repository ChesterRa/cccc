use super::*;
use std::io::{BufRead, BufReader, Read};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn job_termination_kills_descendants_while_child_lock_is_held() {
    let (mut child, owner) = OwnedProcessTree::spawn(
        Command::new("cmd")
            .args(["/C", "ping -n 60 127.0.0.1"])
            .stdout(Stdio::piped()),
    )
    .expect("spawn child process");
    let mut stdout = BufReader::new(child.stdout.take().expect("take owned test resource"));
    let ready_bytes = stdout
        .read_line(&mut String::new())
        .expect("read descendant readiness");
    assert!(
        ready_bytes > 0,
        "ping must start before testing Job closure"
    );
    let child = Mutex::new(child);
    let mut held_child = child.lock().expect("lock test state");
    assert!(
        owner
            .try_wait(|| held_child.try_wait())
            .expect("poll live parent")
            .is_none(),
        "cmd must still be running before Job closure"
    );
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = tx.send(stdout.read_to_end(&mut output));
    });
    assert!(
        matches!(
            rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "the live process tree must keep stdout open before Job closure"
    );
    owner.terminate().expect("terminate owned process tree");
    rx.recv_timeout(Duration::from_secs(5))
        .expect("cmd or ping survived Job closure")
        .expect("drain stdout after Job closure");
    // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE promises termination, not a nonzero
    // exit code. In Windows CI the terminated cmd returned zero. Verify the
    // running -> exited transition directly, with no exit-code assumption.
    let deadline = Instant::now() + Duration::from_secs(5);
    while held_child
        .try_wait()
        .expect("poll terminated parent")
        .is_none()
    {
        assert!(Instant::now() < deadline, "cmd survived Job closure");
        std::thread::sleep(Duration::from_millis(10));
    }
    held_child.wait().expect("reap terminated parent");
    owner.terminate().expect("terminate owned process tree");
}
