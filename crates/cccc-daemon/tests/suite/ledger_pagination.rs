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
fn ledger_statuses_restore_read_ack_and_reply_state() {
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
            "priority":"attention",
            "reply_required":true
        }),
    );
    let event_id = sent.result["event"]["id"]
        .as_str()
        .expect("event id")
        .to_owned();
    call(
        &home,
        "send",
        json!({
            "group_id":group_id,
            "by":"user",
            "to":[],
            "text":"proxied user message",
            "reply_to":event_id,
            "actor_id":"alice"
        }),
    );

    let before = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[event_id]}),
    );
    assert_eq!(
        before.result["statuses"][&event_id]["read_status"]["alice"],
        false
    );
    assert_eq!(
        before.result["statuses"][&event_id]["ack_status"]["alice"],
        false
    );
    assert_eq!(
        before.result["statuses"][&event_id]["obligation_status"]["alice"]["replied"],
        false
    );
    assert!(
        before.result["statuses"][&event_id]["read_status"]
            .get("bob")
            .is_none()
    );

    call(
        &home,
        "inbox_mark_read",
        json!({"group_id":group_id,"actor_id":"alice","event_id":event_id,"by":"user"}),
    );
    call(
        &home,
        "reply",
        json!({
            "group_id":group_id,
            "reply_to":event_id,
            "text":"done",
            "by":"alice"
        }),
    );

    let after = call(
        &home,
        "ledger_statuses",
        json!({"group_id":group_id,"event_ids":[event_id]}),
    );
    let status = &after.result["statuses"][&event_id];
    assert_eq!(status["read_status"]["alice"], true);
    assert_eq!(status["ack_status"]["alice"], true);
    assert_eq!(status["obligation_status"]["alice"]["read"], true);
    assert_eq!(status["obligation_status"]["alice"]["acked"], true);
    assert_eq!(status["obligation_status"]["alice"]["replied"], true);
    assert_eq!(status["obligation_status"]["alice"]["reply_required"], true);

    let single = call(
        &home,
        "message_read_status",
        json!({"group_id":group_id,"event_id":event_id}),
    );
    assert_eq!(single.result["read_status"]["alice"], true);
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

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}
