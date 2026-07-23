#![cfg(unix)]

use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::{GroupStore, HomeLayout, ledger};
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
            "command":["sh","-c","stty -echo; IFS= read -r preamble; IFS= read -r first; IFS= read -r second; IFS= read -r third; IFS= read -r fourth; printf 'PREAMBLE:%s\\nFIRST:%s\\nSECOND:%s\\nTHIRD:%s\\nFOURTH:%s' \"$preamble\" \"$first\" \"$second\" \"$third\" \"$fourth\"; sleep 2"],
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
    assert!(text.contains("PREAMBLE:[CCCC] You are peer1"));
    assert!(text.contains("FIRST:[cccc] user → peer1: one"));
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

    let actor_message = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"lead","to":[],"text":"status update"}),
    );
    assert_eq!(actor_message.result["event"]["data"]["to"], json!(["user"]));
}

#[test]
fn replies_default_to_the_original_audience_and_reject_self_delivery() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"reply-recipient-test","by":"user"}),
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

    let user_message = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["lead"],"text":"question"}),
    );
    let user_message_id = user_message.result["event"]["id"]
        .as_str()
        .expect("event id");
    let default_reply = call(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"lead","to":[],
            "reply_to":user_message_id,"text":"answer"
        }),
    );
    assert_eq!(default_reply.result["event"]["data"]["to"], json!(["user"]));

    let self_reply = call_raw(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"lead","to":["lead"],
            "reply_to":user_message_id,"text":"wrong target"
        }),
    );
    assert!(!self_reply.ok);
    assert_eq!(
        self_reply.error.as_ref().map(|error| error.code.as_str()),
        Some("no_enabled_recipients")
    );

    let lead_message = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"lead","to":["peer1"],"text":"update"}),
    );
    let own_message_reply = call(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"lead",
            "reply_to":lead_message.result["event"]["id"],"text":"follow-up"
        }),
    );
    assert_eq!(
        own_message_reply.result["event"]["data"]["to"],
        json!(["peer1"])
    );

    let explicit_reply = call(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"lead","to":["peer1"],
            "reply_to":user_message_id,"text":"ask peer"
        }),
    );
    assert_eq!(
        explicit_reply.result["event"]["data"]["to"],
        json!(["peer1"])
    );
}

#[test]
fn peer_insight_gate_validates_before_persisting_and_exempts_user_only_messages() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"peer-insight-test","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"lead","by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );

    let missing = call_raw(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"lead","to":["peer1"],"text":"work",
            "require_peer_insight":true
        }),
    );
    assert!(!missing.ok);
    assert_eq!(
        missing.error.as_ref().map(|error| error.code.as_str()),
        Some("peer_insight_required")
    );
    assert_eq!(
        missing.error.expect("peer insight error").details["new_side_effects"],
        false
    );

    let user_only = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"lead","to":["user"],"text":"status",
            "require_peer_insight":true
        }),
    );
    assert!(user_only.ok);

    let accepted = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"lead","to":["peer1"],"text":"work",
            "insight":"  reconsider the dependency boundary  ","require_peer_insight":true
        }),
    );
    assert!(accepted.ok);
    assert_eq!(
        accepted.result["event"]["data"]["insight"],
        "reconsider the dependency boundary"
    );
    assert!(
        accepted.result["event"]["data"]
            .get("require_peer_insight")
            .is_none()
    );

    call(
        &home,
        "actor_update",
        json!({"group_id":group_id,"actor_id":"peer1","patch":{"enabled":false},"by":"user"}),
    );
    let disabled = call_raw(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"lead","to":["peer1"],"text":"wake and review",
            "require_peer_insight":true
        }),
    );
    assert_eq!(
        disabled.error.as_ref().map(|error| error.code.as_str()),
        Some("peer_insight_required")
    );
}

#[test]
fn cross_group_peer_insight_gate_precedes_both_ledger_writes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let source = call(&home, "group_create", json!({"title":"source","by":"user"}));
    let destination = call(
        &home,
        "group_create",
        json!({"title":"destination","by":"user"}),
    );
    let source_id = source.result["group"]["group_id"]
        .as_str()
        .expect("source id");
    let destination_id = destination.result["group"]["group_id"]
        .as_str()
        .expect("destination id");
    call(
        &home,
        "actor_add",
        json!({"group_id":destination_id,"actor_id":"reviewer","by":"user"}),
    );
    let store = GroupStore::new(home.clone()).expect("store");
    let source_ledger = store.ledger_path(source_id).expect("source ledger");
    let destination_ledger = store
        .ledger_path(destination_id)
        .expect("destination ledger");
    let before_source = ledger::read_all(&source_ledger)
        .expect("source events")
        .len();
    let before_destination = ledger::read_all(&destination_ledger)
        .expect("destination events")
        .len();

    let rejected = call_raw(
        &home,
        "send_cross_group",
        json!({
            "group_id":source_id,"dst_group_id":destination_id,"by":"user",
            "to":["reviewer"],"text":"review this","require_peer_insight":true
        }),
    );
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("peer_insight_required")
    );
    assert_eq!(
        ledger::read_all(&source_ledger)
            .expect("source events")
            .len(),
        before_source
    );
    assert_eq!(
        ledger::read_all(&destination_ledger)
            .expect("destination events")
            .len(),
        before_destination
    );

    let accepted = call(
        &home,
        "send_cross_group",
        json!({
            "group_id":source_id,"dst_group_id":destination_id,"by":"user",
            "to":["reviewer"],"text":"review this","insight":"check the outcome",
            "require_peer_insight":true,"client_id":"cross-group-1"
        }),
    );
    assert!(
        accepted.result["source_event"]["data"]
            .get("require_peer_insight")
            .is_none()
    );
    assert!(
        accepted.result["event"]["data"]
            .get("require_peer_insight")
            .is_none()
    );
    let source_after_accept = ledger::read_all(&source_ledger)
        .expect("source events")
        .len();
    let destination_after_accept = ledger::read_all(&destination_ledger)
        .expect("destination events")
        .len();
    let replay = call(
        &home,
        "send_cross_group",
        json!({
            "group_id":source_id,"dst_group_id":destination_id,"by":"user",
            "to":["reviewer"],"text":"changed retry body","require_peer_insight":true,
            "client_id":"cross-group-1"
        }),
    );
    assert_eq!(replay.result["duplicate"], true);
    assert_eq!(
        ledger::read_all(&source_ledger)
            .expect("source events")
            .len(),
        source_after_accept
    );
    assert_eq!(
        ledger::read_all(&destination_ledger)
            .expect("destination events")
            .len(),
        destination_after_accept
    );
}

#[test]
fn remote_cross_group_record_validates_insight_before_source_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let source = call(
        &home,
        "group_create",
        json!({"title":"remote-source","by":"user"}),
    );
    let source_id = source.result["group"]["group_id"]
        .as_str()
        .expect("source id");
    let store = GroupStore::new(home.clone()).expect("store");
    let source_ledger = store.ledger_path(source_id).expect("source ledger");
    let before = ledger::read_all(&source_ledger)
        .expect("source events")
        .len();

    let rejected = call_raw(
        &home,
        "send_cross_group_remote_record",
        json!({
            "group_id":source_id,"dst_group_id":"remote-group","by":"user",
            "to":["reviewer"],"text":"review this","require_peer_insight":true
        }),
    );
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("peer_insight_required")
    );
    assert_eq!(
        ledger::read_all(&source_ledger)
            .expect("source events")
            .len(),
        before
    );
}

async fn wait_for(client: &DaemonClient, group_id: &str, expected: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
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
            "PTY did not receive {expected:?}; tail={:?}",
            tail.result["text"].as_str().unwrap_or_default()
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
    let response = call_raw(home, op, args);
    assert!(
        response.ok,
        "{op} failed: {:?}",
        response.error.as_ref().map(|error| &error.message)
    );
    response
}

fn call_raw(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let request = DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().cloned().unwrap_or_else(Map::new),
    };
    cccc_daemon::handle_request(home, &request)
}
