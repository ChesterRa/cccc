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
    assert_eq!(notify.result["event"]["data"]["im_visibility"], "internal");
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
fn send_files_uses_the_active_scope_and_normal_send_contract() {
    use cccc_contracts::GroupState;

    let temp = tempfile::tempdir().expect("tempdir");
    let scope = temp.path().join("scope");
    let outside = temp.path().join("outside.txt");
    std::fs::create_dir_all(&scope).expect("scope");
    std::fs::write(scope.join("frame.png"), b"\x89PNG\r\n\x1a\nfixture").expect("image");
    std::fs::write(scope.join("brief.txt"), b"reader brief").expect("brief");
    std::fs::write(&outside, b"outside").expect("outside");

    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"send-files","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":scope,"by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"worker","by":"user"}),
    );
    let store = GroupStore::new(home.clone()).expect("store");
    store
        .mutate(group_id, |group| {
            group.state = GroupState::Idle;
            Ok(())
        })
        .expect("idle");

    let sent = call(
        &home,
        "send_files",
        json!({
            "group_id":group_id,
            "by":"user",
            "to":["worker"],
            "paths":[scope.join("frame.png"), "brief.txt"]
        }),
    );
    let event = &sent.result["event"];
    assert_eq!(event["kind"], "chat.message");
    assert_eq!(event["data"]["text"], "[files] frame.png, brief.txt");
    assert_eq!(event["data"]["to"], json!(["worker"]));
    assert_eq!(
        event["data"]["scope_key"],
        store.load(group_id).expect("group").active_scope_key
    );
    let attachments = event["data"]["attachments"]
        .as_array()
        .expect("attachments");
    assert_eq!(attachments.len(), 2);
    assert_eq!(attachments[0]["kind"], "image");
    assert_eq!(attachments[0]["title"], "frame.png");
    assert_eq!(attachments[0]["mime_type"], "image/png");
    assert_eq!(attachments[1]["kind"], "file");
    assert_eq!(attachments[1]["title"], "brief.txt");
    for attachment in attachments {
        let blob = cccc_core::blobs::resolve(
            &home,
            group_id,
            attachment["path"].as_str().expect("blob path"),
        )
        .expect("blob");
        assert!(blob.is_file());
        assert_eq!(
            std::fs::metadata(blob).expect("metadata").len(),
            attachment["bytes"].as_u64().expect("bytes")
        );
    }
    assert_eq!(
        store.load(group_id).expect("group").state,
        GroupState::Active
    );

    let before = ledger::read_all(&store.ledger_path(group_id).expect("ledger"))
        .expect("events")
        .len();
    let rejected = call_raw(
        &home,
        "send_files",
        json!({"group_id":group_id,"by":"user","to":["worker"],"paths":[outside]}),
    );
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_path")
    );
    assert_eq!(
        ledger::read_all(&store.ledger_path(group_id).expect("ledger"))
            .expect("events")
            .len(),
        before
    );
}

#[test]
fn slash_skill_dispatch_persists_hidden_control_contract_and_replays_by_client_id() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    home.initialize().expect("initialize");
    let created = call(
        &home,
        "group_create",
        json!({"title":"slash-skill-test","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"architect","by":"user"}),
    );
    call(
        &home,
        "capability_import",
        json!({
            "group_id":group_id,
            "by":"user",
            "record":{
            "capability_id":"skill:test:using-superpowers",
            "kind":"skill",
            "name":"using-superpowers",
            "capsule_text":"Use the superpowers workflow for this task."
        }}),
    );

    let args = json!({
        "group_id":group_id,
        "by":"user",
        "to":["architect"],
        "task_text":"开始执行",
        "command":"/using-superpowers",
        "capability_id":"skill:test:using-superpowers",
        "priority":"attention",
        "reply_required":true,
        "client_id":"slash-client-1",
        "reply_to":"event-original",
        "quote_text":"原始请求"
    });
    let first = call(&home, "slash_skill_dispatch", args.clone());
    assert_eq!(first.result["hidden"], true);
    assert_eq!(first.result["delivered"], true);
    assert_eq!(first.result["command"], "/using-superpowers");
    assert_eq!(
        first.result["capability_id"],
        "skill:test:using-superpowers"
    );
    assert_eq!(first.result["to"], json!(["architect"]));
    assert!(
        first.result["event_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(first.result.get("replayed").is_none());

    let ledger_path = GroupStore::new(home.clone())
        .expect("group store")
        .ledger_path(group_id)
        .expect("ledger path");
    let events = ledger::read_all(&ledger_path).expect("ledger");
    let chat_events = events
        .iter()
        .filter(|event| event.kind == "chat.message")
        .collect::<Vec<_>>();
    assert_eq!(chat_events.len(), 1);
    let event = chat_events[0];
    let text = event.data["text"].as_str().expect("control text");
    assert!(text.starts_with("[CCCC] INTERNAL CONTROL"));
    assert!(text.contains("skill_command: /using-superpowers"));
    assert!(text.contains("capability_id: skill:test:using-superpowers"));
    assert!(text.contains("run `cccc_help` first"));
    assert!(text.ends_with("User task:\n开始执行"));
    assert_eq!(event.data["attachments"], json!([]));
    assert!(event.data.get("task_text").is_none());
    assert!(event.data.get("command").is_none());
    assert!(event.data.get("capability_id").is_none());
    assert_eq!(
        event.data["refs"],
        json!([{
            "kind":"text",
            "title":"slash_skill_dispatch",
            "hidden":true,
            "control_kind":"slash_skill_dispatch",
            "command":"/using-superpowers",
            "capability_id":"skill:test:using-superpowers",
            "task_text":"开始执行"
        }])
    );

    let replay = call(&home, "slash_skill_dispatch", args);
    assert_eq!(replay.result["replayed"], true);
    assert_eq!(replay.result["event_id"], first.result["event_id"]);
    assert_eq!(
        ledger::read_all(&ledger_path)
            .expect("ledger after replay")
            .iter()
            .filter(|event| event.kind == "chat.message")
            .count(),
        1
    );
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

    let accepted = call(
        &home,
        "send_cross_group_remote_record",
        json!({
            "group_id":source_id,"dst_group_id":"remote-group","by":"user",
            "to":["reviewer"],"text":"review this","require_peer_insight":true,
            "insight":"The remote reviewer owns the requested decision."
        }),
    );
    assert_eq!(
        accepted.result["source_event"]["data"]["to"],
        json!(["user"])
    );
    assert_eq!(
        accepted.result["source_event"]["data"]["dst_to"],
        json!(["reviewer"])
    );
}

#[test]
fn tracked_send_creates_links_and_recovers_idempotently() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"tracked-send","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"worker","by":"user"}),
    );
    let rejected = call_raw(
        &home,
        "tracked_send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"title":"Rejected",
            "text":"missing insight","require_peer_insight":true
        }),
    );
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("peer_insight_required")
    );
    assert_eq!(
        call(
            &home,
            "context_get",
            json!({"group_id":group_id,"by":"user"})
        )
        .result["coordination"]["tasks"]
            .as_array()
            .expect("tasks")
            .len(),
        0
    );
    let invalid_priority = call_raw(
        &home,
        "tracked_send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"title":"Rejected",
            "text":"invalid priority","message_priority":"urgent"
        }),
    );
    assert_eq!(
        invalid_priority
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("invalid_priority")
    );
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer","by":"user"}),
    );
    let forbidden = call_raw(
        &home,
        "tracked_send",
        json!({
            "group_id":group_id,"by":"peer","to":["worker"],"title":"Rejected",
            "text":"cross-assigned by peer"
        }),
    );
    assert_eq!(
        forbidden.error.as_ref().map(|error| error.code.as_str()),
        Some("context_sync_error")
    );
    assert_eq!(
        call(
            &home,
            "context_get",
            json!({"group_id":group_id,"by":"user"})
        )
        .result["coordination"]["tasks"]
            .as_array()
            .expect("tasks")
            .len(),
        0
    );
    let args = json!({
        "group_id":format!(" {group_id} "),
        "by":" user ",
        "to":["worker"],
        "title":" Fix delivery ",
        "text":" Implement and verify ",
        "outcome":"Delivery is reliable",
        "message_priority":" normal ",
        "checklist":[{"text":"add tests"}],
        "idempotency_key":" tracked-1 "
    });
    let first = call(&home, "tracked_send", args.clone());
    assert_eq!(first.result["task_created"], true);
    assert_eq!(first.result["message_sent"], true);
    assert_eq!(first.result["partial_failure"], false);
    assert_eq!(first.result["task_ref"]["kind"], "task_ref");
    assert_eq!(first.result["event"]["data"]["refs"][0]["kind"], "task_ref");
    assert_eq!(
        first.result["event"]["data"]["text"],
        "Implement and verify"
    );
    assert_eq!(first.result["event"]["data"]["priority"], "normal");
    let task_id = first.result["task_id"].as_str().expect("task id");

    let context = call(
        &home,
        "context_get",
        json!({"group_id":group_id,"by":"user"}),
    );
    let task = context.result["coordination"]["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["id"] == task_id)
        .expect("tracked task");
    assert_eq!(task["title"], "Fix delivery");
    assert_eq!(task["outcome"], "Delivery is reliable");
    assert_eq!(task["assignee"], "worker");

    let replay = call(&home, "tracked_send", args);
    assert_eq!(replay.result["replayed"], true);
    assert_eq!(replay.result["task_id"], task_id);
    assert_eq!(replay.result["event_id"], first.result["event_id"]);
    assert_eq!(
        call(
            &home,
            "context_get",
            json!({"group_id":group_id,"by":"user"})
        )
        .result["coordination"]["tasks"]
            .as_array()
            .expect("tasks")
            .len(),
        1
    );

    let without_key = call(
        &home,
        "tracked_send",
        json!({
            "group_id":group_id,"by":"user","to":"worker","title":"No key",
            "text":"send once","checklist":"first\nsecond",
            "refs":[null,"invalid",{"kind":"text","title":"source"}]
        }),
    );
    assert_eq!(without_key.result["event"]["data"].get("client_id"), None);
    let context = call(
        &home,
        "context_get",
        json!({"group_id":group_id,"by":"user"}),
    );
    let no_key_task = context.result["coordination"]["tasks"]
        .as_array()
        .expect("tasks")
        .iter()
        .find(|task| task["id"] == without_key.result["task_id"])
        .expect("task");
    assert_eq!(
        no_key_task["checklist"],
        json!([{"text":"first"},{"text":"second"}])
    );
    assert!(no_key_task.get("client_request_id").is_none());
    assert_eq!(
        without_key.result["event"]["data"]["refs"],
        json!([
            {"kind":"text","title":"source"},
            {
                "kind":"task_ref",
                "task_id":without_key.result["task_id"],
                "title":"No key",
                "status":"planned",
                "waiting_on":"actor",
                "handoff_to":""
            }
        ])
    );
}

#[test]
fn message_domain_contracts_cover_install_ack_idle_stream_and_validation() {
    use cccc_contracts::GroupState;

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"message-contracts","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,"actor_id":"worker","title":"Stream Worker","by":"user"
        }),
    );
    let store = GroupStore::new(home.clone()).expect("store");
    store
        .mutate(group_id, |group| {
            group.state = GroupState::Idle;
            Ok(())
        })
        .expect("idle");

    let install = call(
        &home,
        "send",
        json!({"group_id":group_id,"by":"user","to":["worker"],"text":"/install owner/repo"}),
    );
    assert_eq!(
        install.result["event"]["data"]["refs"][0]["capability_id"],
        "skill:cccc:install"
    );
    assert_eq!(
        store.load(group_id).expect("group").state,
        GroupState::Active
    );

    let attention = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"text":"respond",
            "priority":"attention","reply_required":true
        }),
    );
    let reply = call(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"worker","reply_to":attention.result["event"]["id"],
            "text":"done"
        }),
    );
    assert_eq!(reply.result["ack_event"]["kind"], "chat.ack");
    assert_eq!(
        reply.result["ack_event"]["data"]["event_id"],
        attention.result["event"]["id"]
    );
    let normal = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"text":"normal reply",
            "priority":"normal","reply_required":true
        }),
    );
    let normal_reply = call(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"worker","reply_to":normal.result["event"]["id"],
            "text":"done"
        }),
    );
    assert_eq!(normal_reply.result["ack_event"], Value::Null);

    let start = call(
        &home,
        "stream_emit",
        json!({"group_id":group_id,"by":"worker","op":"start","text":"a"}),
    );
    let stream_id = start.result["stream_id"].as_str().expect("stream id");
    assert!(!stream_id.is_empty());
    assert_eq!(
        start.result["event"]["data"]["sender_title"],
        "Stream Worker"
    );
    let update = call(
        &home,
        "stream_emit",
        json!({"group_id":group_id,"by":"worker","op":"update","stream_id":stream_id,"text":"b"}),
    );
    assert_eq!(update.result["stream_id"], stream_id);
    let missing_stream = call_raw(
        &home,
        "stream_emit",
        json!({"group_id":group_id,"by":"worker","op":"end"}),
    );
    assert_eq!(
        missing_stream
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("missing_stream_id")
    );

    let missing_attachment = call_raw(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"text":"",
            "attachments":[{"path":"state/blobs/missing"}]
        }),
    );
    assert_eq!(
        missing_attachment
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("invalid_attachments")
    );
    let outside_scope = call_raw(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"text":"path",
            "path":temp.path().join("outside").to_string_lossy()
        }),
    );
    assert_eq!(
        outside_scope
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("scope_not_attached")
    );
}

#[test]
fn delegation_relays_to_target_foreman_with_legacy_cross_group_schema() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let source = call(&home, "group_create", json!({"title":"source","by":"user"}));
    let destination = call(
        &home,
        "group_create",
        json!({"title":"destination","by":"user"}),
    );
    let source_id = source.result["group"]["group_id"].as_str().expect("source");
    let destination_id = destination.result["group"]["group_id"]
        .as_str()
        .expect("destination");
    call(
        &home,
        "actor_add",
        json!({"group_id":source_id,"actor_id":"source-lead","by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({"group_id":destination_id,"actor_id":"target-lead","by":"user"}),
    );

    let direct = call(
        &home,
        "send_cross_group",
        json!({
            "group_id":source_id,"dst_group_id":destination_id,"by":"source-lead",
            "to":["target-lead"],"text":"schema"
        }),
    );
    assert_eq!(direct.result["src_event"], direct.result["source_event"]);
    assert_eq!(direct.result["dst_event"], direct.result["event"]);

    let delegated = call(
        &home,
        "relay_user_delegation",
        json!({
            "group_id":source_id,"dst_group_id":destination_id,"by":"user",
            "text":"#destination 请处理","source_event_id":"source-message"
        }),
    );
    assert_eq!(delegated.result["relay"]["sender"], "source-lead");
    assert_eq!(delegated.result["relay"]["target_actor_id"], "target-lead");
    assert!(
        delegated.result["relay"]["delegation_id"]
            .as_str()
            .is_some_and(|value| value.starts_with("dlg_"))
    );
    assert!(
        delegated.result["relay"]["src_event_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    assert!(
        delegated.result["relay"]["dst_event_id"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
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
