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

fn summary_row<'a>(summary: &'a Value, actor_id: &str) -> &'a Value {
    summary["actors"]
        .as_array()
        .expect("actors")
        .iter()
        .find(|actor| actor["actor_id"] == actor_id)
        .expect("actor row")
}

#[tokio::test]
async fn overdue_mail_summary_is_passive_authorized_and_generation_scoped() {
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
    let client = DaemonClient::new(home.clone()).with_timeout(Duration::from_secs(30));
    let created = call(
        &client,
        "group_create",
        json!({"title":"Mail summary","by":"user"}),
    )
    .await;
    let group = created["group"]["group_id"].as_str().expect("group");
    // These Actors are never started. The first visible Actor is Foreman.
    for actor in ["coordinator", "reader", "other"] {
        call(
            &client,
            "actor_add",
            json!({"group_id":group,"actor_id":actor,"runtime":"custom","by":"user"}),
        )
        .await;
    }
    let sent = call(&client, "message_send", json!({"group_id":group,"by":"user","to":["reader"],"message_mode":"mail","text":"Please review this change."})).await;
    let id = sent["event"]["id"].as_str().expect("id");
    let timestamp = sent["event"]["ts"].as_str().expect("original timestamp");
    let before = call(
        &client,
        "inbox_summary",
        json!({"group_id":group,"cutoff":"1970-01-01T00:00:00Z"}),
    )
    .await;
    assert_eq!(summary_row(&before, "reader")["awaiting_count"], 1);
    assert_eq!(summary_row(&before, "reader")["overdue_count"], 0);
    let summary = call(
        &client,
        "inbox_summary",
        json!({"group_id":group,"cutoff":timestamp}),
    )
    .await;
    assert_eq!(summary["actors"].as_array().unwrap().len(), 3);
    assert_eq!(
        summary_row(&summary, "reader")["overdue_count"],
        1,
        "cutoff equality is inclusive"
    );
    assert_eq!(summary_row(&summary, "coordinator")["awaiting_count"], 0);
    assert_eq!(summary_row(&summary, "other")["awaiting_count"], 0);
    let after = call(
        &client,
        "inbox_summary",
        json!({"group_id":group,"cutoff":summary["observed_at"],"by":"coordinator"}),
    )
    .await;
    assert_eq!(summary_row(&after, "reader")["overdue_count"], 1);
    assert_eq!(
        summary_row(&after, "reader")["oldest_awaiting_at"],
        summary_row(&summary, "reader")["oldest_awaiting_at"]
    );
    for by in ["reader", "unknown"] {
        let response = client
            .call(&DaemonRequest {
                v: 1,
                op: "inbox_summary".into(),
                args: json!({"group_id":group,"cutoff":timestamp,"by":by})
                    .as_object()
                    .unwrap()
                    .clone(),
            })
            .await
            .unwrap();
        assert!(!response.ok, "{by} obtained a Group-wide Inbox view");
    }
    for cutoff in ["not-a-time", "9999-01-01T00:00:00Z"] {
        let response = client
            .call(&DaemonRequest {
                v: 1,
                op: "inbox_summary".into(),
                args: json!({"group_id":group,"cutoff":cutoff})
                    .as_object()
                    .unwrap()
                    .clone(),
            })
            .await
            .unwrap();
        assert!(!response.ok);
    }
    let peek = call(
        &client,
        "inbox_peek",
        json!({"group_id":group,"actor_id":"reader"}),
    )
    .await;
    assert_eq!(peek["messages"][0]["id"], id, "summary consumed Mail");
    let disabled = call(
        &client,
        "actor_update",
        json!({"group_id":group,"actor_id":"reader","patch":{"enabled":false},"by":"user"}),
    )
    .await;
    assert_eq!(disabled["actor"]["enabled"], false);
    let disabled_summary = call(
        &client,
        "inbox_summary",
        json!({"group_id":group,"cutoff":timestamp}),
    )
    .await;
    assert_eq!(
        summary_row(&disabled_summary, "reader")["awaiting_count"],
        1
    );
    call(&client, "reply", json!({"group_id":group,"by":"reader","to":["user"],"reply_to":id,"message_mode":"send","text":"Reviewed."})).await;
    let replied = call(
        &client,
        "inbox_summary",
        json!({"group_id":group,"cutoff":timestamp}),
    )
    .await;
    assert_eq!(summary_row(&replied, "reader")["awaiting_count"], 0);
    assert_eq!(
        call(
            &client,
            "inbox_peek",
            json!({"group_id":group,"actor_id":"reader"})
        )
        .await["messages"][0]["id"],
        id,
        "a reply is not an Inbox read"
    );
    call(&client, "message_send", json!({"group_id":group,"by":"user","to":["reader"],"message_mode":"mail","text":"Please check the next change."})).await;
    call(
        &client,
        "inbox_read",
        json!({"group_id":group,"actor_id":"reader"}),
    )
    .await;
    let read = call(
        &client,
        "inbox_summary",
        json!({"group_id":group,"cutoff":timestamp}),
    )
    .await;
    assert_eq!(summary_row(&read, "reader")["awaiting_count"], 0);
    call(&client, "message_send", json!({"group_id":group,"by":"user","to":["reader"],"message_mode":"mail","text":"Review after the next session."})).await;
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
    let generation = call(
        &client,
        "inbox_summary",
        json!({"group_id":group,"cutoff":timestamp}),
    )
    .await;
    assert_eq!(summary_row(&generation, "reader")["awaiting_count"], 0);
    let current = call(&client, "message_send", json!({"group_id":group,"by":"user","to":["reader"],"message_mode":"mail","text":"Review in this session."})).await;
    let current_cutoff = current["event"]["ts"].as_str().unwrap();
    let warm = call(
        &client,
        "inbox_summary",
        json!({"group_id":group,"cutoff":current_cutoff}),
    )
    .await;
    call(
        &client,
        "ledger_compact",
        json!({"group_id":group,"reason":"summary snapshot rebuild"}),
    )
    .await;
    let rebuilt = call(
        &client,
        "inbox_summary",
        json!({"group_id":group,"cutoff":current_cutoff}),
    )
    .await;
    assert_eq!(summary_row(&warm, "reader")["awaiting_count"], 1);
    assert_eq!(summary_row(&rebuilt, "reader")["awaiting_count"], 1);
    assert_eq!(summary_row(&rebuilt, "reader")["overdue_count"], 1);
    // Exercise the public CLI entry point against the same real daemon.
    let output = Command::new(env!("CARGO_BIN_EXE_cccc"))
        .args([
            "inbox-summary",
            "--group",
            group,
            "--cutoff",
            current_cutoff,
        ])
        .env("CCCC_HOME", home.root())
        .output()
        .expect("CLI summary");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cli: Value = serde_json::from_slice(&output.stdout).expect("CLI JSON");
    assert_eq!(summary_row(&cli, "reader")["overdue_count"], 1);
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
