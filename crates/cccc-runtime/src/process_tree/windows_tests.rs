use super::*;
use std::io::{BufRead, BufReader, Read};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn job_termination_kills_descendants_while_child_lock_is_held() {
    let (mut child, owner) = OwnedProcessTree::spawn(
        Command::new("cmd")
            .args(["/C", "ping -n 60 127.0.0.1"])
            .stdout(Stdio::piped()),
    )
    .expect("spawn child process");
    let mut stdout = BufReader::new(child.stdout.take().expect("take owned test resource"));
    stdout
        .read_line(&mut String::new())
        .expect("read child readiness");
    let child = Mutex::new(child);
    let mut held_child = child.lock().expect("lock test state");
    owner.terminate().expect("terminate owned process tree");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let _ = tx.send(stdout.read_to_end(&mut output));
    });
    rx.recv_timeout(Duration::from_secs(5))
        .expect("cmd or ping survived Job closure")
        .expect("complete expect in fixture");
    assert!(!held_child.wait().expect("reap child process").success());
    owner.terminate().expect("terminate owned process tree");
}
