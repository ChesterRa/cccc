use cccc_contracts::{DaemonRequest, Event};
use cccc_core::{GroupStore, HomeLayout, group_bridge_legacy, ledger};
use serde_json::json;
use std::thread;
use tempfile::tempdir;

use super::{
    delivery_status, normalize_outbound_payload, session_runtime, validate_remote_payload,
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
    group_bridge_legacy::update(&home, |state| {
        state.insert(
            "registrations".into(),
            json!([
                {"registration_id":"greg_1","group_id":"g_local","status":"active"}
            ]),
        );
        state.insert(
            "deliveries".into(),
            json!([
                {"registration_id":"greg_1","idempotency_key":"once","status":"delivered"}
            ]),
        );
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
    group_bridge_legacy::update(&home, |state| {
        state.clear();
        state.insert(
            "trusts".into(),
            json!([{
                "trust_id":"trust_reply","registration_id":"registration_reply",
                "group_id":group.group_id,"remote_group_id":"g_remote",
                "remote_peer_id":"peer_remote","transport":"group_bridge_session",
                "status":"active","remote_access_level":"messages"
            }]),
        );
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
    complete_args["result"] = json!({
        "ok":true,
        "receipt":{
            "status":"delivered","event_id":"remote-answer",
            "projected":true,"registration_id":"peer-controlled"
        }
    });
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
    assert_eq!(
        response.result["group_bridge_reply"]["receipt"]["status"], "sent",
        "new Rust receipts must use the shared Python/Rust success status"
    );

    let events = ledger::read_all(&ledger_path).expect("read ledger");
    let messages = events
        .iter()
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
    let projected = events
        .iter()
        .filter(|event| event.kind == "chat.cross_group_receipt")
        .collect::<Vec<_>>();
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].data["source_event_id"],
        response.result["event"]["id"]
    );
    assert_eq!(projected[0].data["remote_event_id"], "remote-answer");
    assert_eq!(
        response.result["group_bridge_reply"]["receipt"]["registration_id"], "registration_reply",
        "peer receipt metadata must not replace the local receipt identity"
    );

    let mut close_args = route;
    close_args["generation"] = json!(generation);
    session_runtime::close(&home, &request("close", close_args)).expect("close");
}

#[test]
fn rust_retry_reuses_a_python_source_event_without_appending_a_duplicate() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("sender", "").expect("group");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    let mut source = Event::new("chat.message", &group.group_id);
    source.by = "python-agent".into();
    source.data = json!({
        "text":"created by Python","to":["user"],
        "dst_group_id":"g_remote","dst_to":["@foreman"],
        "client_id":"python-source-client-id"
    })
    .as_object()
    .cloned()
    .expect("source data");
    ledger::append(&ledger_path, &source).expect("append Python source");
    group_bridge_legacy::update(&home, |state| {
        state.clear();
        state.insert(
            "trusts".into(),
            json!([{
                "trust_id":"trust_retry","registration_id":"registration_retry",
                "group_id":group.group_id,"remote_group_id":"g_remote",
                "remote_peer_id":"peer_remote","transport":"group_bridge_session",
                "status":"active","remote_access_level":"messages"
            }]),
        );
        state.insert(
            "deliveries".into(),
            json!([{
                "ok":false,"status":"retrying",
                "registration_id":"registration_retry",
                "idempotency_key":"python-retry","src_group_id":group.group_id,
                "dst_group_id":"g_remote","source_event_id":source.id,
                "attempt":1,"max_attempts":5,
                "payload":{
                    "text":"created by Python","to":["@foreman"],
                    "priority":"normal","reply_required":false,
                    "refs":[],"attachments":[],"source_by":"python-agent"
                },
                "source_record_payload":{
                    "text":"created by Python","to":["@foreman"],
                    "priority":"normal","reply_required":false,
                    "refs":[],"attachments":[],"source_by":"python-agent"
                }
            }]),
        );
        Ok(())
    })
    .expect("bridge state");

    let result = super::remote_send(
        &home,
        &request(
            "remote_send",
            json!({
                "group_id":group.group_id,
                "registration_id":"registration_retry",
                "idempotency_key":"python-retry",
                "by":"python-agent",
                "payload":{"text":"changed retry body","to":["@foreman"]}
            }),
        ),
    )
    .expect("retry remains durable");

    assert_eq!(result["receipt"]["status"], "retrying");
    assert_eq!(result["receipt"]["source_event_id"], source.id);
    let messages = ledger::read_all(&ledger_path)
        .expect("ledger")
        .into_iter()
        .filter(|event| event.kind == "chat.message")
        .collect::<Vec<_>>();
    assert_eq!(
        messages.len(),
        1,
        "retry must reuse the Python source event"
    );
    assert_eq!(messages[0].id, source.id);
}

#[test]
fn retrying_delivery_resumes_when_a_reverse_session_opens_and_later_work_continues() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("sender", "").expect("group");
    group_bridge_legacy::update(&home, |state| {
        state.clear();
        state.insert(
            "trusts".into(),
            json!([{
                "trust_id":"trust_resume","registration_id":"registration_resume",
                "group_id":group.group_id,"remote_group_id":"g_remote",
                "remote_peer_id":"peer_remote","transport":"group_bridge_session",
                "status":"active","remote_access_level":"messages"
            }]),
        );
        Ok(())
    })
    .expect("bridge state");

    let first = super::remote_send(
        &home,
        &request(
            "remote_send",
            json!({
                "group_id":group.group_id,
                "registration_id":"registration_resume",
                "idempotency_key":"resume-a",
                "by":"user",
                "payload":{"text":"first while offline","to":["user"]}
            }),
        ),
    )
    .expect("durable first attempt");
    assert_eq!(first["receipt"]["status"], "retrying");

    let route = json!({
        "group_id":group.group_id,"remote_group_id":"g_remote",
        "remote_peer_id":"peer_remote"
    });
    let opened = session_runtime::open(&home, &request("open", route.clone())).expect("open");
    let generation = opened["generation"].clone();
    let mut poll_args = route.clone();
    poll_args["generation"] = generation.clone();
    poll_args["timeout_ms"] = json!(1_000);
    let resumed = session_runtime::poll(&home, &request("poll", poll_args)).expect("poll");
    assert_eq!(resumed["request"]["idempotency_key"], "resume-a");
    let mut complete_args = route.clone();
    complete_args["generation"] = generation.clone();
    complete_args["response_to"] = resumed["request"]["request_id"].clone();
    complete_args["result"] = json!({
        "ok":true,"receipt":{"status":"delivered","event_id":"remote-a"}
    });
    session_runtime::complete(&home, &request("complete", complete_args)).expect("complete A");
    for _ in 0..100 {
        let state = group_bridge_legacy::load(&home).expect("receipt state");
        if super::find_delivery(&state, "registration_resume", "resume-a")
            .is_some_and(|receipt| receipt["status"] == "sent")
        {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    let state = group_bridge_legacy::load(&home).expect("receipt state");
    assert_eq!(
        super::find_delivery(&state, "registration_resume", "resume-a").expect("A receipt")["status"],
        "sent"
    );

    let send_home = home.clone();
    let group_id = group.group_id.clone();
    let second = thread::spawn(move || {
        super::remote_send(
            &send_home,
            &request(
                "remote_send",
                json!({
                    "group_id":group_id,
                    "registration_id":"registration_resume",
                    "idempotency_key":"resume-b",
                    "by":"user",
                    "payload":{"text":"second after recovery","to":["user"]}
                }),
            ),
        )
    });
    let mut poll_args = route.clone();
    poll_args["generation"] = generation.clone();
    poll_args["timeout_ms"] = json!(1_000);
    let continued = session_runtime::poll(&home, &request("poll", poll_args)).expect("poll B");
    assert_eq!(continued["request"]["idempotency_key"], "resume-b");
    let mut complete_args = route.clone();
    complete_args["generation"] = generation.clone();
    complete_args["response_to"] = continued["request"]["request_id"].clone();
    complete_args["result"] = json!({
        "ok":true,"receipt":{"status":"delivered","event_id":"remote-b"}
    });
    session_runtime::complete(&home, &request("complete", complete_args)).expect("complete B");
    let second = second.join().expect("join B").expect("send B");
    assert_eq!(second["receipt"]["status"], "sent");

    let mut close_args = route;
    close_args["generation"] = generation;
    session_runtime::close(&home, &request("close", close_args)).expect("close");
}

#[test]
fn remote_reply_without_return_recipient_fails_before_local_append() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("receiver", "").expect("group");
    group_bridge_legacy::update(&home, |state| {
        state.clear();
        state.insert(
            "trusts".into(),
            json!([{
                "trust_id":"trust_reply","registration_id":"registration_reply",
                "group_id":group.group_id,"remote_group_id":"g_remote",
                "remote_peer_id":"peer_remote","transport":"group_bridge_session",
                "status":"active","remote_access_level":"messages"
            }]),
        );
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
    group_bridge_legacy::update(&home, |state| {
        state.clear();
        state.insert(
            "trusts".into(),
            json!([{
                "trust_id":"trust_reply","registration_id":"registration_reply",
                "group_id":group.group_id,"remote_group_id":"g_remote",
                "remote_peer_id":"peer_remote","transport":"group_bridge_session",
                "status":"active","remote_access_level":"messages"
            }]),
        );
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
