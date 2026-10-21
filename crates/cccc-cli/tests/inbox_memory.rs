#![cfg(target_os = "linux")]

use cccc_client::DaemonClient;
use cccc_contracts::{Actor, DaemonRequest, Event};
use cccc_core::{GroupStore, HomeLayout};
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

fn rss_kib(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .expect("child process status")
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .expect("child VmRSS")
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
async fn mail_peek_and_consume_do_not_retain_the_whole_ledger() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("bounded inbox", "").expect("group");
    let mut actor = Actor::new("reader");
    actor.enabled = false;
    group.actors.push(actor);
    store.save(&group).expect("save group");
    let mut daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_cccc"))
            .args(["daemon", "run", "--host", "127.0.0.1", "--port", "0"])
            .env("CCCC_HOME", home.root())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start daemon"),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !home.daemon_dir().join("ccccd.sock").exists() {
        assert!(
            daemon.0.try_wait().expect("daemon status").is_none(),
            "daemon exited"
        );
        assert!(Instant::now() < deadline, "daemon startup timed out");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let client = DaemonClient::new(home).with_timeout(Duration::from_secs(30));
    // Generated content only: no production ledger or private message text.
    // All records go through the daemon's API. This leaves its query index
    // cold, making a peek's hidden whole-ledger allocation visible.
    let payload = "x".repeat(32 * 1024);
    let mut first_ids = Vec::new();
    for number in 0..2048 {
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "user".into();
        event.data = json!({"to":["reader"],"message_mode":"mail","text":payload})
            .as_object()
            .expect("event data")
            .clone();
        if number < 2 {
            first_ids.push(event.id.clone());
        }
        call(&client, "event_append", json!({"event":event})).await;
    }
    let before = rss_kib(daemon.0.id());
    let args = json!({"group_id":group.group_id,"actor_id":"reader","limit":1});
    let peek = call(&client, "inbox_peek", args.clone()).await;
    assert_eq!(peek["messages"][0]["id"], first_ids[0]);
    assert_eq!(peek["messages"].as_array().expect("messages").len(), 1);
    let consumed = call(&client, "inbox_read", args.clone()).await;
    assert_eq!(consumed["messages"][0]["id"], first_ids[0]);
    let next = call(&client, "inbox_peek", args).await;
    assert_eq!(next["messages"][0]["id"], first_ids[1]);
    let after = rss_kib(daemon.0.id());
    // The 64 MiB ledger must not become a retained parsed Event index for
    // a one-record page. Allow allocator/thread noise without allowing the
    // whole payload to fit below the assertion.
    assert!(
        after.saturating_sub(before) < 32 * 1024,
        "bounded Mail page retained {} KiB (before={before}, after={after})",
        after.saturating_sub(before)
    );
}
