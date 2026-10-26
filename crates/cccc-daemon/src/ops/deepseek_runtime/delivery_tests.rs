use super::*;
use cccc_contracts::{ActorRuntime, Event};
use cccc_core::{GroupStore, ledger};
use std::sync::atomic::AtomicBool;

/// These tests ask only whether the turn completed.
fn deliver(
    home: &HomeLayout,
    group: &GroupDoc,
    actor: &Actor,
    event: &Event,
    cancelled: &AtomicBool,
) -> bool {
    super::deliver(home, group, actor, event, cancelled) == TurnDelivery::Completed
}

#[cfg(unix)]
#[test]
fn fake_acp_delivery_persists_update_and_terminal_before_success() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek delivery", "").expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
else
  printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","updateOrdinal":0,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"}}}}'
  printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","updateOrdinal":0,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"}}}}'
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "${rid:-3}"
fi
done"#;
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), script.into()];
    group.actors.push(actor.clone());
    store.save(&group).expect("save group");
    start(&home, &group, &actor, temp.path()).expect("start");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = serde_json::json!({"to":["deepseek"],"text":"hello"})
        .as_object()
        .cloned()
        .expect("event data");
    ledger::append(&store.ledger_path(&group.group_id).expect("ledger"), &event)
        .expect("append event");
    let cancelled = AtomicBool::new(false);
    assert!(deliver(&home, &group, &actor, &event, &cancelled));
    assert!(deliver(&home, &group, &actor, &event, &cancelled));
    let path = store
        .state_dir(&group.group_id)
        .expect("state")
        .join("headless/events.jsonl");
    let text = std::fs::read_to_string(path).expect("headless events");
    assert_eq!(text.matches("headless.message.delta").count(), 2);
    assert!(text.contains("headless.message.completed"));
    assert!(text.contains("headless.turn.completed"));
    stop(&group.group_id, &actor.id);
}

#[cfg(unix)]
#[test]
fn failed_attempt_output_does_not_hide_successful_retry() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek retry output", "").expect("group");
    let script = r#"attempt=0
while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
else
  attempt=$((attempt + 1))
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  if [ "$attempt" -eq 1 ]; then
    printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"partial"}}}}'
    printf '{"jsonrpc":"2.0","id":%s,"error":{"message":"temporary"}}\n' "$rid"
  else
    printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"complete"}}}}'
    printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$rid"
  fi
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
    let cancelled = AtomicBool::new(false);

    assert!(!deliver(&home, &group, &actor, &event, &cancelled));
    assert!(deliver(&home, &group, &actor, &event, &cancelled));

    let events = std::fs::read_to_string(
        store
            .state_dir(&group.group_id)
            .expect("state")
            .join("headless/events.jsonl"),
    )
    .expect("headless events")
    .lines()
    .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("event json"))
    .collect::<Vec<_>>();
    let deltas = events
        .iter()
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str) == Some("headless.message.delta")
        })
        .filter_map(|event| {
            event
                .pointer("/data/delta")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<Vec<_>>();
    let completed = events
        .iter()
        .filter(|event| {
            event.get("type").and_then(serde_json::Value::as_str)
                == Some("headless.message.completed")
        })
        .filter_map(|event| {
            event
                .pointer("/data/text")
                .and_then(serde_json::Value::as_str)
        })
        .collect::<Vec<_>>();
    assert_eq!(deltas, ["partial", "complete"]);
    assert_eq!(completed, ["partial", "complete"]);
    stop(&group.group_id, &actor.id);
}

#[cfg(unix)]
#[test]
fn missing_credential_is_structured_secret_free_and_stops_runtime() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek credential", "").expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
else
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  printf '{"jsonrpc":"2.0","id":%s,"error":{"message":"no API key for DEEPSEEK_API_KEY; diagnostic=should-not-leak"}}\n' "${rid:-3}"
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

    assert!(!deliver(
        &home,
        &group,
        &actor,
        &event,
        &AtomicBool::new(false),
    ));
    assert!(!running(&group.group_id, &actor.id));
    assert!(manual_restart_required(&home, &group, &actor));
    let events = std::fs::read_to_string(
        store
            .state_dir(&group.group_id)
            .expect("state")
            .join("headless/events.jsonl"),
    )
    .expect("headless events");
    assert!(events.contains("credential_unavailable"));
    assert!(events.contains("environment"));
    assert!(events.contains("DeepSeek API credential is not configured"));
    assert!(!events.contains("should-not-leak"));
    stop(&group.group_id, &actor.id);
}

#[cfg(unix)]
#[test]
fn context_overflow_is_structured_and_requires_manual_restart() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store
        .create("deepseek context overflow", "")
        .expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
else
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32603,"message":"This model request failed","data":"maximum context length is 1048576 tokens; diagnostic=should-not-leak"}}\n' "${rid:-3}"
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

    assert!(!deliver(
        &home,
        &group,
        &actor,
        &event,
        &AtomicBool::new(false),
    ));
    assert!(!running(&group.group_id, &actor.id));
    assert!(manual_restart_required(&home, &group, &actor));
    let events = std::fs::read_to_string(
        store
            .state_dir(&group.group_id)
            .expect("state")
            .join("headless/events.jsonl"),
    )
    .expect("headless events");
    assert!(events.contains("context_window_exceeded"));
    assert!(events.contains("context"));
    assert!(events.contains("restart the actor to create a fresh session"));
    assert!(!events.contains("should-not-leak"));
    stop(&group.group_id, &actor.id);
    assert!(manual_restart_required(&home, &group, &actor));
    start(&home, &group, &actor, temp.path()).expect("explicit restart");
    assert!(!manual_restart_required(&home, &group, &actor));
    stop(&group.group_id, &actor.id);
}

#[cfg(unix)]
#[test]
fn fake_acp_output_append_failure_does_not_report_delivery_success() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek append failure", "").expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
elif printf '%s' "$line" | grep -q '"method":"session/cancel"'; then
  :
else
  printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"}}}}'
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "${rid:-3}"
fi
done"#;
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), script.into()];
    group.actors.push(actor.clone());
    store.save(&group).expect("save");
    start(&home, &group, &actor, temp.path()).expect("start");
    let state = store.state_dir(&group.group_id).expect("state");
    std::fs::create_dir_all(&state).expect("state dir");
    std::fs::write(state.join("headless"), b"not a directory").expect("failure fixture");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = serde_json::json!({"to":["deepseek"],"text":"hello"})
        .as_object()
        .cloned()
        .expect("event data");
    let cancelled = AtomicBool::new(false);
    assert!(!deliver(&home, &group, &actor, &event, &cancelled));
    assert!(running(&group.group_id, &actor.id));
    std::fs::remove_file(state.join("headless")).expect("remove failure fixture");
    assert!(deliver(&home, &group, &actor, &event, &cancelled));
    stop(&group.group_id, &actor.id);
}

#[cfg(unix)]
#[test]
fn unconfirmed_cancel_stops_supervisor_after_durable_write_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("deepseek cancel failure", "").expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
fi
done"#;
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), script.into()];
    group.actors.push(actor.clone());
    store.save(&group).expect("save");
    start(&home, &group, &actor, temp.path()).expect("start");
    let state = store.state_dir(&group.group_id).expect("state");
    std::fs::create_dir_all(&state).expect("state dir");
    std::fs::write(state.join("headless"), b"not a directory").expect("failure fixture");
    let mut event = Event::new("chat.message", &group.group_id);
    event.by = "user".into();
    event.data = serde_json::json!({"to":["deepseek"],"text":"hello"})
        .as_object()
        .cloned()
        .expect("event data");
    assert!(!deliver(
        &home,
        &group,
        &actor,
        &event,
        &AtomicBool::new(false),
    ));
    assert!(!running(&group.group_id, &actor.id));
    stop(&group.group_id, &actor.id);
}

#[cfg(unix)]
#[test]
fn cancelled_terminal_is_not_completed_or_delivered() {
    assert_eq!(
        cccc_runtime::deepseek_acp::terminal_stop_reason(
            &serde_json::json!({"result":{"stopReason":"cancelled"}})
        ),
        Some("cancelled")
    );
    assert_ne!(
        cccc_runtime::deepseek_acp::terminal_stop_reason(
            &serde_json::json!({"result":{"stopReason":"cancelled"}})
        ),
        Some("end_turn")
    );
}

#[cfg(unix)]
#[test]
fn running_query_does_not_wait_for_the_supervisor_turn_lock() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store
        .create("deepseek nonblocking status", "")
        .expect("group");
    let script = r#"while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
else
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
fi
done"#;
    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), script.into()];
    group.actors.push(actor.clone());
    store.save(&group).expect("save");
    start(&home, &group, &actor, temp.path()).expect("start");
    let key = (group.group_id.clone(), actor.id.clone());
    let holder = sessions()
        .read()
        .expect("sessions")
        .get(&key)
        .cloned()
        .expect("holder");
    let guard = holder.supervisor.lock().expect("turn lock");
    let started = std::time::Instant::now();
    assert!(running(&group.group_id, &actor.id));
    assert!(started.elapsed() < std::time::Duration::from_millis(50));
    drop(guard);
    stop(&group.group_id, &actor.id);
}

#[cfg(unix)]
#[test]
fn launch_workspace_projection_tracks_the_deepseek_owner_not_saved_settings() {
    use cccc_contracts::DaemonRequest;
    use serde_json::json;
    let temp = tempfile::tempdir().expect("isolated DeepSeek fixture");
    let home = HomeLayout::from_path(temp.path().join("cccc")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store
        .create("DeepSeek launch workspace", "")
        .expect("group");
    let roots = [temp.path().join("A"), temp.path().join("B")];
    for (root, key) in roots.iter().zip(["scope_a", "scope_b"]) {
        std::fs::create_dir(root).expect("scope root");
        std::fs::write(root.join("same.txt"), key).expect("scope bytes");
        group.scopes.push(cccc_core::Scope {
            scope_key: key.into(),
            url: root.to_string_lossy().into_owned(),
            label: key.into(),
            git_remote: String::new(),
        });
    }
    group.active_scope_key = "scope_a".into();
    let mut actor = Actor::new("deepseek-scope");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["sh".into(), "-c".into(), r#"pwd > physical-cwd.txt
while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"isolated-fixture"}}}'
else
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fixture-scope"}}'
fi
done"#.into()];
    actor.env = std::collections::BTreeMap::from([
        (
            "HOME".into(),
            temp.path().join("provider").to_string_lossy().into_owned(),
        ),
        (
            "CODEX_HOME".into(),
            temp.path()
                .join("provider/codex")
                .to_string_lossy()
                .into_owned(),
        ),
        (
            "CLAUDE_CONFIG_DIR".into(),
            temp.path()
                .join("provider/claude")
                .to_string_lossy()
                .into_owned(),
        ),
    ]);
    group.actors.push(actor.clone());
    store.save(&group).expect("save fixture configuration");
    struct Cleanup(String, String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            stop(&self.0, &self.1);
        }
    }
    let _cleanup = Cleanup(group.group_id.clone(), actor.id.clone());
    start(&home, &group, &actor, &roots[0]).expect("fixture protocol launch");
    actor.default_scope_key = "scope_b".into();
    actor.runtime = ActorRuntime::Custom;
    group.actors[0] = actor.clone();
    store.save(&group).expect("save without restart");
    let show = || {
        crate::handle_request(
            &home,
            &DaemonRequest {
                v: 1,
                op: "group_show".into(),
                args: json!({"group_id":group.group_id,"by":actor.id,"actor_id":"other"})
                    .as_object()
                    .expect("args")
                    .clone(),
            },
        )
    };
    let projection = show();
    assert!(projection.ok, "{:?}", projection.error);
    assert_eq!(
        projection.result["effective_scope"],
        json!({"path":roots[0],"scope_key":"scope_a","source":"runtime_launch"})
    );
    assert_eq!(
        std::fs::read_to_string(roots[0].join("physical-cwd.txt"))
            .expect("physical launch cwd")
            .trim(),
        roots[0].to_string_lossy()
    );
    let prepared =
        crate::ops::messaging::files::read(&group, &actor.id, &[json!("same.txt")], 1024)
            .expect("owner source admission");
    let mut data = serde_json::Map::new();
    prepared
        .apply(&home, &group, &mut data)
        .expect("actual blob placement");
    let blob = cccc_core::blobs::resolve(
        &home,
        &group.group_id,
        data["attachments"][0]["path"].as_str().expect("blob path"),
    )
    .expect("blob");
    assert_eq!(
        std::fs::read_to_string(blob).expect("blob bytes"),
        "scope_a"
    );
    let prompt = crate::ops::actor_runtime::session_prompt(&home, &group, &actor);
    assert!(
        prompt.contains(&format!("workspace: {}\n", roots[0].display())),
        "{prompt}"
    );
    // A busy protocol lock must not make a read projection wait or switch roots.
    let holder = sessions()
        .read()
        .expect("sessions")
        .get(&(group.group_id.clone(), actor.id.clone()))
        .cloned()
        .expect("owner");
    let turn = holder.supervisor.lock().expect("active turn");
    assert_eq!(show().result["effective_scope"]["path"], json!(roots[0]));
    drop(turn);
    stop(&group.group_id, &actor.id);
    assert_eq!(
        show().result["effective_scope"],
        json!({"path":roots[1],"scope_key":"scope_b","source":"actor_default"})
    );
    start(&home, &group, &actor, &roots[1]).expect("next fixture launch");
    assert_eq!(show().result["effective_scope"]["path"], json!(roots[1]));
    assert_eq!(
        std::fs::read_to_string(roots[1].join("physical-cwd.txt"))
            .expect("next physical cwd")
            .trim(),
        roots[1].to_string_lossy()
    );
}
