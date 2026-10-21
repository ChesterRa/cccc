#![cfg(target_os = "linux")]

use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

async fn call(client: &DaemonClient, op: &str, args: Value) -> Value {
    let response = client
        .call(&DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().expect("request object").clone(),
        })
        .await
        .expect("daemon API response");
    assert!(response.ok, "{op}: {:?}", response.error);
    Value::Object(response.result)
}

#[tokio::test]
async fn inbox_projection_refills_overflow_and_follows_compaction() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let mut daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_cccc"))
            .args(["daemon", "run", "--host", "127.0.0.1", "--port", "0"])
            .env("CCCC_HOME", home.root())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("daemon"),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !home.daemon_dir().join("ccccd.addr.json").exists() {
        assert!(daemon.0.try_wait().expect("status").is_none());
        assert!(Instant::now() < deadline, "startup timed out");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let client = DaemonClient::new(home).with_timeout(Duration::from_secs(30));
    let created = call(
        &client,
        "group_create",
        json!({"title":"Inbox projection","by":"user"}),
    )
    .await;
    let group = created["group"]["group_id"].as_str().expect("group id");
    call(
        &client,
        "actor_add",
        json!({"group_id":group,"actor_id":"reader","runtime":"custom","by":"user"}),
    )
    .await;
    // Exercise the real send operation, its routing/validation, and cursor
    // persistence. RSS proof uses private real-group replays, not filler data.
    let mut ids = Vec::new();
    for task in 0..1005 {
        let result = call(&client,"message_send",json!({"group_id":group,"by":"user","to":["reader"],"message_mode":"mail","text":format!("Please review task {task}.")})).await;
        ids.push(result["event"]["id"].as_str().expect("event id").to_owned());
    }
    let actor_list = call(
        &client,
        "actor_list",
        json!({"group_id":group,"include_unread":true}),
    )
    .await;
    assert_eq!(actor_list["actors"][0]["unread_count"], 1000);
    for start in (0..1000).step_by(200) {
        let read = call(
            &client,
            "inbox_read",
            json!({"group_id":group,"actor_id":"reader","limit":200}),
        )
        .await;
        let actual = read["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|event| event["id"].as_str().expect("id"))
            .collect::<Vec<_>>();
        assert_eq!(actual, ids[start..start + 200]);
    }
    // Rotation reuses ledger.jsonl but changes its inode/source order. The
    // next query must rebuild from one consistent set of pinned descriptors.
    call(
        &client,
        "ledger_compact",
        json!({"group_id":group,"reason":"inbox projection rotation"}),
    )
    .await;
    let peek = call(
        &client,
        "inbox_peek",
        json!({"group_id":group,"actor_id":"reader","limit":10}),
    )
    .await;
    assert_eq!(
        peek["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|event| event["id"].as_str().expect("id"))
            .collect::<Vec<_>>(),
        ids[1000..]
    );
    let actor_list = call(
        &client,
        "actor_list",
        json!({"group_id":group,"include_unread":true}),
    )
    .await;
    assert_eq!(actor_list["actors"][0]["unread_count"], 5);
    call(
        &client,
        "actor_remove",
        json!({"group_id":group,"actor_id":"reader","by":"user"}),
    )
    .await;
    call(
        &client,
        "actor_add",
        json!({"group_id":group,"actor_id":"reader","runtime":"custom","by":"user"}),
    )
    .await;
    let peek = call(
        &client,
        "inbox_peek",
        json!({"group_id":group,"actor_id":"reader","limit":10}),
    )
    .await;
    assert!(
        peek["messages"].as_array().expect("messages").is_empty(),
        "a re-added actor inherited old-generation mail"
    );
}
