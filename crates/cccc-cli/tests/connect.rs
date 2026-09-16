use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::Command;

fn command(home: &HomeLayout) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cccc"));
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("CCCC_")) {
        cmd.env_remove(key);
    }
    cmd.env("CCCC_HOME", home.root())
        .env("CCCC_GROUP_ID", "g_actor")
        .env("CCCC_ACTOR_ID", "worker");
    cmd
}
#[test]
fn connect_cli_preserves_actor_scope_qualified_address_and_reply_defaults() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    cccc_core::active::set(&home, "g_someone_else").expect("active group");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("IPC");
    cccc_core::fs::write_json(
        &cccc_daemon::DaemonPaths::new(home.clone()).address,
        &json!({"v":1,"transport":"tcp","path":"","host":"127.0.0.1",
        "port":listener.local_addr().expect("port").port(),"pid":std::process::id(),
        "version":env!("CARGO_PKG_VERSION"),"ts":"2026-09-16T00:00:00Z"}),
    )
    .expect("address");
    let server = std::thread::spawn(move || {
        for stream in listener.incoming().take(4) {
            let stream = stream.expect("accept");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .expect("timeout");
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).expect("request");
            let request: DaemonRequest = serde_json::from_str(&line).expect("JSON");
            writeln!(
                stream.get_mut(),
                "{}",
                json!({"ok":true,"result":{"op":request.op,"args":request.args}})
            )
            .expect("response");
        }
    });
    let invoke = |args: &[&str]| {
        let output = command(&home).args(args).output().expect("CLI");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).expect("result")
    };
    let directory = invoke(&[
        "connect",
        "--instance",
        "i_remote",
        "--target-group",
        "g_remote",
        "--limit",
        "1",
    ]);
    assert_eq!(directory["op"], "connect_catalog");
    assert_eq!(directory["args"]["group_id"], "g_actor");
    assert_eq!(directory["args"]["by"], "worker");
    assert_eq!(directory["args"]["target_group_id"], "g_remote");
    let sent = invoke(&[
        "send",
        "hello",
        "--dst-instance",
        "i_remote",
        "--dst-group",
        "g_remote",
        "--insight",
        "perspective",
        "--idempotency-key",
        "stable",
        "--mode",
        "mail",
    ]);
    assert_eq!(sent["op"], "connect_send");
    assert_eq!(sent["args"]["instance_id"], "i_remote");
    assert_eq!(sent["args"]["target_group_id"], "g_remote");
    assert_eq!(sent["args"]["to"], json!(["@foreman"]));
    assert_eq!(sent["args"]["insight"], "perspective");
    assert_eq!(sent["args"]["client_id"], "stable");
    assert!(sent["args"].get("dst_group_id").is_none());
    let reply = invoke(&[
        "reply",
        "local-event",
        "answer",
        "--insight",
        "perspective",
        "--idempotency-key",
        "reply-key",
    ]);
    assert_eq!(reply["op"], "reply");
    assert_eq!(reply["args"]["reply_to"], "local-event");
    assert_eq!(reply["args"]["client_id"], "reply-key");
    assert!(
        reply["args"].get("to").is_none(),
        "omission preserves original remote participants"
    );
    let local = invoke(&["send", "local", "--group", "g_explicit", "--to", "user"]);
    assert_eq!(local["op"], "message_send");
    assert_eq!(local["args"]["group_id"], "g_explicit");
    server.join().expect("IPC joined");
}

#[test]
fn incomplete_remote_cli_arguments_fail_before_ipc() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    for args in [
        vec!["send", "hello", "--dst-instance", "i_remote"],
        vec!["send", "hello", "--dst-group", "g_remote"],
        vec![
            "send",
            "hello",
            "--dst-instance",
            "i_remote",
            "--dst-group",
            "g_remote",
            "--path",
            ".",
        ],
        vec!["connect", "--target-group", "g_remote"],
    ] {
        let output = command(&home).args(&args).output().expect("CLI");
        assert!(!output.status.success(), "must reject {args:?}");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("daemon unavailable"));
    }
}
