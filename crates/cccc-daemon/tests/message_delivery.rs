#![cfg(unix)]

use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::time::Duration;

#[tokio::test]
async fn serializes_delivery_notifies_and_advances_cursor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let daemon = tokio::spawn(cccc_daemon::run(home.clone()));
    wait_until(|| cccc_daemon::DaemonPaths::new(home.clone()).address.exists()).await;
    let client = DaemonClient::new(home.clone());
    let created = daemon_call(
        &client,
        "group_create",
        json!({"title":"message-delivery-test","by":"user"}),
    )
    .await;
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    daemon_call(
        &client,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runner":"pty",
            "runtime":"custom",
            "submit":"newline",
            "command":["sh","-c","stty -echo; IFS= read -r first; IFS= read -r second; IFS= read -r third; IFS= read -r fourth; printf 'FIRST:%s\\nSECOND:%s\\nTHIRD:%s\\nFOURTH:%s' \"$first\" \"$second\" \"$third\" \"$fourth\"; sleep 2"],
            "by":"user"
        }),
    )
    .await;
    daemon_call(
        &client,
        "actor_start",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;

    let first = daemon_call(
        &client,
        "send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"one"}),
    )
    .await;
    let second = daemon_call(
        &client,
        "tracked_send",
        json!({"group_id":group_id,"by":"user","to":["peer1"],"text":"two"}),
    )
    .await;
    let notify = daemon_call(
        &client,
        "system_notify",
        json!({"group_id":group_id,"by":"system","to":["peer1"],"text":"notice"}),
    )
    .await;
    let reply = daemon_call(
        &client,
        "reply",
        json!({
            "group_id":group_id,
            "by":"user",
            "to":["peer1"],
            "reply_to":first.result["event"]["id"],
            "text":"fix it"
        }),
    )
    .await;
    assert_eq!(first.result["delivery"]["state"], "queued");
    assert_eq!(second.result["delivery"]["queued"], 1);
    assert_eq!(notify.result["delivery"]["state"], "queued");
    assert_eq!(reply.result["delivery"]["state"], "queued");

    wait_for(&client, &group_id, "FOURTH:[cccc] user → peer1 (reply:").await;
    let tail = daemon_call(
        &client,
        "terminal_tail",
        json!({"group_id":group_id,"actor_id":"peer1"}),
    )
    .await;
    let text = tail.result["text"].as_str().unwrap_or_default();
    assert!(text.contains("SECOND:[cccc] user → peer1: two"));
    assert!(text.contains("THIRD:[cccc] SYSTEM (info): notice"));
    assert!(text.contains("FOURTH:[cccc] user → peer1 (reply:"));
    assert!(text.contains("> \"one\": fix it"));

    wait_until_async(|| async {
        let inbox = daemon_call(
            &client,
            "inbox_list",
            json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
        )
        .await;
        inbox.result["messages"]
            .as_array()
            .is_some_and(Vec::is_empty)
    })
    .await;
    let inbox = daemon_call(
        &client,
        "inbox_list",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    )
    .await;
    assert_eq!(inbox.result["messages"].as_array().map(Vec::len), Some(0));

    daemon_call(&client, "shutdown", json!({})).await;
    tokio::time::timeout(Duration::from_secs(5), daemon)
        .await
        .expect("daemon shutdown timeout")
        .expect("daemon task")
        .expect("daemon result");
}

#[test]
fn empty_recipients_follow_the_group_default_policy() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"default-recipient-test","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    for actor_id in ["lead", "peer1"] {
        call(
            &home,
            "actor_add",
            json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
        );
    }

    let default_send = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":[],"text":"foreman only"}),
    );
    assert_eq!(
        default_send.result["event"]["data"]["to"],
        json!(["@foreman"])
    );

    call(
        &home,
        "group_settings_update",
        json!({"group_id":group_id,"by":"user","patch":{"default_send_to":"broadcast"}}),
    );
    let broadcast = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":[],"text":"everyone"}),
    );
    assert_eq!(broadcast.result["event"]["data"]["to"], json!(["@all"]));
}

async fn wait_for(client: &DaemonClient, group_id: &str, expected: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(7);
    loop {
        let tail = daemon_call(
            client,
            "terminal_tail",
            json!({"group_id":group_id,"actor_id":"peer1"}),
        )
        .await;
        if tail.result["text"]
            .as_str()
            .unwrap_or_default()
            .contains(expected)
        {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "PTY did not receive {expected:?}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn daemon_call(client: &DaemonClient, op: &str, args: Value) -> DaemonResponse {
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_else(Map::new),
    };
    let response = client.call(&request).await.expect("daemon request");
    assert!(
        response.ok,
        "{op} failed: {:?}",
        response.error.as_ref().map(|error| &error.message)
    );
    response
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition timed out"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_until_async<F, Fut>(mut condition: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(7);
    while !condition().await {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition timed out"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_else(Map::new),
    };
    let response = cccc_daemon::handle_request(home, &request);
    assert!(
        response.ok,
        "{op} failed: {:?}",
        response.error.as_ref().map(|error| &error.message)
    );
    response
}
