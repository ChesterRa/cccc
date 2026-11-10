//! `cccc ledger search` forwards the existing `ledger_search` filters by their
//! daemon argument names, prints the daemon result, and rejects invalid
//! arguments before IPC. The daemon here is a stand-in over a unix socket.
#![cfg(unix)]

use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::time::Duration;

/// Bind a stand-in daemon on a unix socket, pointing the client at it through
/// the address file. Each request line is recorded and answered with `reply`.
fn stand_in(home: &HomeLayout, reply: Value) -> mpsc::Receiver<DaemonRequest> {
    std::fs::create_dir_all(home.daemon_dir()).expect("daemon directory");
    let socket = home.daemon_dir().join("stand-in.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).expect("stand-in");
    std::fs::write(
        cccc_client::address_path(home),
        json!({"v":1,"transport":"unix","path":socket,"pid":std::process::id(),
            "version":"stand-in","ts":"2026-11-10T00:00:00Z"})
        .to_string(),
    )
    .expect("stand-in address");
    let (send, recv) = mpsc::channel();
    let reply = serde_json::to_string(&reply).expect("reply") + "\n";
    std::thread::spawn(move || {
        while let Ok((stream, _)) = listener.accept() {
            let send = send.clone();
            let reply = reply.clone();
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stream.try_clone().expect("clone"));
                let mut writer = stream;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let request: DaemonRequest =
                        serde_json::from_str(line.trim()).expect("request");
                    let _ = send.send(request);
                    if writer.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                }
            });
        }
    });
    recv
}

fn run(home: &HomeLayout, args: &[&str]) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_cccc"));
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("CCCC_")) {
        command.env_remove(key);
    }
    command
        .args(args)
        .env("CCCC_HOME", home.root())
        .output()
        .expect("cccc run")
}

#[test]
fn search_forwards_every_filter_and_prints_the_result() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let requests = stand_in(
        &home,
        json!({"v":1,"ok":true,"result":{
            "count":1,
            "events":[{"id":"evt-1","kind":"chat.message"}],
            "has_more":false
        }}),
    );

    let output = run(
        &home,
        &[
            "ledger",
            "search",
            "--group",
            "g_other",
            "--by",
            "sender",
            "--by-any",
            "alice",
            "--by-any",
            "bob",
            "--to",
            "recipient",
            "--since",
            "2026-11-01T00:00:00Z",
            "--until",
            "2026-11-02T00:00:00Z",
            "--kind",
            "chat",
            "-q",
            "deploy",
            "--before",
            "evt-before",
            "-n",
            "25",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("one daemon request");
    assert_eq!(request.op, "ledger_search");
    assert_eq!(
        request.args,
        json!({
            "group_id":"g_other",
            "by":"sender",
            "by_any":["alice","bob"],
            "to":"recipient",
            "since_ts":"2026-11-01T00:00:00Z",
            "until_ts":"2026-11-02T00:00:00Z",
            "kind":"chat",
            "q":"deploy",
            "before":"evt-before",
            "limit":25u64
        })
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("\"count\": 1"), "{stdout}");
    assert!(stdout.contains("\"id\": \"evt-1\""), "{stdout}");
    assert!(stdout.contains("\"has_more\": false"), "{stdout}");
}

#[test]
fn search_uses_the_active_group_and_page_defaults() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    cccc_core::active::set(&home, "g_test").expect("active group");
    let requests = stand_in(&home, json!({"v":1,"ok":true,"result":{}}));

    let output = run(&home, &["ledger", "search"]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("one daemon request");
    assert_eq!(request.op, "ledger_search");
    assert_eq!(
        request.args,
        json!({"group_id":"g_test","kind":"all","limit":50u64})
    );
}

#[test]
fn search_forwards_the_after_cursor_and_query() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    cccc_core::active::set(&home, "g_test").expect("active group");
    let requests = stand_in(&home, json!({"v":1,"ok":true,"result":{}}));

    let output = run(
        &home,
        &[
            "ledger",
            "search",
            "--after",
            "evt-after",
            "--to",
            "operator",
            "-q",
            "deploy",
            "-n",
            "5",
        ],
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let request = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("one daemon request");
    assert_eq!(
        request.args,
        json!({
            "group_id":"g_test",
            "kind":"all",
            "limit":5u64,
            "after":"evt-after",
            "to":"operator",
            "q":"deploy"
        })
    );
}

#[test]
fn search_rejects_invalid_arguments_before_ipc() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");

    for args in [
        vec!["ledger", "search", "--kind", "recent"],
        vec!["ledger", "search", "--limit", "many"],
    ] {
        let output = run(&home, &args);
        assert!(!output.status.success(), "must reject {args:?}");
        let stderr = String::from_utf8(output.stderr).expect("stderr");
        assert!(stderr.contains("invalid value"), "{args:?}: {stderr}");
        assert!(
            !stderr.contains("daemon unavailable"),
            "{args:?}: parsed but reached IPC: {stderr}"
        );
    }
}
