use super::*;
use cccc_contracts::{ActorRuntime, Event};
use cccc_core::{GroupStore, ledger};
use serde_json::{Map, json};

#[test]
fn durable_terminal_recovery_does_not_skip_failed_prefix() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek recovery", "").expect("group");
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let mut first = Event::new("chat.message", &group.group_id);
    first.by = "user".into();
    first.data = serde_json::json!({"to":["deepseek"],"text":"first"})
        .as_object()
        .cloned()
        .expect("event data");
    let mut second = Event::new("chat.message", &group.group_id);
    second.by = "user".into();
    second.data = serde_json::json!({"to":["deepseek"],"text":"second"})
        .as_object()
        .cloned()
        .expect("event data");
    let ledger_path = store.ledger_path(&group.group_id).expect("ledger");
    ledger::append(&ledger_path, &first).expect("first append");
    ledger::append(&ledger_path, &second).expect("second append");
    crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        &actor.id,
        "headless.turn.completed",
        Map::from_iter([("event_id".into(), json!(second.id))]),
        Some("deepseek.turn:second"),
    )
    .expect("terminal append");
    assert_eq!(recover(&home, &group, &actor, 256), 0);
    assert!(
        cccc_core::inbox::cursor(&home, &group.group_id, &actor.id)
            .expect("cursor")
            .is_none()
    );
}

#[test]
fn recovery_and_dedupe_are_bounded_with_persistent_marker() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("deepseek bounds", "").expect("group");
    let state = store.state_dir(&group.group_id).expect("state");
    let events = state.join("headless/events.jsonl");
    std::fs::create_dir_all(events.parent().expect("parent")).expect("headless");
    std::fs::write(&events, vec![b'x'; 4 * 1024 * 1024 + 1]).expect("oversized log");
    assert!(read_bounded_events(&events).is_none());
    std::fs::remove_file(&events).expect("remove oversized log");
    crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("event-1"))]),
        Some("deepseek.update:event-1:0"),
    )
    .expect("first append");
    let marker_dir = events.parent().expect("parent").join("events.dedupe");
    assert_eq!(std::fs::read_dir(marker_dir).expect("markers").count(), 2);
    crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        "deepseek",
        "headless.message.delta",
        Map::from_iter([("event_id".into(), json!("event-1"))]),
        Some("deepseek.update:event-1:0"),
    )
    .expect("marker dedupe");
    assert_eq!(
        std::fs::read_to_string(&events)
            .expect("events")
            .lines()
            .count(),
        1
    );
    for ordinal in 1..320 {
        crate::ops::local_headless::append_event_with_dedupe(
            &home,
            &group.group_id,
            "deepseek",
            "headless.message.delta",
            Map::from_iter([("payload".into(), json!("x".repeat(1024)))]),
            Some(&format!("deepseek.update:event-1:{ordinal}")),
        )
        .expect("ready index accepts growth");
    }
    assert!(
        std::fs::read_to_string(events)
            .expect("events")
            .lines()
            .count()
            > 256
    );
}
