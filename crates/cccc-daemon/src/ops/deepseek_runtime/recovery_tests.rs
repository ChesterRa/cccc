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
fn recovery_uses_the_persistent_marker_after_the_event_log_grows_large() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek bounds", "").expect("group");
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = json!({"to":["deepseek"],"text":"recover me"})
        .as_object()
        .cloned()
        .expect("event data");
    ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event)
        .expect("append message");
    crate::ops::local_headless::append_event_with_dedupe(
        &home,
        &group.group_id,
        &actor.id,
        "headless.turn.completed",
        Map::from_iter([("event_id".into(), json!(event.id))]),
        Some(&format!(
            "deepseek.turn:headless.turn.completed:{}",
            event.id
        )),
    )
    .expect("terminal append");
    let state = store.state_dir(&group.group_id).expect("state");
    let events = state.join("headless/events.jsonl");
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&events)
        .expect("open events");
    file.write_all(&vec![b'x'; 4 * 1024 * 1024 + 1])
        .expect("grow event log");
    file.write_all(b"\n").expect("finish padding");
    assert_eq!(recover(&home, &group, &actor, 256), 1);
    assert_eq!(
        cccc_core::inbox::cursor(&home, &group.group_id, &actor.id)
            .expect("cursor")
            .as_deref(),
        Some(event.id.as_str())
    );
}
