#![cfg(unix)]

use cccc_client::DaemonClient;
use cccc_contracts::{DaemonRequest, DaemonResponse, Event};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Map, Value, json};
use std::time::Duration;

#[tokio::test]
async fn serializes_delivery_and_keeps_read_as_a_separate_fact() {
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
    let project = temp.path().join("project");
    std::fs::create_dir(&project).expect("project scope");
    daemon_call(
        &client,
        "attach",
        json!({"group_id":group_id,"path":project,"by":"user"}),
    )
    .await;
    daemon_call(
        &client,
        "group_preamble_set",
        json!({"group_id":group_id,"content":"Use the CCCC delivery protocol.","by":"user"}),
    )
    .await;
    daemon_call(
        &client,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runner":"pty",
            "runtime":"custom",
            "submit":"newline",
            "command":["sh","-c","stty -echo; IFS= read -r preamble; IFS= read -r first; IFS= read -r second; IFS= read -r third; IFS= read -r fourth; printf 'PREAMBLE:%s\\nFIRST:%s\\nSECOND:%s\\nTHIRD:%s\\nFOURTH:%s' \"$preamble\" \"$first\" \"$second\" \"$third\" \"$fourth\"; sleep 30"],
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
        json!({
            "group_id":group_id,"by":"user","to":["peer1"],"text":"one",
            "message_mode":"send"
        }),
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
    assert_eq!(first.result["message_mode"], "send");
    assert_eq!(second.result["message_mode"], "send");
    assert!(notify.result.get("delivery").is_none());
    assert_eq!(notify.result["event"]["data"]["im_visibility"], "internal");
    assert_eq!(reply.result["message_mode"], "send");
    let first_event_id = first.result["event"]["id"]
        .as_str()
        .expect("first event id");
    let second_event_id = second.result["event"]["id"]
        .as_str()
        .expect("second event id");
    let reply_event_id = reply.result["event"]["id"]
        .as_str()
        .expect("reply event id");

    wait_for(
        &client,
        &group_id,
        &format!("[event_id={reply_event_id} message_mode=send reply_to={first_event_id}]"),
    )
    .await;
    let tail = daemon_call(
        &client,
        "terminal_tail",
        json!({"group_id":group_id,"actor_id":"peer1","strip_ansi":false}),
    )
    .await;
    let text = tail.result["text"].as_str().unwrap_or_default();
    assert!(text.contains("PREAMBLE:[CCCC] You are peer1"));
    assert!(text.contains(&format!(
        "FIRST:[cccc] user → peer1 [event_id={first_event_id} message_mode=send]: one"
    )));
    assert!(text.contains(&format!(
        "SECOND:[cccc] user → peer1 [event_id={second_event_id} message_mode=send]: two"
    )));
    assert!(text.contains("THIRD:[cccc] SYSTEM (info): notice"));
    assert!(text.contains(&format!(
        "FOURTH:[cccc] user → peer1 (reply:{}) [event_id={reply_event_id} message_mode=send reply_to={first_event_id}]",
        &first_event_id[..8]
    )));
    assert!(text.contains("> \"one\": fix it"));

    let inbox = daemon_call(
        &client,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1","limit":10}),
    )
    .await;
    assert_eq!(inbox.result["messages"], json!([]));
    assert!(inbox.result["event"].is_null());
    let empty = daemon_call(
        &client,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    )
    .await;
    assert_eq!(empty.result["messages"], json!([]));

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
        json!({
            "group_id":group_id,"by":"user","to":[],"text":"foreman only",
            "message_mode":"send"
        }),
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
        json!({
            "group_id":group_id,"by":"user","to":[],"text":"everyone",
            "message_mode":"send"
        }),
    );
    assert_eq!(broadcast.result["event"]["data"]["to"], json!(["@all"]));

    let actor_message = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"lead","to":[],"text":"status update",
            "message_mode":"send"
        }),
    );
    assert_eq!(actor_message.result["event"]["data"]["to"], json!(["@all"]));
}

#[test]
fn send_reply_and_inbox_enforce_audience_domains() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"audience-domain-test","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    for actor_id in ["peer1", "peer2"] {
        call(
            &home,
            "actor_add",
            json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
        );
    }

    let mail = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["peer1"],"text":"read later",
            "message_mode":"mail"
        }),
    );
    assert_eq!(mail.result["event"]["data"]["message_mode"], "mail");
    let user_request = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"peer1","to":["user"],"text":"please decide",
            "message_mode":"request_reply"
        }),
    );

    for mode in ["send", "request_reply", "mail"] {
        let response = call_raw(
            &home,
            "send",
            json!({
                "group_id":group_id,"by":"peer1","to":["user","peer2"],
                "text":"split this audience","message_mode":mode
            }),
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("mixed_recipient_kinds")
        );
    }

    let mail_to_user = call_raw(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"peer1","to":["user"],"text":"invalid mail",
            "message_mode":"mail"
        }),
    );
    assert_eq!(
        mail_to_user.error.as_ref().map(|error| error.code.as_str()),
        Some("mail_requires_actor_recipient")
    );

    let reply_mail_to_user = call_raw(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"peer1","to":["user"],
            "reply_to":user_request.result["event"]["id"],"text":"invalid reply mail",
            "message_mode":"mail"
        }),
    );
    assert_eq!(
        reply_mail_to_user
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("mail_requires_actor_recipient")
    );

    for op in ["inbox_peek", "inbox_read"] {
        let response = call_raw(
            &home,
            op,
            json!({"group_id":group_id,"actor_id":"user","by":"user"}),
        );
        assert_eq!(
            response.error.as_ref().map(|error| error.code.as_str()),
            Some("invalid_inbox_recipient")
        );
    }
    let history = call(
        &home,
        "message_history",
        json!({"group_id":group_id,"actor_id":"user","by":"user"}),
    );
    assert!(
        history.result["messages"]
            .as_array()
            .is_some_and(|messages| !messages.is_empty())
    );
}

#[test]
fn inbox_read_consumes_only_mail_in_append_order() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group_id = stopped_peer_group(&home, "consume inbox");
    let notify = call(
        &home,
        "system_notify",
        json!({
            "group_id":group_id,"by":"system","title":"notify first",
            "message":"notify first","target_actor_id":"peer1"
        }),
    );
    let mail = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["peer1"],"text":"mail second",
            "message_mode":"mail"
        }),
    );

    let peek = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1","limit":10}),
    );
    assert_eq!(
        peek.result["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|event| event["id"].as_str().expect("event id"))
            .collect::<Vec<_>>(),
        vec![mail.result["event"]["id"].as_str().expect("mail id")]
    );

    let first = call(
        &home,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1","limit":1}),
    );
    assert_eq!(
        first.result["messages"][0]["id"],
        mail.result["event"]["id"]
    );
    assert_eq!(first.result["event"]["kind"], "mail.read");

    let second = call(
        &home,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1","limit":10}),
    );
    assert_eq!(second.result["messages"], json!([]));
    assert!(second.result["event"].is_null());
    assert_eq!(
        first.result["cursor"]["event_id"],
        mail.result["event"]["id"]
    );
    assert_ne!(notify.result["event"]["id"], mail.result["event"]["id"]);

    let empty = call(
        &home,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    );
    assert_eq!(empty.result["messages"], json!([]));
    assert!(empty.result["event"].is_null());
}

#[test]
fn message_history_pages_and_filters_without_consuming_mail() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group_id = stopped_peer_group(&home, "message history");
    let direct = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["peer1"],"text":"direct update",
            "message_mode":"send"
        }),
    );
    let requested = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["peer1"],"text":"please answer",
            "message_mode":"request_reply"
        }),
    );
    let mail = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["peer1"],"text":"read later",
            "message_mode":"mail"
        }),
    );

    let first_page = call(
        &home,
        "message_history",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1","limit":2}),
    );
    assert_eq!(
        first_page.result["messages"]
            .as_array()
            .expect("history")
            .iter()
            .map(|event| event["id"].as_str().expect("event id"))
            .collect::<Vec<_>>(),
        vec![
            mail.result["event"]["id"].as_str().expect("mail id"),
            requested.result["event"]["id"]
                .as_str()
                .expect("requested id")
        ]
    );
    assert_eq!(first_page.result["has_more"], true);

    let older = call(
        &home,
        "message_history",
        json!({
            "group_id":group_id,"actor_id":"peer1","by":"peer1",
            "before_event_id":mail.result["event"]["id"],"limit":10
        }),
    );
    assert_eq!(
        older.result["messages"]
            .as_array()
            .expect("older history")
            .iter()
            .map(|event| event["id"].as_str().expect("event id"))
            .collect::<Vec<_>>(),
        vec![
            requested.result["event"]["id"]
                .as_str()
                .expect("requested id"),
            direct.result["event"]["id"].as_str().expect("direct id")
        ]
    );
    assert_eq!(older.result["has_more"], false);

    let searched = call(
        &home,
        "message_history",
        json!({
            "group_id":group_id,"actor_id":"peer1","by":"peer1",
            "query":"PLEASE ANSWER","limit":10
        }),
    );
    assert_eq!(
        searched.result["messages"][0]["id"],
        requested.result["event"]["id"]
    );
    assert_eq!(
        searched.result["messages"].as_array().map(Vec::len),
        Some(1)
    );

    let inbox = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    );
    assert_eq!(
        inbox.result["messages"][0]["id"],
        mail.result["event"]["id"]
    );
    assert_eq!(inbox.result["messages"].as_array().map(Vec::len), Some(1));
}

#[test]
fn inbox_read_rejects_a_non_object_cursor_document_without_overwriting_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group_id = stopped_peer_group(&home, "malformed cursor");
    call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["peer1"],"text":"read me",
            "message_mode":"mail"
        }),
    );
    let cursor_path = GroupStore::new(home.clone())
        .expect("store")
        .state_dir(&group_id)
        .expect("state dir")
        .join("read_cursors.json");
    std::fs::write(&cursor_path, b"[]").expect("non-object cursor fixture");

    let response = call_raw(
        &home,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    );

    assert!(!response.ok);
    assert_eq!(
        std::fs::read_to_string(cursor_path).expect("preserved cursor document"),
        "[]"
    );
}

#[test]
fn actor_generation_bounds_inbox_read_and_reply_status() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"actor generation order","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    call(
        &home,
        "group_stop",
        json!({"group_id":group_id,"by":"user"}),
    );
    let store = GroupStore::new(home.clone()).expect("store");
    let ledger_path = store.ledger_path(&group_id).expect("ledger");
    let message = |timestamp: &str, text: &str, mode: &str| {
        let mut event = Event::new("chat.message", &group_id);
        event.ts = timestamp.into();
        event.by = "user".into();
        event.data = json!({
            "text":text,"to":["peer1"],"message_mode":mode
        })
        .as_object()
        .cloned()
        .expect("message data");
        event
    };
    let before_actor = message("2999-01-01T00:00:00Z", "before actor", "mail");
    ledger::append(&ledger_path, &before_actor).expect("pre-actor append");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
    let after_actor = message("2000-01-01T00:00:00Z", "after actor", "mail");
    ledger::append(&ledger_path, &after_actor).expect("post-actor append");
    let reply_request = message("1999-01-01T00:00:00Z", "reply after actor", "request_reply");
    ledger::append(&ledger_path, &reply_request).expect("reply request append");

    let inbox = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1","limit":10}),
    );
    assert_eq!(
        inbox.result["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|event| event["id"].as_str().expect("event id"))
            .collect::<Vec<_>>(),
        vec![after_actor.id.as_str()]
    );

    let statuses = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[before_actor.id,after_actor.id,reply_request.id]}),
    );
    let old = &statuses.result["statuses"][&before_actor.id];
    assert!(old["read_status"].get("peer1").is_none());
    assert!(old["obligation_status"].get("peer1").is_none());
    let current = &statuses.result["statuses"][&after_actor.id];
    assert_eq!(current["read_status"]["peer1"], false);
    assert_eq!(
        current["obligation_status"]["peer1"]["reply_requested"],
        false
    );
    assert!(current.get("ack_status").is_none());
    let requested = &statuses.result["statuses"][&reply_request.id];
    assert!(requested.get("read_status").is_none());
    assert_eq!(
        requested["obligation_status"]["peer1"]["reply_requested"],
        true
    );
    let history = call(
        &home,
        "message_history",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    );
    assert_eq!(
        history.result["messages"]
            .as_array()
            .expect("history")
            .iter()
            .map(|event| event["id"].as_str().expect("event id"))
            .collect::<Vec<_>>(),
        vec![reply_request.id.as_str(), after_actor.id.as_str()]
    );

    let consumed = call(
        &home,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    );
    assert_eq!(consumed.result["cursor"]["event_id"], after_actor.id);
    call(
        &home,
        "actor_remove",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
    let cursor_path = store
        .state_dir(&group_id)
        .expect("state dir")
        .join("read_cursors.json");
    if cursor_path.exists() {
        std::fs::remove_file(cursor_path).expect("remove cursor state");
    }
    let after_recreate = call(
        &home,
        "inbox_peek",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1","limit":10}),
    );
    assert_eq!(after_recreate.result["messages"], json!([]));
    let history_after_recreate = call(
        &home,
        "message_history",
        json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
    );
    assert_eq!(history_after_recreate.result["messages"], json!([]));
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
            "message_mode":"send",
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
        json!({
            "group_id":group_id,"by":"user","to":["worker"],
            "message_mode":"send","paths":[outside]
        }),
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
fn send_files_preflights_rejection_and_replay_before_blob_storage() {
    let temp = tempfile::tempdir().expect("tempdir");
    let scope = temp.path().join("scope");
    std::fs::create_dir_all(&scope).expect("scope");
    std::fs::write(scope.join("payload.bin"), b"accepted payload").expect("payload");
    std::fs::write(scope.join("duplicate.bin"), b"must not be stored").expect("duplicate");

    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"send-files-preflight","by":"user"}),
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
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
    let store = GroupStore::new(home.clone()).expect("store");
    let blob_dir = store
        .state_dir(group_id)
        .expect("state directory")
        .join("blobs");
    let blob_files = || {
        let mut files = std::fs::read_dir(&blob_dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        files.sort();
        files
    };

    let invalid_mode = call_raw(
        &home,
        "send_files",
        json!({
            "group_id":group_id,"paths":["payload.bin"],"by":"user",
            "to":["user"],"message_mode":"urgent"
        }),
    );
    assert_eq!(
        invalid_mode.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_message_mode")
    );
    assert!(blob_files().is_empty());

    for (recipients, mode, expected_code) in [
        (json!(["user"]), "mail", "mail_requires_actor_recipient"),
        (json!(["user", "peer1"]), "send", "mixed_recipient_kinds"),
    ] {
        let invalid_audience = call_raw(
            &home,
            "send_files",
            json!({
                "group_id":group_id,"paths":["payload.bin"],"by":"user",
                "to":recipients,"message_mode":mode
            }),
        );
        assert_eq!(
            invalid_audience
                .error
                .as_ref()
                .map(|error| error.code.as_str()),
            Some(expected_code)
        );
        assert!(blob_files().is_empty());
    }

    let rejected = call_raw(
        &home,
        "send_files",
        json!({
            "group_id":group_id,"paths":["payload.bin"],"by":"user",
            "to":["missing-actor"],"message_mode":"send"
        }),
    );
    assert_eq!(
        rejected.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_recipient")
    );
    assert!(blob_files().is_empty());

    let sent = call(
        &home,
        "send_files",
        json!({
            "group_id":group_id,"paths":["payload.bin"],"by":"user",
            "to":["user"],"message_mode":"send","client_id":"send-files-preflight-key"
        }),
    );
    let event_id = sent.result["event"]["id"]
        .as_str()
        .expect("event id")
        .to_owned();
    let blobs_after_send = blob_files();
    assert_eq!(blobs_after_send.len(), 1);

    let replayed = call(
        &home,
        "send_files",
        json!({
            "group_id":group_id,"paths":["duplicate.bin"],"by":"user",
            "to":["user"],"message_mode":"send","client_id":"send-files-preflight-key"
        }),
    );
    assert_eq!(replayed.result["event"]["id"], event_id);
    assert_eq!(blob_files(), blobs_after_send);
    assert_eq!(
        ledger::read_all(&store.ledger_path(group_id).expect("ledger"))
            .expect("events")
            .into_iter()
            .filter(|event| event.kind == "chat.message")
            .count(),
        1
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
        "client_id":"slash-client-1",
        "reply_to":"event-original",
        "quote_text":"原始请求"
    });
    let first = call(&home, "slash_skill_dispatch", args.clone());
    assert_eq!(first.result["hidden"], true);
    assert_eq!(first.result["accepted"], true);
    assert_eq!(first.result["message_mode"], "send");
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
        json!({
            "group_id":group_id,"by":"user","to":["lead"],"text":"question",
            "message_mode":"send"
        }),
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

    let mail_request = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"peer1","to":["lead"],"text":"reply later",
            "message_mode":"request_reply"
        }),
    );
    let mail_reply = call(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"lead","to":["peer1"],
            "reply_to":mail_request.result["event"]["id"],"text":"mail answer",
            "message_mode":"mail"
        }),
    );
    assert_eq!(mail_reply.result["message_mode"], "mail");
    assert_eq!(mail_reply.result["event"]["data"]["message_mode"], "mail");
    let mail_status = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[mail_request.result["event"]["id"]]}),
    );
    let mail_request_id = mail_request.result["event"]["id"]
        .as_str()
        .expect("mail request id");
    assert_eq!(
        mail_status.result["statuses"][mail_request_id]["obligation_status"]["lead"]["replied"],
        true
    );

    let nested = call_raw(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"lead","to":["user"],
            "reply_to":user_message_id,"text":"nested request",
            "message_mode":"request_reply"
        }),
    );
    assert_eq!(
        nested.error.as_ref().map(|error| error.code.as_str()),
        Some("invalid_message_mode")
    );

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
        json!({
            "group_id":group_id,"by":"lead","to":["peer1"],"text":"update",
            "message_mode":"send"
        }),
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
            "message_mode":"mail","require_peer_insight":true
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
            "message_mode":"send","require_peer_insight":true
        }),
    );
    assert!(user_only.ok);

    let accepted = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"lead","to":["peer1"],"text":"work",
            "message_mode":"mail","insight":"  reconsider the dependency boundary  ","require_peer_insight":true
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
            "message_mode":"mail","require_peer_insight":true
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

    for (to, mode, expected_code) in [
        (json!(["user", "reviewer"]), "send", "mixed_recipient_kinds"),
        (json!(["user"]), "mail", "mail_requires_actor_recipient"),
    ] {
        let rejected = call_raw(
            &home,
            "send_cross_group",
            json!({
                "group_id":source_id,"dst_group_id":destination_id,"by":"user",
                "to":to,"text":"invalid audience","message_mode":mode
            }),
        );
        assert_eq!(
            rejected.error.as_ref().map(|error| error.code.as_str()),
            Some(expected_code)
        );
    }
    assert_eq!(
        ledger::read_all(&source_ledger)
            .expect("source events after audience rejection")
            .len(),
        before_source
    );
    assert_eq!(
        ledger::read_all(&destination_ledger)
            .expect("destination events after audience rejection")
            .len(),
        before_destination
    );

    let rejected = call_raw(
        &home,
        "send_cross_group",
        json!({
            "group_id":source_id,"dst_group_id":destination_id,"by":"user",
            "to":["reviewer"],"text":"review this","message_mode":"mail",
            "require_peer_insight":true
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
            "message_mode":"mail","require_peer_insight":true,"client_id":"cross-group-1"
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
            "message_mode":"mail","client_id":"cross-group-1"
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
fn local_cross_group_reply_request_cancellation_reaches_the_relayed_event_once() {
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
    let sent = call(
        &home,
        "send_cross_group",
        json!({
            "group_id":source_id,"dst_group_id":destination_id,"by":"user",
            "to":["user"],"text":"please answer","message_mode":"request_reply"
        }),
    );
    let source_event_id = sent.result["src_event"]["id"]
        .as_str()
        .expect("source event id");
    let destination_event_id = sent.result["dst_event"]["id"]
        .as_str()
        .expect("destination event id");

    let cancelled = call(
        &home,
        "reply_request_cancel",
        json!({"group_id":source_id,"source_event_id":source_event_id,"by":"user"}),
    );
    assert_eq!(cancelled.result["propagation"]["transport"], "local");
    let cancel_event_id = cancelled.result["event"]["id"]
        .as_str()
        .expect("cancel event id");
    let destination_ledger = GroupStore::new(home.clone())
        .and_then(|store| store.ledger_path(destination_id))
        .expect("destination ledger");
    let propagated = ledger::read_all(&destination_ledger)
        .expect("destination events")
        .into_iter()
        .filter(|event| event.kind == "chat.reply_request.cancelled")
        .collect::<Vec<_>>();
    assert_eq!(propagated.len(), 1);
    assert_eq!(propagated[0].data["source_event_id"], destination_event_id);
    assert_eq!(propagated[0].data["src_event_id"], cancel_event_id);
    assert_eq!(propagated[0].data["src_message_event_id"], source_event_id);

    let replay = call(
        &home,
        "reply_request_cancel",
        json!({"group_id":source_id,"source_event_id":source_event_id,"by":"user"}),
    );
    assert_eq!(replay.result["duplicate"], true);
    assert_eq!(
        ledger::read_all(&destination_ledger)
            .expect("destination events after replay")
            .into_iter()
            .filter(|event| event.kind == "chat.reply_request.cancelled")
            .count(),
        1
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
            "to":["reviewer"],"text":"review this","message_mode":"mail",
            "require_peer_insight":true
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
            "message_mode":"mail","insight":"The remote reviewer owns the requested decision."
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
    assert_eq!(
        accepted.result["source_event"]["data"]["message_mode"],
        "send"
    );
    assert_eq!(
        accepted.result["source_event"]["data"]["dst_message_mode"],
        "mail"
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
    let legacy_message_priority = call_raw(
        &home,
        "tracked_send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"title":"Rejected",
            "text":"legacy message priority","message_priority":"urgent"
        }),
    );
    assert_eq!(
        legacy_message_priority
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("unsupported_message_fields")
    );
    let legacy_priority = call_raw(
        &home,
        "tracked_send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"title":"Rejected",
            "text":"legacy priority","priority":"attention"
        }),
    );
    assert_eq!(
        legacy_priority
            .error
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("unsupported_message_fields")
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
        "task_priority":" normal ",
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
    assert_eq!(first.result["event"]["data"]["message_mode"], "send");
    assert!(first.result["event"]["data"].get("priority").is_none());
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
fn manual_delivery_resumes_a_paused_group_after_claiming() {
    use cccc_contracts::GroupState;

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"manual-delivery-paused","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"worker","by":"user"}),
    );
    let source = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"text":"later",
            "message_mode":"mail"
        }),
    );
    let store = GroupStore::new(home.clone()).expect("store");
    store
        .mutate(group_id, |group| {
            group.state = GroupState::Paused;
            Ok(())
        })
        .expect("pause group");

    let response = call_raw(
        &home,
        "message_deliver",
        json!({
            "group_id":group_id,"by":"user","actor_ids":["worker"],
            "source_event_id":source.result["event"]["id"]
        }),
    );
    assert!(response.ok, "manual delivery failed: {response:?}");
    assert_eq!(response.result["delivery_state"], "claimed");
    assert_eq!(
        store.load(group_id).expect("resumed group").state,
        GroupState::Active
    );
    let ledger_path = store.ledger_path(group_id).expect("ledger path");
    assert!(
        ledger::read_all(&ledger_path)
            .expect("ledger")
            .iter()
            .any(|event| event.kind == "runtime.delivery" && event.data["state"] == "claimed")
    );
}

#[test]
fn manual_delivery_is_blocked_without_a_claim_for_disabled_actor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"manual-delivery-disabled","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"worker","by":"user"}),
    );
    let source = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"text":"later",
            "message_mode":"mail"
        }),
    );
    call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,"actor_id":"worker","patch":{"enabled":false},"by":"user"
        }),
    );

    let response = call_raw(
        &home,
        "message_deliver",
        json!({
            "group_id":group_id,"by":"user","actor_ids":["worker"],
            "source_event_id":source.result["event"]["id"]
        }),
    );
    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("delivery_blocked")
    );
    assert_eq!(
        response
            .error
            .as_ref()
            .map(|error| &error.details["reason"]),
        Some(&json!("actor_disabled"))
    );
    let store = GroupStore::new(home.clone()).expect("store");
    let ledger_path = store.ledger_path(group_id).expect("ledger path");
    assert!(
        ledger::read_all(&ledger_path)
            .expect("ledger")
            .iter()
            .all(|event| event.kind != "runtime.delivery")
    );
}

#[test]
fn manual_delivery_reports_the_actual_conflicting_claim_without_partial_reservation() {
    use cccc_contracts::GroupState;

    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"manual-delivery-conflict","by":"user"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    for actor_id in ["peer1", "peer2"] {
        call(
            &home,
            "actor_add",
            json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
        );
    }
    let source = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["peer1","peer2"],"text":"later",
            "message_mode":"mail"
        }),
    );
    let source_event_id = source.result["event"]["id"]
        .as_str()
        .expect("source event id");
    let store = GroupStore::new(home.clone()).expect("store");
    let ledger_path = store.ledger_path(group_id).expect("ledger path");
    for (actor_id, state) in [("peer1", "ambiguous"), ("peer2", "claimed")] {
        let mut event = Event::new("runtime.delivery", group_id);
        event.by = "system".into();
        event.data = json!({
            "actor_id":actor_id,
            "source_event_id":source_event_id,
            "state":state,
            "transport":"manual_request"
        })
        .as_object()
        .cloned()
        .expect("delivery data");
        ledger::append(&ledger_path, &event).expect("append delivery state");
    }
    store
        .mutate(group_id, |group| {
            group.state = GroupState::Paused;
            Ok(())
        })
        .expect("pause group");

    let response = call_raw(
        &home,
        "message_deliver",
        json!({
            "group_id":group_id,"by":"user","actor_ids":["peer1","peer2"],
            "source_event_id":source_event_id,"force_ambiguous":true
        }),
    );

    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("delivery_in_progress")
    );
    assert_eq!(
        response
            .error
            .as_ref()
            .map(|error| &error.details["actor_id"]),
        Some(&json!("peer2"))
    );
    assert_eq!(
        ledger::read_all(&ledger_path)
            .expect("ledger")
            .iter()
            .filter(|event| event.kind == "runtime.delivery")
            .count(),
        2
    );
    assert_eq!(
        store.load(group_id).expect("unchanged group").state,
        GroupState::Paused
    );
}

#[test]
fn message_domain_contracts_cover_install_reply_idle_stream_and_validation() {
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
        json!({
            "group_id":group_id,"by":"user","to":["worker"],
            "text":"/install owner/repo","message_mode":"send"
        }),
    );
    assert_eq!(
        install.result["event"]["data"]["refs"][0]["capability_id"],
        "skill:cccc:install"
    );
    assert_eq!(
        store.load(group_id).expect("group").state,
        GroupState::Active
    );

    let reply_request = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"text":"respond",
            "message_mode":"request_reply"
        }),
    );
    let reply = call(
        &home,
        "reply",
        json!({
            "group_id":group_id,"by":"worker","reply_to":reply_request.result["event"]["id"],
            "text":"done"
        }),
    );
    assert_eq!(reply.result["event"]["data"]["message_mode"], "send");
    assert!(reply.result.get("ack_event").is_none());
    let cancelled_request = call(
        &home,
        "send",
        json!({
            "group_id":group_id,"by":"user","to":["worker"],"text":"cancel reply",
            "message_mode":"request_reply"
        }),
    );
    let cancelled = call(
        &home,
        "reply_request_cancel",
        json!({
            "group_id":group_id,"by":"user",
            "source_event_id":cancelled_request.result["event"]["id"]
        }),
    );
    assert_eq!(
        cancelled.result["event"]["kind"],
        "chat.reply_request.cancelled"
    );

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
            "message_mode":"send","attachments":[{"path":"state/blobs/missing"}]
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
            "message_mode":"send","path":temp.path().join("outside").to_string_lossy()
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
            "to":["target-lead"],"text":"schema","message_mode":"send"
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
            json!({"group_id":group_id,"actor_id":"peer1","strip_ansi":false}),
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

fn stopped_peer_group(home: &HomeLayout, title: &str) -> String {
    let created = call(home, "group_create", json!({"title":title,"by":"user"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    call(home, "group_stop", json!({"group_id":group_id,"by":"user"}));
    call(
        home,
        "actor_add",
        json!({
            "group_id":group_id,"actor_id":"peer1","runtime":"custom",
            "runner":"pty","command":["sh","-c","exit 0"],"by":"user"
        }),
    );
    group_id
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
