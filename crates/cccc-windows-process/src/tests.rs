use super::*;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn child_cannot_execute_before_resume_and_drop_cleans_up_suspended_launch() {
    let mut command = Command::new("cmd");
    command.args(["/C", "echo resumed"]).stdout(Stdio::piped());
    let mut suspended = spawn_suspended(&mut command, 0x0800_0000).expect("spawn suspended child");
    // Before resume the child cannot emit its very first output instruction.
    let stdout = suspended
        .take_stdout_for_test()
        .expect("complete take stdout for test in fixture");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read child readiness");
        let _ = tx.send(line);
    });
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    let mut child = suspended.resume().expect("resume suspended child");
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5))
            .expect("receive before test deadline")
            .trim(),
        "resumed"
    );
    assert!(child.wait().expect("reap child process").success());

    let mut command = Command::new("cmd");
    command
        .args(["/C", "echo must-not-run"])
        .stdout(Stdio::piped());
    let mut suspended = spawn_suspended(&mut command, 0).expect("spawn suspended child");
    let stdout = suspended
        .take_stdout_for_test()
        .expect("complete take stdout for test in fixture");
    drop(suspended);
    let mut output = String::new();
    use std::io::Read;
    BufReader::new(stdout)
        .read_to_string(&mut output)
        .expect("read fixture contents");
    assert!(
        output.is_empty(),
        "aborted suspended launch executed child code"
    );
}
