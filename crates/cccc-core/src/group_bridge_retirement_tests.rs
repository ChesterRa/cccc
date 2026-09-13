use super::*;

fn fixture() -> (tempfile::TempDir, HomeLayout, GroupStore, Event, Value) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("Home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Retirement fixture", "").expect("Group");
    let mut source = Event::new("chat.message", &group.group_id);
    source.by = "user".into();
    source.scope_key = "original-scope".into();
    source.data = json!({"text":"Keep the original message", "to":["user"]})
        .as_object()
        .expect("data")
        .clone();
    ledger::append(
        &store.ledger_path(&group.group_id).expect("ledger"),
        &source,
    )
    .expect("source");
    let record = json!({
        "operation":"remote_send", "status":"retrying", "attempt":1,
        "registration_id":"registration", "idempotency_key":"same-request",
        "src_group_id":group.group_id, "dst_group_id":"g_remote",
        "source_event_id":source.id,
        "payload":{"credential":"never-project-this","text":"old payload"}
    });
    (temp, home, store, source, record)
}

fn write_records(home: &HomeLayout, values: &[Value]) {
    let records: Map<String, Value> = values
        .iter()
        .enumerate()
        .map(|(i, value)| (i.to_string(), value.clone()))
        .collect();
    fs::write_yaml(&home.root().join(RECEIPTS), &json!({"receipts":records})).expect("receipts");
}

#[test]
fn retires_uncertain_work_without_replaying_or_exposing_payloads_and_preserves_identity() {
    let (_temp, home, store, source, record) = fixture();
    let key = home.root().join("group_bridge_identity_key.yaml");
    std::fs::write(&key, "private-key-must-not-change").expect("identity");
    for name in &RETIRED_FILES[..RETIRED_FILES.len() - 1] {
        std::fs::write(home.root().join(name), "old credentials and routes").expect("state");
    }
    write_records(&home, &[record]);
    settings::update(&home, |settings| {
        settings.extra.insert("unrelated".into(), json!("kept"));
        Ok(())
    })
    .expect("settings");
    retire(&home).expect("retire");
    assert_eq!(
        std::fs::read_to_string(&key).expect("identity"),
        "private-key-must-not-change"
    );
    assert!(
        RETIRED_FILES
            .iter()
            .all(|name| !home.root().join(name).exists())
    );
    assert_eq!(
        settings::load(&home).expect("settings").extra["unrelated"],
        "kept"
    );
    let events =
        ledger::read_all(&store.ledger_path(&source.group_id).expect("ledger")).expect("events");
    assert_eq!(events[0], source);
    let receipt = events
        .iter()
        .find(|event| event.kind == "chat.cross_group_receipt")
        .expect("receipt");
    assert_eq!(receipt.data["status"], "unconfirmed");
    assert_eq!(receipt.scope_key, "original-scope");
    let notice = events
        .iter()
        .find(|event| event.kind == "system.notify")
        .expect("notice");
    assert_eq!(notice.data["target_actor_id"], "user");
    assert_eq!(notice.data["im_visibility"], "internal");
    assert!(
        !serde_json::to_string(&events)
            .expect("serialized")
            .contains("never-project-this")
    );
    retire(&home).expect("second startup");
    assert_eq!(
        ledger::read_all(&store.ledger_path(&source.group_id).expect("ledger")).expect("events"),
        events
    );
}

#[test]
fn canonical_confirmation_wins_over_stale_shadow_and_queued_control_is_not_reported_sent() {
    let (_temp, home, store, source, mut record) = fixture();
    settings::update(&home, |settings| {
        settings.extra.insert(
            "group_bridge".into(),
            json!({"deliveries":[record.clone()]}),
        );
        Ok(())
    })
    .expect("shadow");
    record["status"] = json!("sent");
    record["remote_event_id"] = json!("confirmed-target-event");
    let mut cancel = Event::new("chat.reply_cancel", &source.group_id);
    cancel.by = "user".into();
    ledger::append(
        &store.ledger_path(&source.group_id).expect("ledger"),
        &cancel,
    )
    .expect("cancel");
    let control = json!({
        "operation":"reply_request_cancel", "status":"queued", "attempt":0,
        "registration_id":"registration", "idempotency_key":"cancel-request",
        "src_group_id":source.group_id, "dst_group_id":"g_remote",
        "source_event_id":cancel.id, "source_message_event_id":source.id
    });
    write_records(&home, &[record, control]);
    retire(&home).expect("retire");
    assert!(
        !settings::load(&home)
            .expect("settings")
            .extra
            .contains_key("group_bridge")
    );
    let events =
        ledger::read_all(&store.ledger_path(&source.group_id).expect("ledger")).expect("events");
    let receipts: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "chat.cross_group_receipt")
        .collect();
    assert_eq!(receipts.len(), 2);
    assert!(receipts.iter().any(|event| event.data["status"] == "sent"
        && event.data["remote_event_id"] == "confirmed-target-event"));
    assert!(receipts.iter().any(
        |event| event.data["status"] == "failed" && event.data["source_event_id"] == cancel.id
    ));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "system.notify")
            .count(),
        1
    );
}

#[test]
fn cleanup_failure_keeps_receipts_and_retry_recovers_from_archived_projections() {
    let (_temp, home, store, source, record) = fixture();
    write_records(&home, &[record]);
    let credentials = home.root().join(RETIRED_FILES[0]);
    std::fs::create_dir(&credentials).expect("inject removal failure");
    assert!(retire(&home).is_err());
    assert!(home.root().join(RECEIPTS).is_file());
    let path = store.ledger_path(&source.group_id).expect("ledger");
    let before = ledger::read_all(&path).expect("events");
    assert_eq!(before.len(), 3);
    // Use the same archive layout consumed by the canonical ledger reader.
    let archive = path
        .parent()
        .expect("Group directory")
        .join("state/ledger/segments");
    std::fs::create_dir_all(&archive).expect("archive");
    std::fs::rename(&path, archive.join("0001.jsonl")).expect("archive ledger");
    std::fs::remove_dir(&credentials).expect("repair failure");
    retire(&home).expect("recover");
    assert_eq!(ledger::read_all(&path).expect("history"), before);
    assert!(!home.root().join(RECEIPTS).exists());
}

#[test]
fn malformed_state_is_retained_without_erasing_credentials_or_history() {
    let (_temp, home, store, source, _) = fixture();
    std::fs::write(home.root().join(RECEIPTS), "not a receipt map").expect("corrupt fixture");
    std::fs::write(home.root().join(RETIRED_FILES[0]), "kept").expect("credentials");
    assert!(retire(&home).is_err());
    assert_eq!(
        std::fs::read_to_string(home.root().join(RECEIPTS)).expect("retained"),
        "not a receipt map"
    );
    assert_eq!(
        std::fs::read_to_string(home.root().join(RETIRED_FILES[0])).expect("retained"),
        "kept"
    );
    assert_eq!(
        ledger::read_all(&store.ledger_path(&source.group_id).expect("ledger")).expect("events"),
        [source]
    );
}

#[test]
fn deleted_group_is_not_recreated_and_inbound_receipts_do_not_create_outbound_history() {
    let (_temp, home, store, source, record) = fixture();
    assert!(store.delete(&source.group_id).expect("delete Group"));
    let inbound = json!({"registration_id":"other","idempotency_key":"received","status":"sent","event_id":"existing-target-event"});
    write_records(&home, &[record, inbound]);
    retire(&home).expect("retire");
    assert!(
        !store
            .group_dir(&source.group_id)
            .expect("directory")
            .exists()
    );
    assert!(store.list().expect("registry").is_empty());
}
