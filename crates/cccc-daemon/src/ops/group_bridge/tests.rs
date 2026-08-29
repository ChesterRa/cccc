use cccc_contracts::{DaemonRequest, Event, GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION};
use cccc_core::{GroupStore, HomeLayout, group_bridge_legacy, ledger};
use serde_json::json;
use std::thread;
use tempfile::tempdir;

use super::{
    delivery_status, normalize_outbound_payload, receipt_retry_due, schedule_due_retries,
    session_runtime, validate_remote_payload,
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
                {"registration_id":"greg_1","idempotency_key":"once","status":"sent"}
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
    assert_eq!(result["receipt"]["status"], "sent");
}

#[test]
fn session_delivery_requires_a_supported_operation() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path()).expect("home path");
    home.initialize().expect("home");
    let route = json!({
        "group_id":"g_local","remote_group_id":"g_remote",
        "remote_peer_id":"peer_remote","idempotency_key":"once",
        "payload":{"text":"hello"}
    });

    let missing = session_runtime::deliver(&home, &request("deliver", route.clone()))
        .expect_err("operation is required");
    assert_eq!(missing.code, "invalid_args");

    let mut unsupported = route;
    unsupported["operation"] = json!("legacy_send");
    let error = session_runtime::deliver(&home, &request("deliver", unsupported))
        .expect_err("unsupported operation");
    assert_eq!(error.code, "unsupported_op");
}

#[test]
fn outbound_peer_message_requires_insight_before_side_effects() {
    let request = DaemonRequest {
        v: 1,
        op: "remote_send".into(),
        args: json!({
            "by":"peer-a","require_peer_insight":true,
            "payload":{"message_mode":"send","text":"review this","to":["@foreman"]}
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
        "message_mode":"send","text":"hello","to":[" @foreman ",7],"refs":[{"event_id":"e1"}]
    })
    .as_object()
    .cloned()
    .expect("payload");
    let error = validate_remote_payload(&mut payload).expect_err("unsupported refs");
    assert_eq!(error.code, "unsupported_refs");
}

#[test]
fn remote_payload_enforces_one_audience_domain_and_agent_only_mail() {
    for (recipients, mode, expected_code) in [
        (json!(["user", "peer1"]), "send", "mixed_recipient_kinds"),
        (json!(["user"]), "mail", "mail_requires_actor_recipient"),
    ] {
        let mut payload = json!({
            "message_mode":mode,"text":"hello","to":recipients
        })
        .as_object()
        .cloned()
        .expect("payload");
        let error = validate_remote_payload(&mut payload).expect_err("invalid audience");
        assert_eq!(error.code, expected_code);
    }
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
            "status":"sent","event_id":"remote-answer",
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
    assert_eq!(response.result["event"]["data"]["message_mode"], "send");
    assert_eq!(response.result["event"]["data"]["dst_message_mode"], "send");
    assert_eq!(
        response.result["group_bridge_reply"]["receipt"]["remote_event_id"],
        "remote-answer"
    );
    assert_eq!(
        response.result["group_bridge_reply"]["receipt"]["status"], "sent",
        "new receipts must use the canonical success status"
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
        "message_mode":"send",
        "dst_group_id":"g_remote","dst_to":["@foreman"],
        "dst_message_mode":"mail",
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
                "operation":"remote_send","ok":false,"status":"retrying",
                "registration_id":"registration_retry",
                "idempotency_key":"python-retry","src_group_id":group.group_id,
                "dst_group_id":"g_remote","source_event_id":source.id,
                "attempt":1,"max_attempts":5,
                "payload":{
                    "text":"created by Python","to":["@foreman"],
                    "message_mode":"mail",
                    "refs":[],"attachments":[],"source_by":"python-agent"
                },
                "source_record_payload":{
                    "text":"created by Python","to":["@foreman"],
                    "message_mode":"mail",
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
                "payload":{"message_mode":"mail","text":"changed retry body","to":["@foreman"]}
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
fn periodic_retry_respects_due_and_stale_boundaries() {
    let now = chrono::Utc::now();
    let future = (now + chrono::Duration::seconds(30)).to_rfc3339();
    let past = (now - chrono::Duration::seconds(30)).to_rfc3339();
    assert!(!receipt_retry_due(
        &json!({"status":"retrying","attempt":1,"max_attempts":5,"next_attempt_at":future}),
        now,
        true,
    ));
    assert!(receipt_retry_due(
        &json!({"status":"retrying","attempt":1,"max_attempts":5,"next_attempt_at":past}),
        now,
        true,
    ));
    assert!(!receipt_retry_due(
        &json!({"status":"sending","attempt":1,"max_attempts":5,"last_attempt_at":now.to_rfc3339()}),
        now,
        true,
    ));
    assert!(receipt_retry_due(
        &json!({"status":"sending","attempt":1,"max_attempts":5,"last_attempt_at":(
            now - chrono::Duration::seconds(121)
        ).to_rfc3339()}),
        now,
        true,
    ));
    assert!(!receipt_retry_due(
        &json!({"status":"retrying","attempt":5,"max_attempts":5}),
        now,
        false,
    ));
}

#[test]
fn due_receipt_is_retried_without_web_or_caller_activity() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("sender", "").expect("group");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    let mut source = Event::new("chat.message", &group.group_id);
    source.by = "user".into();
    source.data = json!({
        "text":"retry in daemon","to":["user"],"message_mode":"send",
        "dst_group_id":"g_remote","dst_to":["@foreman"],
        "dst_message_mode":"mail","client_id":"daemon-outbox-source"
    })
    .as_object()
    .cloned()
    .expect("source data");
    ledger::append(&ledger_path, &source).expect("append source");
    group_bridge_legacy::update(&home, |state| {
        state.clear();
        state.insert(
            "trusts".into(),
            json!([{
                "trust_id":"trust_due","registration_id":"registration_due",
                "group_id":group.group_id,"remote_group_id":"g_remote",
                "remote_peer_id":"peer_remote","transport":"group_bridge_session",
                "status":"active","remote_access_level":"messages"
            }]),
        );
        state.insert(
            "deliveries".into(),
            json!([{
                "operation":"remote_send","ok":false,"status":"retrying",
                "registration_id":"registration_due","idempotency_key":"daemon-retry",
                "src_group_id":group.group_id,"dst_group_id":"g_remote",
                "source_event_id":source.id,"attempt":1,"max_attempts":5,
                "next_attempt_at":"2000-01-01T00:00:00Z",
                "payload":{
                    "text":"retry in daemon","to":["@foreman"],"source_by":"user",
                    "message_mode":"mail","refs":[],"attachments":[]
                },
                "source_record_payload":{
                    "text":"retry in daemon","to":["@foreman"],"source_by":"user",
                    "message_mode":"mail","refs":[],"attachments":[]
                }
            }]),
        );
        Ok(())
    })
    .expect("bridge state");

    schedule_due_retries(home.clone());
    let mut retried = None;
    for _ in 0..100 {
        let state = group_bridge_legacy::load(&home).expect("receipt state");
        retried = state["deliveries"]
            .as_array()
            .and_then(|receipts| receipts.first())
            .cloned();
        if retried
            .as_ref()
            .is_some_and(|receipt| receipt["attempt"] == 2 && receipt["status"] == "retrying")
        {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    let retried = retried.expect("receipt");
    assert_eq!(retried["attempt"], 2, "{retried}");
    assert_eq!(retried["status"], "retrying");
    assert!(
        retried["next_attempt_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "{retried}"
    );
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
                "payload":{"message_mode":"mail","text":"first while offline","to":["@foreman"]}
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
        "ok":true,"receipt":{"status":"sent","event_id":"remote-a"}
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
                    "payload":{"message_mode":"mail","text":"second after recovery","to":["@foreman"]}
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
        "ok":true,"receipt":{"status":"sent","event_id":"remote-b"}
    });
    session_runtime::complete(&home, &request("complete", complete_args)).expect("complete B");
    let second = second.join().expect("join B").expect("send B");
    assert_eq!(second["receipt"]["status"], "sent");

    let mut close_args = route;
    close_args["generation"] = generation;
    session_runtime::close(&home, &request("close", close_args)).expect("close");
}

#[test]
fn remote_reply_request_cancellation_reuses_the_sent_remote_source() {
    let temp = tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home path");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("sender", "").expect("group");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger path");
    let mut source = Event::new("chat.message", &group.group_id);
    source.by = "user".into();
    source.data = json!({
        "text":"answer this","to":["user"],"message_mode":"send",
        "dst_group_id":"g_remote","dst_to":["@foreman"],
        "dst_message_mode":"request_reply"
    })
    .as_object()
    .cloned()
    .expect("source data");
    ledger::append(&ledger_path, &source).expect("append source");
    group_bridge_legacy::update(&home, |state| {
        state.clear();
        state.insert(
            "trusts".into(),
            json!([{
                "trust_id":"trust_cancel","registration_id":"registration_cancel",
                "group_id":group.group_id,"remote_group_id":"g_remote",
                "remote_peer_id":"peer_remote","transport":"group_bridge_session",
                "status":"active","remote_access_level":"messages"
            }]),
        );
        state.insert(
            "deliveries".into(),
            json!([{
                "operation":"remote_send","ok":true,"status":"sent",
                "registration_id":"registration_cancel","idempotency_key":"message-key",
                "src_group_id":group.group_id,"dst_group_id":"g_remote",
                "source_event_id":source.id,"remote_event_id":"remote-message-1",
                "transport":"group_bridge_session","attempt":1,"max_attempts":5
            }]),
        );
        Ok(())
    })
    .expect("bridge state");

    let route = json!({
        "group_id":group.group_id,"remote_group_id":"g_remote",
        "remote_peer_id":"peer_remote"
    });
    let opened = session_runtime::open(&home, &request("open", route.clone())).expect("open");
    let generation = opened["generation"].clone();
    let cancel_home = home.clone();
    let group_id = group.group_id.clone();
    let source_event_id = source.id.clone();
    let cancel_task = thread::spawn(move || {
        crate::dispatch::dispatch(
            &cancel_home,
            &request(
                "reply_request_cancel",
                json!({
                    "group_id":group_id,"source_event_id":source_event_id,"by":"user"
                }),
            ),
        )
    });

    let mut poll_args = route.clone();
    poll_args["generation"] = generation.clone();
    poll_args["timeout_ms"] = json!(1_000);
    let pending = session_runtime::poll(&home, &request("poll", poll_args)).expect("poll");
    let frame = &pending["request"];
    assert_eq!(frame["op"], "reply_request_cancel");
    assert_eq!(
        frame["message_contract_version"],
        GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
    );
    assert_eq!(
        frame["payload"]["remote_source_event_id"],
        "remote-message-1"
    );
    assert_eq!(frame["payload"]["source_message_event_id"], source.id);
    let mut complete_args = route.clone();
    complete_args["generation"] = generation.clone();
    complete_args["response_to"] = frame["request_id"].clone();
    complete_args["result"] = json!({"ok":true,"event_id":"remote-cancel-1"});
    session_runtime::complete(&home, &request("complete", complete_args)).expect("complete");

    let response = cancel_task.join().expect("cancel join");
    assert!(response.ok, "cancel failed: {:?}", response.error);
    assert_eq!(response.result["propagation"]["state"], "sent");
    let events = ledger::read_all(&ledger_path).expect("ledger");
    let cancellation = events
        .iter()
        .find(|event| event.kind == "chat.reply_request.cancelled")
        .expect("local cancellation");
    let projected = events
        .iter()
        .find(|event| {
            event.kind == "chat.cross_group_receipt"
                && event
                    .data
                    .get("operation")
                    .and_then(serde_json::Value::as_str)
                    == Some("reply_request_cancel")
        })
        .expect("cancellation receipt projection");
    assert_eq!(projected.data["source_event_id"], cancellation.id);
    assert_eq!(projected.data["remote_event_id"], "remote-cancel-1");

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
    assert_eq!(response.result["event"]["data"]["to"], json!(["user"]));
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
