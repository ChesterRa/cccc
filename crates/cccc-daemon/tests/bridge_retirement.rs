use cccc_contracts::{DaemonRequest, DaemonResponse, Event};
use cccc_core::{GroupStore, HomeLayout, ledger};
use serde_json::{Value, json};

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().expect("args").clone(),
        },
    )
}

#[test]
fn retired_remote_replies_are_rejected_before_local_writes_even_with_colliding_ids() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("Home");
    let store = GroupStore::new(home.clone()).expect("store");
    let local = store.create("Local", "").expect("Group");
    let collision = store
        .create("Unrelated local destination", "")
        .expect("Group");
    let path = store.ledger_path(&local.group_id).expect("ledger");
    let mut source = Event::new("chat.message", &local.group_id);
    source.by = "user".into();
    source.data = json!({"text":"Old remote request", "to":["user"], "message_mode":"send", "dst_group_id":collision.group_id}).as_object().expect("data").clone();
    ledger::append(&path, &source).expect("original");
    let mut receipt = Event::new("chat.cross_group_receipt", &local.group_id);
    receipt.by = "system".into();
    receipt.data =
        json!({"source_event_id":source.id, "group_bridge_retired":true,"status":"unconfirmed"})
            .as_object()
            .expect("data")
            .clone();
    ledger::append(&path, &receipt).expect("receipt");
    let archive = path.parent().expect("group").join("state/ledger/segments");
    std::fs::create_dir_all(&archive).expect("archive");
    std::fs::rename(&path, archive.join("original.jsonl")).expect("rotate");
    for op in ["reply", "message_upload_preflight"] {
        let response = call(
            &home,
            op,
            json!({"group_id":local.group_id,"by":"user","reply_to":source.id,"operation":"reply","text":"must not be sent","has_attachments":true}),
        );
        assert!(!response.ok, "{op}");
        assert_eq!(response.error.expect("error").code, "group_bridge_retired");
    }
    assert_eq!(
        ledger::read_all(&path).expect("source history"),
        [source.clone(), receipt]
    );
    assert!(
        ledger::read_all(&store.ledger_path(&collision.group_id).expect("path"))
            .expect("other Group")
            .is_empty()
    );
    let statuses = call(
        &home,
        "ledger_statuses",
        json!({"group_id":local.group_id,"event_ids":[source.id]}),
    );
    assert!(statuses.ok);
    assert_eq!(
        statuses.result["statuses"][&source.id]["retired_bridge"],
        true
    );
    for op in [
        "remote_send",
        "group_bridge_session_open",
        "group_bridge_receive_remote_send",
        "send_cross_group_remote_record",
    ] {
        let response = call(&home, op, json!({"group_id":local.group_id}));
        assert!(!response.ok, "retired operation {op}");
        assert_eq!(response.error.expect("error").code, "unknown_op");
    }
}

#[test]
fn ordinary_local_cross_group_cancel_still_propagates_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("Home");
    let store = GroupStore::new(home.clone()).expect("store");
    let source = store.create("Source", "").expect("Group");
    let target = store.create("Target", "").expect("Group");
    let sent = call(
        &home,
        "send_cross_group",
        json!({"group_id":source.group_id,"dst_group_id":target.group_id,"by":"user","to":["user"],"text":"Please reply","message_mode":"request_reply"}),
    );
    assert!(sent.ok, "{:?}", sent.error);
    let source_id = &sent.result["source_event"]["id"];
    for _ in 0..2 {
        let response = call(
            &home,
            "reply_request_cancel",
            json!({"group_id":source.group_id,"by":"user","source_event_id":source_id}),
        );
        assert!(response.ok, "{:?}", response.error);
        assert_eq!(response.result["propagation"]["state"], "sent");
    }
    let events =
        ledger::read_all(&store.ledger_path(&target.group_id).expect("path")).expect("history");
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == "chat.reply_request.cancelled")
            .count(),
        1
    );
}
