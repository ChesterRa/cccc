use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupStore, HomeLayout, integration_state, ledger};
use serde_json::json;
use std::thread;
use tempfile::tempdir;

use super::{
    STORE_KEY, delivery_status, normalize_outbound_payload, session_runtime,
    validate_remote_payload,
};

fn request(op: &str, value: serde_json::Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: value
            .as_object()
            .cloned()
            .expect("request args must be an object"),
    }
}

#[test]
fn delivery_status_reads_python_compatible_receipt() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path()).expect("home path");
    home.initialize().expect("home");
    integration_state::global_update(&home, STORE_KEY, |state| {
        *state = json!({
            "registrations":[{"registration_id":"greg_1","group_id":"g_local","status":"active"}],
            "deliveries":[{"registration_id":"greg_1","idempotency_key":"once","status":"delivered"}]
        });
        Ok(())
    })
    .expect("state");
    let result = delivery_status(
        &home,
        &DaemonRequest {
            v: 1,
            op: "remote_delivery_status".into(),
            args: json!({
                "group_id":"g_local","registration_id":"greg_1","idempotency_key":"once"
            })
            .as_object()
            .cloned()
            .expect("args"),
        },
    )
    .expect("status");
    assert_eq!(result["receipt"]["status"], "delivered");
}

#[test]
fn outbound_peer_message_requires_insight_before_side_effects() {
    let request = DaemonRequest {
        v: 1,
        op: "remote_send".into(),
        args: json!({
            "by":"peer-a","require_peer_insight":true,
            "payload":{"text":"review this","to":["@foreman"]}
        })
        .as_object()
        .cloned()
        .expect("args"),
    };
    let mut payload = request.args["payload"]
        .as_object()
        .cloned()
        .expect("payload");
    let error = normalize_outbound_payload(&request, &mut payload).expect_err("missing insight");
    assert_eq!(error.code, "peer_insight_required");
    assert_eq!(error.details["new_side_effects"], false);
}

#[test]
fn remote_payload_rejects_refs_and_normalizes_recipients() {
    let mut payload = json!({
        "text":"hello","to":[" @foreman ",7],"refs":[{"event_id":"e1"}]
    })
    .as_object()
    .cloned()
    .expect("payload");
    let error = validate_remote_payload(&mut payload).expect_err("unsupported refs");
    assert_eq!(error.code, "unsupported_refs");
}

#[test]
fn outbound_attachments_are_encoded_without_exposing_local_paths() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("sender", "").expect("group");
    let blob =
        cccc_core::blobs::store(&home, &group.group_id, b"binary-reply").expect("store attachment");
    let mut payload = json!({
        "text":"see attachment",
        "to":["remote-agent"],
        "attachments":[{
            "kind":"file","title":"reply.bin","path":blob.path,
            "bytes":blob.bytes,"sha256":blob.sha256
        }]
    })
    .as_object()
    .cloned()
    .expect("payload");

    super::payload::encode_outbound_attachments(&home, &group.group_id, &mut payload)
        .expect("encode attachments");

    let attachment = &payload["attachments"][0];
    assert_eq!(attachment["content_base64"], "YmluYXJ5LXJlcGx5");
    assert_eq!(attachment["bytes"], 12);
    assert!(attachment.get("path").is_none());
}

#[test]
fn remote_reply_uses_reverse_session_and_keeps_one_local_record() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("receiver", "").expect("group");
    integration_state::global_update(&home, STORE_KEY, |state| {
        *state = json!({"trusts":[{
            "trust_id":"trust_reply","registration_id":"registration_reply",
            "group_id":group.group_id,"remote_group_id":"g_remote",
            "remote_peer_id":"peer_remote","transport":"group_bridge_session",
            "status":"active","remote_access_level":"messages"
        }]});
        Ok(())
    })
    .expect("bridge state");

    let mut inbound = Event::new("chat.message", &group.group_id);
    inbound.by = "group_bridge:peer_remote".into();
    inbound.data = json!({
        "text":"question from remote","to":["@foreman"],
        "source_platform":"group_bridge_session",
        "source_user_name":"Remote group","source_user_id":"peer_remote",
        "src_group_id":"g_remote","src_event_id":"remote-question",
        "src_by":"remote-agent","remote_reply_to":["remote-agent"]
    })
    .as_object()
    .cloned()
    .expect("inbound data");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    ledger::append(&ledger_path, &inbound).expect("append inbound");

    let route = json!({
        "group_id":group.group_id,"remote_group_id":"g_remote",
        "remote_peer_id":"peer_remote"
    });
    let opened = session_runtime::open(&home, &request("open", route.clone())).expect("open");
    let generation = opened["generation"]
        .as_str()
        .expect("generation")
        .to_owned();
    let home_for_reply = home.clone();
    let group_id = group.group_id.clone();
    let inbound_id = inbound.id.clone();
    let reply_task = thread::spawn(move || {
        crate::dispatch::dispatch(
            &home_for_reply,
            &request(
                "reply",
                json!({
                    "group_id":group_id,"by":"user","reply_to":inbound_id,
                    "text":"answer to remote","to":[],"client_id":"reply-once"
                }),
            ),
        )
    });

    let mut poll_args = route.clone();
    poll_args["generation"] = json!(generation);
    poll_args["timeout_ms"] = json!(1_000);
    let pending = session_runtime::poll(&home, &request("poll", poll_args)).expect("poll");
    let frame = &pending["request"];
    assert_eq!(frame["op"], "remote_send");
    assert_eq!(frame["payload"]["to"], json!(["remote-agent"]));
    assert_eq!(frame["payload"]["reply_to"], "remote-question");
    assert_eq!(frame["payload"]["source_by"], "user");
    assert!(
        frame["payload"]["src_event_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );

    let mut complete_args = route.clone();
    complete_args["generation"] = json!(generation);
    complete_args["response_to"] = frame["request_id"].clone();
    complete_args["result"] = json!({"ok":true,"event_id":"remote-answer"});
    session_runtime::complete(&home, &request("complete", complete_args)).expect("complete");

    let response = reply_task.join().expect("reply thread");
    assert!(response.ok, "reply failed: {:?}", response.error);
    assert_eq!(response.result["event"]["data"]["to"], json!(["user"]));
    assert_eq!(
        response.result["event"]["data"]["dst_to"],
        json!(["remote-agent"])
    );
    assert_eq!(response.result["event"]["data"]["dst_group_id"], "g_remote");
    assert_eq!(
        response.result["group_bridge_reply"]["receipt"]["remote_event_id"],
        "remote-answer"
    );

    let messages = ledger::read_all(&ledger_path)
        .expect("read ledger")
        .into_iter()
        .filter(|event| event.kind == "chat.message")
        .collect::<Vec<_>>();
    assert_eq!(
        messages.len(),
        2,
        "remote reply must not append a duplicate source record"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|event| event
                .data
                .get("reply_to")
                .and_then(serde_json::Value::as_str)
                == Some(inbound.id.as_str()))
            .count(),
        1
    );

    let mut close_args = route;
    close_args["generation"] = json!(generation);
    session_runtime::close(&home, &request("close", close_args)).expect("close");
}

#[test]
fn remote_reply_without_return_recipient_fails_before_local_append() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("receiver", "").expect("group");
    integration_state::global_update(&home, STORE_KEY, |state| {
        *state = json!({"trusts":[{
            "trust_id":"trust_reply","registration_id":"registration_reply",
            "group_id":group.group_id,"remote_group_id":"g_remote",
            "remote_peer_id":"peer_remote","transport":"group_bridge_session",
            "status":"active","remote_access_level":"messages"
        }]});
        Ok(())
    })
    .expect("bridge state");
    let mut inbound = Event::new("chat.message", &group.group_id);
    inbound.by = "group_bridge:peer_remote".into();
    inbound.data = json!({
        "text":"question from remote","to":["@foreman"],
        "source_platform":"group_bridge_session","source_user_id":"peer_remote",
        "src_group_id":"g_remote","src_event_id":"remote-question"
    })
    .as_object()
    .cloned()
    .expect("inbound data");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    ledger::append(&ledger_path, &inbound).expect("append inbound");

    let response = crate::dispatch::dispatch(
        &home,
        &request(
            "reply",
            json!({
                "group_id":group.group_id,"by":"user","reply_to":inbound.id,
                "text":"answer to remote","to":[]
            }),
        ),
    );
    assert!(!response.ok);
    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("missing_remote_recipient")
    );
    assert_eq!(ledger::read_all(&ledger_path).expect("ledger").len(), 1);
}

#[test]
fn remote_reply_allows_an_explicit_remote_audience_without_local_actors() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("receiver", "").expect("group");
    integration_state::global_update(&home, STORE_KEY, |state| {
        *state = json!({"trusts":[{
            "trust_id":"trust_reply","registration_id":"registration_reply",
            "group_id":group.group_id,"remote_group_id":"g_remote",
            "remote_peer_id":"peer_remote","transport":"group_bridge_session",
            "status":"active","remote_access_level":"messages"
        }]});
        Ok(())
    })
    .expect("bridge state");
    let mut inbound = Event::new("chat.message", &group.group_id);
    inbound.by = "group_bridge:peer_remote".into();
    inbound.data = json!({
        "text":"question","to":["user"],
        "source_platform":"group_bridge_session","source_user_id":"peer_remote",
        "src_group_id":"g_remote","src_event_id":"remote-question"
    })
    .as_object()
    .cloned()
    .expect("inbound data");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    ledger::append(&ledger_path, &inbound).expect("append inbound");

    let response = crate::dispatch::dispatch(
        &home,
        &request(
            "reply",
            json!({
                "group_id":group.group_id,"by":"user","reply_to":inbound.id,
                "text":"answer to foreman","to":["@foreman"]
            }),
        ),
    );

    assert!(response.ok, "remote reply failed: {:?}", response.error);
    assert_eq!(response.result["event"]["data"]["to"], json!(["@foreman"]));
    assert_eq!(
        response.result["event"]["data"]["dst_to"],
        json!(["@foreman"])
    );
}

#[test]
fn reply_to_local_event_with_inherited_bridge_metadata_stays_local() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("receiver", "").expect("group");
    let mut local = Event::new("chat.message", &group.group_id);
    local.by = "user".into();
    local.data = json!({
        "text":"local reply","to":["user"],
        "source_platform":"group_bridge_session",
        "source_user_id":"peer_remote","dst_group_id":"g_remote"
    })
    .as_object()
    .cloned()
    .expect("local data");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    ledger::append(&ledger_path, &local).expect("append local");

    let response = crate::dispatch::dispatch(
        &home,
        &request(
            "reply",
            json!({
                "group_id":group.group_id,"by":"system","reply_to":local.id,
                "text":"local follow-up","to":[]
            }),
        ),
    );

    assert!(response.ok, "local reply failed: {:?}", response.error);
    assert!(response.result.get("group_bridge_reply").is_none());
    assert_eq!(response.result["event"]["data"]["to"], json!(["user"]));
}
