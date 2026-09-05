use super::*;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

#[test]
fn forced_launcher_exit_terminates_owned_tree_without_destructors() {
    const CHILD: &str = "CCCC_TEST_FORCED_LAUNCHER";
    if std::env::var_os(CHILD).is_some() {
        let (_child, tree) = cccc_runtime::OwnedProcessTree::spawn(
            Command::new("sh")
                .args([
                    "-c",
                    "sleep 60 & printf 'CCCC_FORCE_READY:%s\\n' \"$$\"; wait",
                ])
                .stdout(Stdio::inherit()),
        )
        .expect("spawn child process");
        // The terminator remains inaccessible behind an ordinary owner lock;
        // process::exit must work without taking that lock or running Drop.
        let owner = std::sync::Mutex::new(tree);
        let _held_owner = owner.lock().expect("lock test state");
        let mut go = String::new();
        std::io::stdin()
            .read_line(&mut go)
            .expect("read child readiness");
        force_exit();
    }
    let mut launcher = Command::new(std::env::current_exe().expect("locate test executable"))
        .args(["--exact", "shutdown::process_tests::forced_launcher_exit_terminates_owned_tree_without_destructors", "--nocapture"])
        .env(CHILD, "1").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("spawn child process");
    let stdout = launcher.stdout.take().expect("take owned test resource");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (eof_tx, eof_rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.expect("fixture value must be available");
            if let Some(pid) = line.strip_prefix("CCCC_FORCE_READY:") {
                let _ = ready_tx.send(pid.parse::<i32>().expect("fixture value must be available"));
            }
        }
        let _ = eof_tx.send(());
    });
    let pid = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("actor ready");
    launcher
        .stdin
        .take()
        .expect("take owned test resource")
        .write_all(b"go\n")
        .expect("write process input");
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = launcher.try_wait().expect("poll child process") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = launcher.kill();
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
            launcher.wait().expect("reap child process");
            panic!("forced launcher exit blocked");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let closed = eof_rx.recv_timeout(Duration::from_secs(5));
    if closed.is_err() {
        let _ = nix::sys::signal::killpg(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    assert_eq!(status.code(), Some(INTERRUPTED_EXIT_CODE));
    closed.expect("actor or descendant survived process::exit and still owns stdout");
}
