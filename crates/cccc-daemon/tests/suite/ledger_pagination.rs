// Included by the crate-level integration test harness.
use cccc_contracts::{DaemonRequest, DaemonResponse, Event};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn chat_pagination_filters_noise_before_applying_limit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"pagination"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");

    for index in 0..5 {
        append(&home, group_id, "chat.message", &format!("chat-{index}"));
        for noise in 0..30 {
            append(
                &home,
                group_id,
                "actor.activity",
                &format!("noise-{index}-{noise}"),
            );
        }
    }

    let tail = call(
        &home,
        "ledger_tail",
        json!({"group_id":group_id,"kind":"chat","limit":2}),
    );
    assert_eq!(ids(&tail), ["chat-3", "chat-4"]);
    assert_eq!(tail.result["count"], 2);
    assert_eq!(tail.result["has_more"], true);

    let older = call(
        &home,
        "ledger_search",
        json!({"group_id":group_id,"kind":"chat","before":"chat-3","limit":2}),
    );
    assert_eq!(ids(&older), ["chat-1", "chat-2"]);
    assert_eq!(older.result["has_more"], true);

    let oldest = call(
        &home,
        "ledger_search",
        json!({"group_id":group_id,"kind":"chat","before":"chat-1","limit":2}),
    );
    assert_eq!(ids(&oldest), ["chat-0"]);
    assert_eq!(oldest.result["has_more"], false);
}

#[test]
fn sender_union_uses_one_history_cursor_and_filters_before_limiting() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"sender union"}));
    let group_id = created.result["group"]["group_id"].as_str().expect("group");
    for index in 0..10 {
        let mut event = Event::new("chat.message", group_id);
        event.id = format!("message-{index}");
        event.by = ["lead", "other", "peer"][index % 3].into();
        event.data.insert("text".into(), json!("matching text"));
        call(&home, "event_append", json!({"event":event}));
        append(&home, group_id, "actor.activity", &format!("noise-{index}"));
    }
    let query = json!({"group_id":group_id,"kind":"chat","by_any":["lead","peer"],"limit":3});
    let latest = call(&home, "ledger_search", query.clone());
    assert_eq!(ids(&latest), ["message-6", "message-8", "message-9"]);
    assert_eq!(latest.result["has_more"], true);
    let mut older = query.clone();
    older["before"] = json!("message-6");
    let older = call(&home, "ledger_search", older);
    assert_eq!(ids(&older), ["message-2", "message-3", "message-5"]);
    let mut oldest = query.clone();
    oldest["before"] = json!("message-2");
    let oldest = call(&home, "ledger_search", oldest);
    assert_eq!(ids(&oldest), ["message-0"]);
    assert_eq!(oldest.result["has_more"], false);
    let mut after = query.clone();
    after["after"] = json!("message-3");
    after["limit"] = json!(2);
    let after = call(&home, "ledger_search", after);
    assert_eq!(ids(&after), ["message-5", "message-6"]);
    assert_eq!(after.result["has_more"], true);
    let mut exact = query;
    exact["by"] = json!("lead");
    assert_eq!(
        ids(&call(&home, "ledger_search", exact)),
        ["message-3", "message-6", "message-9"]
    );
}

#[test]
fn invalid_sender_collections_do_not_silently_remove_the_filter() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"invalid senders"}));
    let group_id = created.result["group"]["group_id"].as_str().expect("group");
    for value in [json!("lead"), json!(["lead", 1]), json!([""])] {
        let response = call_raw(
            &home,
            "ledger_search",
            json!({"group_id":group_id,"by_any":value}),
        );
        assert!(!response.ok);
        assert_eq!(
            response.error.expect("invalid senders").code,
            "invalid_args"
        );
    }
    let empty = call(
        &home,
        "ledger_search",
        json!({"group_id":group_id,"kind":"chat","by_any":[]}),
    );
    assert!(ids(&empty).is_empty());
}

#[test]
fn message_window_is_centered_and_reports_both_directions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"window"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    for index in 0..7 {
        append(&home, group_id, "chat.message", &format!("chat-{index}"));
    }

    let window = call(
        &home,
        "ledger_window",
        json!({"group_id":group_id,"kind":"chat","center":"chat-3","before":2,"after":2}),
    );
    assert_eq!(
        ids(&window),
        ["chat-1", "chat-2", "chat-3", "chat-4", "chat-5"]
    );
    assert_eq!(window.result["center_index"], 2);
    assert_eq!(window.result["has_more_before"], true);
    assert_eq!(window.result["has_more_after"], true);
}

#[test]
fn reply_persists_quote_snapshot_for_refresh_rendering() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"reply quote"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"lead","by":"user"}),
    );
    append(&home, group_id, "chat.message", "target");

    let reply = call(
        &home,
        "reply",
        json!({
            "group_id":group_id,
            "reply_to":"target",
            "text":"response",
            "by":"lead"
        }),
    );

    assert_eq!(reply.result["event"]["data"]["reply_to"], "target");
    assert_eq!(reply.result["event"]["data"]["quote_text"], "target");
}

#[test]
fn message_persists_immutable_sender_snapshot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"sender snapshot"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let avatar = cccc_core::blobs::store(&home, group_id, b"original avatar").expect("avatar");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"alice",
            "title":"Original title",
            "runtime":"claude",
            "avatar_asset_path":avatar.path,
            "by":"user"
        }),
    );

    let sent = call(
        &home,
        "send",
        json!({
            "group_id":group_id,
            "by":"alice",
            "to":["user"],
            "text":"hello",
            "message_mode":"send",
            "sender_title":"Spoofed title",
            "sender_runtime":"custom",
            "sender_avatar_path":"state/blobs/spoofed"
        }),
    );
    let event_id = sent.result["event"]["id"].as_str().expect("event id");
    call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"alice",
            "title":"Changed title",
            "avatar_asset_path":"",
            "by":"user"
        }),
    );

    let window = call(
        &home,
        "ledger_window",
        json!({"group_id":group_id,"kind":"chat","center":event_id,"before":0,"after":0}),
    );
    let data = &window.result["events"][0]["data"];
    assert_eq!(data["sender_title"], "Original title");
    assert_eq!(data["sender_runtime"], "claude");
    assert_eq!(data["sender_avatar_path"], avatar.path);
}

#[test]
fn ledger_statuses_restore_read_and_reply_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"message statuses"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    for actor_id in ["alice", "bob"] {
        call(
            &home,
            "actor_add",
            json!({"group_id":group_id,"actor_id":actor_id,"by":"user"}),
        );
    }
    let sent = call(
        &home,
        "send",
        json!({
            "group_id":group_id,
            "by":"user",
            "to":["alice"],
            "text":"please respond",
            "message_mode":"request_reply"
        }),
    );
    let request_event_id = sent.result["event"]["id"]
        .as_str()
        .expect("event id")
        .to_owned();
    let mail = call(
        &home,
        "send",
        json!({
            "group_id":group_id,
            "by":"user",
            "to":["alice"],
            "text":"read when ready",
            "message_mode":"mail"
        }),
    );
    let mail_event_id = mail.result["event"]["id"]
        .as_str()
        .expect("Mail event id")
        .to_owned();
    call(
        &home,
        "send",
        json!({
            "group_id":group_id,
            "by":"user",
            "to":[],
            "text":"proxied user message",
            "reply_to":request_event_id,
            "actor_id":"alice",
            "message_mode":"send"
        }),
    );

    let before = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[request_event_id,mail_event_id]}),
    );
    assert_eq!(
        before.result["statuses"][&mail_event_id]["read_status"]["alice"],
        false
    );
    assert_eq!(
        before.result["statuses"][&request_event_id]["obligation_status"]["alice"]["replied"],
        false
    );
    assert!(
        before.result["statuses"][&request_event_id]
            .get("read_status")
            .is_none()
    );
    assert!(
        before.result["statuses"][&mail_event_id]["read_status"]
            .get("bob")
            .is_none()
    );

    call(
        &home,
        "inbox_read",
        json!({"group_id":group_id,"actor_id":"alice","limit":1,"by":"user"}),
    );
    let after_read = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[request_event_id,mail_event_id]}),
    );
    assert_eq!(
        after_read.result["statuses"][&mail_event_id]["read_status"]["alice"],
        true
    );
    assert_eq!(
        after_read.result["statuses"][&request_event_id]["obligation_status"]["alice"]["replied"],
        false
    );
    assert!(
        after_read.result["statuses"][&request_event_id]
            .get("read_status")
            .is_none()
    );
    call(
        &home,
        "reply",
        json!({
            "group_id":group_id,
            "reply_to":request_event_id,
            "text":"done",
            "by":"alice"
        }),
    );

    let after = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[request_event_id,mail_event_id]}),
    );
    assert_eq!(
        after.result["statuses"][&mail_event_id]["read_status"]["alice"],
        true
    );
    let status = &after.result["statuses"][&request_event_id];
    assert!(status.get("read_status").is_none());
    assert_eq!(status["obligation_status"]["alice"]["replied"], true);
    assert_eq!(
        status["obligation_status"]["alice"]["reply_requested"],
        true
    );
    assert!(status.get("ack_status").is_none());

    let single = call(
        &home,
        "message_read_status",
        json!({"group_id":group_id,"event_id":mail_event_id}),
    );
    assert_eq!(single.result["read_status"]["alice"], true);
}

#[test]
fn ledger_and_context_reads_reject_a_missing_group() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    for (op, args) in [
        ("ledger_tail", json!({"group_id":"g_missing"})),
        ("ledger_search", json!({"group_id":"g_missing","q":"x"})),
        (
            "ledger_window",
            json!({"group_id":"g_missing","center":"event-missing"}),
        ),
        (
            "ledger_statuses",
            json!({"group_id":"g_missing","event_ids":["event-missing"]}),
        ),
        (
            "message_read_status",
            json!({"group_id":"g_missing","event_id":"event-missing"}),
        ),
        (
            "context_get",
            json!({"group_id":"g_missing","detail":"summary"}),
        ),
        ("task_list", json!({"group_id":"g_missing"})),
    ] {
        let response = call_raw(&home, op, args);
        assert!(!response.ok, "{op} unexpectedly succeeded");
        assert_eq!(
            response.error.expect("missing-group error").code,
            "group_not_found",
            "{op} returned the wrong error"
        );
    }
}

fn append(home: &HomeLayout, group_id: &str, kind: &str, id: &str) {
    let mut event = Event::new(kind, group_id);
    event.id = id.into();
    event.by = "user".into();
    event.data.insert("text".into(), json!(id));
    call(home, "event_append", json!({"event":event}));
}

fn ids(response: &DaemonResponse) -> Vec<&str> {
    response.result["events"]
        .as_array()
        .expect("events")
        .iter()
        .map(|event| event["id"].as_str().expect("event id"))
        .collect()
}

#[test]
fn ledger_maintenance_rejects_a_deleted_group_without_recreating_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"maintenance"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id")
        .to_owned();
    append(&home, &group_id, "chat.message", "before-delete");
    call(&home, "ledger_snapshot", json!({"group_id":group_id}));
    call(&home, "ledger_compact", json!({"group_id":group_id}));

    call(
        &home,
        "group_delete",
        json!({"group_id":group_id,"by":"user"}),
    );
    let group_dir = home.groups_dir().join(&group_id);
    assert!(!group_dir.exists());
    for op in ["ledger_snapshot", "ledger_compact"] {
        let response = call_raw(&home, op, json!({"group_id":group_id}));
        assert_eq!(
            response.error.map(|error| error.code).as_deref(),
            Some("group_not_found"),
            "{op} must reject a deleted Group"
        );
        assert!(
            !group_dir.exists(),
            "{op} must not recreate the deleted Group's directory"
        );
    }
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = call_raw(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn call_raw(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}

#[test]
fn recorded_ledger_recipient_and_time_filters_compose_with_pagination() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let created = call(&home, "group_create", json!({"title":"ledger filters"}));
    let group_id = created.result["group"]["group_id"].as_str().expect("group");
    let mut events: Vec<Event> =
        serde_json::from_str(include_str!("../fixtures/ledger-search/events.json"))
            .expect("recorded events");
    for event in &mut events {
        event.group_id = group_id.into();
        call(&home, "event_append", json!({"event":event}));
    }
    let selected = events
        .iter()
        .filter(|event| {
            event
                .data
                .get("to")
                .and_then(Value::as_array)
                .is_some_and(|to| to.contains(&json!("actor-03")))
        })
        .collect::<Vec<_>>();
    assert!(
        selected.len() >= 4,
        "recording must exercise multiple pages"
    );
    let since = &selected[1].ts;
    let until = &selected[selected.len() - 2].ts;
    let expected = selected
        .iter()
        .filter(|event| event.ts >= *since && event.ts <= *until)
        .map(|event| event.id.as_str())
        .collect::<Vec<_>>();
    let query = json!({"group_id":group_id,"kind":"chat","to":"actor-03","since_ts":since,"until_ts":until,"limit":1});
    let latest = call(&home, "ledger_search", query.clone());
    assert_eq!(ids(&latest), [*expected.last().expect("last")]);
    assert_eq!(latest.result["has_more"], true);
    let mut older = query.clone();
    older["before"] = json!(expected.last().expect("cursor"));
    assert_eq!(
        ids(&call(&home, "ledger_search", older)),
        [expected[expected.len() - 2]]
    );
    let mut forward = query.clone();
    forward["after"] = json!(events[0].id);
    assert_eq!(
        ids(&call(&home, "ledger_search", forward.clone())),
        [expected[0]]
    );
    forward["limit"] = json!(200);
    assert_eq!(
        ids(&call(&home, "ledger_search", forward.clone())),
        expected
    );
    forward["by_any"] = json!([selected[1].by]);
    let narrowed = selected
        .iter()
        .filter(|event| event.ts >= *since && event.ts <= *until && event.by == selected[1].by)
        .map(|event| event.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids(&call(&home, "ledger_search", forward)), narrowed);
    let only_since = call(
        &home,
        "ledger_search",
        json!({"group_id":group_id,"to":"actor-03","since_ts":since}),
    );
    assert_eq!(
        ids(&only_since),
        selected
            .iter()
            .skip(1)
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>()
    );
    let only_until = call(
        &home,
        "ledger_search",
        json!({"group_id":group_id,"to":"actor-03","until_ts":until}),
    );
    assert_eq!(
        ids(&only_until),
        selected
            .iter()
            .take(selected.len() - 1)
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>()
    );
    let wrong_sender = call(
        &home,
        "ledger_search",
        json!({"group_id":group_id,"to":"actor-03","by":"actor-01"}),
    );
    assert!(ids(&wrong_sender).is_empty());
    // RFC3339 offsets compare instants, not their string spelling.
    let boundary = chrono::DateTime::parse_from_rfc3339(since)
        .expect("timestamp")
        .with_timezone(&chrono::FixedOffset::east_opt(3600).expect("offset"))
        .to_rfc3339();
    let exact = call(
        &home,
        "ledger_search",
        json!({"group_id":group_id,"to":"actor-03","since_ts":boundary,"until_ts":since}),
    );
    assert_eq!(ids(&exact), [selected[1].id.as_str()]);
    let none = call(
        &home,
        "ledger_search",
        json!({"group_id":group_id,"to":"unknown-recipient"}),
    );
    assert!(ids(&none).is_empty());
}

#[test]
fn ledger_search_rejects_invalid_recipient_and_time_filters() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let created = call(&home, "group_create", json!({"title":"filter validation"}));
    let group_id = created.result["group"]["group_id"].as_str().expect("group");
    for patch in [
        json!({"to":17}),
        json!({"to":" "}),
        json!({"since_ts":"yesterday"}),
        json!({"until_ts":3}),
        json!({"since_ts":"2026-10-22T10:00:00Z","until_ts":"2026-10-22T09:00:00Z"}),
    ] {
        let mut args = patch.as_object().expect("object").clone();
        args.insert("group_id".into(), json!(group_id));
        let response = call_raw(&home, "ledger_search", Value::Object(args));
        assert!(!response.ok, "invalid filter accepted: {patch}");
        assert_eq!(response.error.expect("error").code, "invalid_args");
    }
}
