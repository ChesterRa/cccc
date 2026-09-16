use super::*;
use cccc_contracts::{ActorRuntime, Event};
use cccc_core::{GroupStore, ledger};
use std::sync::atomic::AtomicBool;

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
fn failed_dsh_assignment_hands_off_once_and_does_not_block_later_work() {
    use crate::ops::actor_delivery::{DeliveryJob, drain_group};
    use crate::ops::actor_delivery_worker::process_batch;
    use cccc_contracts::{ActorRole, RunnerKind};
    use cccc_core::context::ContextStore;
    use serde_json::json;

    let temp = tempfile::tempdir().expect("isolated DSH");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("DSH failure handoff", "").expect("group");
    let mut lead = Actor::new("web-lead");
    lead.runtime = ActorRuntime::WebModel;
    lead.runner = RunnerKind::Headless;
    lead.role = Some(ActorRole::Foreman);
    let mut actor = Actor::new("worker");
    actor.runtime = ActorRuntime::Deepseek;
    actor.runner = RunnerKind::Headless;
    let probe = temp.path().join("prompts");
    actor
        .env
        .insert("PROBE_PATH".into(), probe.to_string_lossy().into_owned());
    actor.command = vec!["sh".into(), "-c".into(), r#"
while IFS= read -r line; do
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}\n' "$rid" ;;
    *'"method":"session/new"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"fixture-%s"}}\n' "$rid" "$$" ;;
    *'BROKEN_TASK'*) printf 'broken\n' >> "$PROBE_PATH"; printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32603,"message":"SSE stream ended without [DONE]"}}\n' "$rid" ;;
    *) printf 'next\n' >> "$PROBE_PATH"; printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$rid" ;;
  esac
done
"#.into()];
    group.actors = vec![lead, actor.clone()];
    group.running = true;
    store.save(&group).expect("save group");
    let context = ContextStore::new(home.clone()).expect("context");
    let task = context
        .sync(
            &group.group_id,
            &[json!({
                "op":"task.create","title":"Broken assignment","assignee":"worker","status":"active"
            })
            .as_object()
            .cloned()
            .expect("task op")],
            None,
            "web-lead",
            false,
        )
        .expect("task")
        .context
        .tasks[0]["id"]
        .clone();
    let path = store.ledger_path(&group.group_id).expect("ledger");
    let jobs = ["BROKEN_TASK", "NEXT_TASK"].map(|text| {
        let mut event = Event::new("chat.message", &group.group_id);
        event.by = "web-lead".into();
        event.data = json!({"to":["worker"],"text":text,"message_mode":"send"})
            .as_object()
            .cloned()
            .expect("message");
        if text == "BROKEN_TASK" {
            event
                .data
                .insert("refs".into(), json!([{"kind":"task_ref","task_id":task}]));
        }
        ledger::append(&path, &event).expect("source");
        DeliveryJob {
            home: home.clone(),
            group: group.clone(),
            actor: actor.clone(),
            event,
        }
    });
    start(&home, &group, &actor, temp.path()).expect("start fake provider");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let cancelled = AtomicBool::new(false);
        for _ in 0..2 {
            assert!(!process_batch(&jobs, &mut String::new(), &cancelled));
        }
        stop(&group.group_id, &actor.id);
        start(&home, &group, &actor, temp.path()).expect("restart keeps attempt history");
        assert!(
            process_batch(&jobs, &mut String::new(), &cancelled),
            "failed original must hand off after three attempts and allow the next task"
        );
        assert_eq!(
            std::fs::read_to_string(&probe).expect("prompts"),
            "broken\nbroken\nbroken\nnext\n"
        );
        drain_group(&home, &group.group_id);
        let events = ledger::read_all(&path).expect("events");
        let failures = events
            .iter()
            .filter(|e| {
                e.kind == "chat.message"
                    && e.data.get("reply_to").and_then(serde_json::Value::as_str)
                        == Some(jobs[0].event.id.as_str())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            failures.len(),
            1,
            "one failure report for the original assignment"
        );
        assert_eq!(failures[0].data["to"], json!(["web-lead"]));
        assert!(
            failures[0].data["text"]
                .as_str()
                .expect("failure text")
                .contains("[DONE]")
        );
        assert!(events.iter().any(|e| {
            e.kind == "coordination.handoff"
                && e.data["turn_status"] == "failed"
                && e.data["source_event_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.contains(&json!(failures[0].id)))
        }));
        assert_eq!(
            context.load(&group.group_id).expect("context").tasks[0]["status"],
            "active",
            "transport acceptance is not task success"
        );
        stop(&group.group_id, &actor.id);
        start(&home, &group, &actor, temp.path()).expect("restart after handoff");
        assert!(process_batch(&jobs, &mut String::new(), &cancelled));
        assert_eq!(
            std::fs::read_to_string(&probe).expect("prompts"),
            "broken\nbroken\nbroken\nnext\n"
        );
        assert_eq!(
            ledger::read_all(&path)
                .expect("events")
                .iter()
                .filter(|e| e.kind == "chat.message"
                    && e.data.get("reply_to").and_then(serde_json::Value::as_str)
                        == Some(jobs[0].event.id.as_str()))
                .count(),
            1
        );

        let decision_request = cccc_contracts::DaemonRequest {
            v: 1,
            op: "coordination_decide".into(),
            args: json!({"group_id":group.group_id,"by":"web-lead","decision":"blocked",
                "reason":"DSH stream incomplete","event_ids":[failures[0].id]})
            .as_object()
            .cloned()
            .expect("decision request"),
        };
        crate::ops::coordination_relay::resolve_operation(&decision_request)
            .expect("native decision")
            .execute(&home, &decision_request)
            .expect("review failure");
        let task_after_review = context.load(&group.group_id).expect("context");
        assert_eq!(task_after_review.tasks[0]["status"], "active");
        assert_eq!(
            task_after_review.tasks[0]["waiting_on"], "external",
            "Foreman decision must resolve the failure against its original task"
        );

        let mut old = Event::new("chat.message", &group.group_id);
        old.by = "web-lead".into();
        old.data = json!({"to":["worker"],"text":"OLD_TASK","message_mode":"send"})
            .as_object()
            .cloned()
            .expect("fixture data");
        ledger::append(&path, &old).expect("old assignment");
        let mut report = Event::new("chat.message", &group.group_id);
        report.by = "worker".into();
        report.data = json!({"to":["web-lead"],"text":"Delivered","reply_to":old.id})
            .as_object()
            .cloned()
            .expect("fixture data");
        ledger::append(&path, &report).expect("existing report");
        let mut decision = Event::new("coordination.decision", &group.group_id);
        decision.by = "web-lead".into();
        decision.data =
            json!({"status":"applied","decision":"complete","source_event_ids":[report.id]})
                .as_object()
                .cloned()
                .expect("fixture data");
        ledger::append(&path, &decision).expect("recorded review");
        assert!(process_batch(
            &[DeliveryJob {
                event: old,
                ..jobs[0].clone()
            }],
            &mut String::new(),
            &cancelled
        ));
        assert_eq!(
            std::fs::read_to_string(&probe).expect("prompts"),
            "broken\nbroken\nbroken\nnext\n",
            "reviewed reports must not require another provider completion"
        );
    }));
    stop(&group.group_id, &actor.id);
    crate::ops::actor_delivery::shutdown_group(&group.group_id);
    outcome.expect("DSH handoff assertions");
}
