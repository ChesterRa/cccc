use super::*;
use cccc_contracts::{ActorRuntime, Event};
use cccc_core::GroupStore;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(unix)]
#[test]
fn stalled_turn_reaches_deadline_and_confirms_cancellation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek timeout", "").expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
elif printf '%s' "$line" | grep -q '"method":"session/cancel"'; then
  printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "${rid:-3}"
else
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","updateOrdinal":0,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"partial reply"}}}}'
fi
done"#;
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), script.into()];
    group.actors.push(actor.clone());
    store.save(&group).expect("save");
    start(&home, &group, &actor, temp.path()).expect("start");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = serde_json::json!({"to":["deepseek"],"text":"hello"})
        .as_object()
        .cloned()
        .expect("event data");

    let started = Instant::now();
    assert!(!delivery::deliver_with_timeout(
        &home,
        &group,
        &actor,
        &event,
        &AtomicBool::new(false),
        Duration::from_millis(500),
    ));

    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(running(&group.group_id, &actor.id));
    let events = std::fs::read_to_string(
        store
            .state_dir(&group.group_id)
            .expect("state")
            .join("headless/events.jsonl"),
    )
    .expect("headless events");
    let events = events
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("event json"))
        .collect::<Vec<_>>();
    let started_index = events
        .iter()
        .position(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("headless.turn.started")
        })
        .expect("started event");
    let failed_index = events
        .iter()
        .position(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("headless.turn.failed")
        })
        .expect("failed event");
    let completed_message_index = events
        .iter()
        .position(|event| {
            event.get("type").and_then(serde_json::Value::as_str)
                == Some("headless.message.completed")
        })
        .expect("completed partial message");
    assert!(started_index < completed_message_index);
    assert!(completed_message_index < failed_index);
    assert_eq!(
        events[completed_message_index]
            .pointer("/data/text")
            .and_then(serde_json::Value::as_str),
        Some("partial reply")
    );
    assert_eq!(
        events[failed_index]
            .pointer("/data/error/code")
            .and_then(serde_json::Value::as_str),
        Some("timeout")
    );
    stop(&group.group_id, &actor.id);
}

#[cfg(unix)]
#[test]
fn cancellation_during_frame_poll_is_not_persisted_as_timeout() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek cancelled", "").expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
elif printf '%s' "$line" | grep -q '"method":"session/cancel"'; then
  printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "${rid:-3}"
else
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
fi
done"#;
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), script.into()];
    group.actors.push(actor.clone());
    store.save(&group).expect("save");
    start(&home, &group, &actor, temp.path()).expect("start");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = serde_json::json!({"to":["deepseek"],"text":"hello"})
        .as_object()
        .cloned()
        .expect("event data");
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_signal = Arc::clone(&cancelled);
    let cancel_thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        cancel_signal.store(true, Ordering::Release);
    });

    assert!(!delivery::deliver_with_timeout(
        &home,
        &group,
        &actor,
        &event,
        cancelled.as_ref(),
        Duration::from_secs(5),
    ));
    cancel_thread.join().expect("cancel thread");

    let events = std::fs::read_to_string(
        store
            .state_dir(&group.group_id)
            .expect("state")
            .join("headless/events.jsonl"),
    )
    .expect("headless events");
    assert!(events.contains("headless.turn.started"));
    assert!(!events.contains("headless.turn.completed"));
    assert!(!events.contains("\"code\":\"timeout\""));
    stop(&group.group_id, &actor.id);
}

#[cfg(unix)]
#[test]
fn end_turn_arriving_during_timeout_settlement_remains_timed_out() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek late success", "").expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
elif printf '%s' "$line" | grep -q '"method":"session/cancel"'; then
  printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "${rid:-3}"
else
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
fi
done"#;
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), script.into()];
    group.actors.push(actor.clone());
    store.save(&group).expect("save");
    start(&home, &group, &actor, temp.path()).expect("start");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = serde_json::json!({"to":["deepseek"],"text":"hello"})
        .as_object()
        .cloned()
        .expect("event data");

    assert!(!delivery::deliver_with_timeout(
        &home,
        &group,
        &actor,
        &event,
        &AtomicBool::new(false),
        Duration::from_millis(100),
    ));

    let events = std::fs::read_to_string(
        store
            .state_dir(&group.group_id)
            .expect("state")
            .join("headless/events.jsonl"),
    )
    .expect("headless events");
    assert!(!events.contains("headless.turn.completed"));
    assert!(events.contains("headless.turn.failed"));
    assert!(events.contains("\"code\":\"timeout\""));
    stop(&group.group_id, &actor.id);
}
