//! `cccc actor new-session` forwards `actor_new_session` to the daemon with the
//! same argument names as `cccc actor restart`, prints the full result on
//! success (including `runtime_confirmation_required`) and fails non-zero on a
//! daemon error. The daemon here is a stand-in over a unix socket.
#![cfg(unix)]

use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::io::Write;
use std::process::Stdio;
use tokio::sync::mpsc;

/// Bind a stand-in daemon on a unix socket, pointing the client at it through
/// the address file. Each request line is answered by `reply` and recorded.
fn stand_in(home: &HomeLayout, reply: Value) -> mpsc::UnboundedReceiver<DaemonRequest> {
    std::fs::create_dir_all(home.daemon_dir()).expect("daemon directory");
    let socket = home.daemon_dir().join("stand-in.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).expect("stand-in");
    std::fs::write(
        cccc_client::address_path(home),
        json!({"v":1,"transport":"unix","path":socket,"pid":std::process::id(),
            "version":"stand-in","ts":"2026-11-09T00:00:00Z"})
        .to_string(),
    )
    .expect("stand-in address");
    let (send, recv) = mpsc::unbounded_channel();
    let reply = serde_json::to_string(&reply).expect("reply") + "\n";
    std::thread::spawn(move || {
        while let Ok((stream, _)) = listener.accept() {
            let send = send.clone();
            let reply = reply.clone();
            std::thread::spawn(move || {
                let (mut read, mut write) = (stream.try_clone().expect("clone"), stream);
                let mut line = String::new();
                let mut reader = std::io::BufReader::new(&mut read);
                while std::io::BufRead::read_line(&mut reader, &mut line).unwrap_or(0) > 0 {
                    let request: DaemonRequest =
                        serde_json::from_str(line.trim()).expect("request");
                    let _ = send.send(request);
                    if write.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                    line.clear();
                }
            });
        }
    });
    recv
}

fn run(home: &HomeLayout, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_cccc"))
        .args(args)
        .env("CCCC_HOME", home.root())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("cccc run")
}

#[test]
fn new_session_forwards_the_op_and_args_and_prints_the_full_result() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    std::fs::write(
        home.root().join("active.json"),
        json!({"v":1,"active_group_id":"g_test"}).to_string(),
    )
    .expect("current group");
    let reply = json!({"v":1,"ok":true,"result":{
        "actor_id":"stage-actor",
        "session_id":"sess-1",
        "started":true,
        "runtime_confirmation_required":true
    }});
    let mut requests = stand_in(&home, reply);
    let output = run(&home, &["actor", "new-session", "stage-actor"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = requests.try_recv().expect("one daemon request");
    assert_eq!(request.op, "actor_new_session");
    assert_eq!(request.args["actor_id"], "stage-actor");
    assert_eq!(request.args["group_id"], "g_test");
    assert_eq!(request.args["by"], "user");
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.contains("\"runtime_confirmation_required\": true"),
        "{stdout}"
    );
}

#[test]
fn new_session_accepts_group_and_by_overrides() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let reply = json!({"v":1,"ok":true,"result":{"actor_id":"stage-actor","started":true}});
    let mut requests = stand_in(&home, reply);
    let output = run(
        &home,
        &[
            "actor",
            "new-session",
            "stage-actor",
            "--group",
            "g_other",
            "--by",
            "operator",
        ],
    );
    assert!(output.status.success());
    let request = requests.try_recv().expect("one daemon request");
    assert_eq!(request.args["group_id"], "g_other");
    assert_eq!(request.args["by"], "operator");
}

#[test]
fn a_daemon_error_fails_and_reports_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let reply =
        json!({"v":1,"ok":false,"error":{"code":"actor_not_found","message":"no such actor"}});
    let _requests = stand_in(&home, reply);
    let output = run(
        &home,
        &["actor", "new-session", "ghost", "--group", "g_test"],
    );
    assert!(!output.status.success(), "expected failure");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("actor_not_found"), "{stderr}");
}
