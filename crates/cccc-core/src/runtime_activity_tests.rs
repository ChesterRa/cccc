use super::store::terminalize_active_activities;
use super::{
    RuntimeActivityEvent, enforce_event_limit, project_hook_event, record_hook_event,
    sanitize_label,
};
use crate::codex_hook_state::CodexHookState;
use crate::{HomeLayout, codex_hook_state};
use serde_json::json;

fn state(runtime: &str, event: &str) -> CodexHookState {
    CodexHookState {
        v: 3,
        runtime: runtime.into(),
        group_id: "g1".into(),
        actor_id: "peer".into(),
        status: "working".into(),
        event: event.into(),
        session_id: "session-1".into(),
        turn_id: Some("turn-1".into()),
        operation_id: Some("op-1".into()),
        interrupted: false,
        awaiting_session_start: false,
        turn_generation: 1,
        launch_token: "token".into(),
        seen_turn_ids: vec!["turn-1".into()],
        seen_operation_ids: vec!["op-1".into()],
        turn_fence_exhausted: false,
        operation_fence_exhausted: false,
        diagnostic: None,
        session_closed: false,
        observation: "full_fidelity".into(),
        updated_at: "2026-07-28T00:00:00Z".into(),
    }
}

#[test]
fn codex_tool_activity_requires_exact_fenced_turn() {
    let payload = json!({
        "hook_event_name":"PreToolUse",
        "session_id":"session-1",
        "turn_id":"turn-1",
        "tool_use_id":"op-1",
        "tool_name":"mcp__github__search"
    });
    let draft = project_hook_event("codex", "token", &payload, &state("codex", "PreToolUse"))
        .expect("activity");
    assert_eq!(draft.kind, "tool");
    assert_eq!(draft.status, "started");
    assert_eq!(draft.tool_name.as_deref(), Some("mcp__github__search"));

    let stale = json!({"hook_event_name":"PreToolUse","session_id":"session-1","turn_id":"old","tool_use_id":"op-1"});
    assert!(project_hook_event("codex", "token", &stale, &state("codex", "PreToolUse")).is_none());

    let wrong_operation = json!({
        "hook_event_name":"PreToolUse",
        "session_id":"session-1",
        "turn_id":"turn-1",
        "tool_use_id":"op-2"
    });
    assert!(
        project_hook_event(
            "codex",
            "token",
            &wrong_operation,
            &state("codex", "PreToolUse")
        )
        .is_none()
    );

    let permission_without_turn = json!({
        "hook_event_name":"PermissionRequest",
        "session_id":"session-1",
        "tool_use_id":"op-1"
    });
    assert!(
        project_hook_event(
            "codex",
            "token",
            &permission_without_turn,
            &state("codex", "PermissionRequest")
        )
        .is_some()
    );

    let completion_without_turn = json!({
        "hook_event_name":"PostToolUse",
        "session_id":"session-1",
        "tool_use_id":"op-1"
    });
    assert!(
        project_hook_event(
            "codex",
            "token",
            &completion_without_turn,
            &state("codex", "PostToolUse")
        )
        .is_some()
    );
}

#[test]
fn claude_observation_accepts_session_fenced_events_without_claiming_turn_precision() {
    let payload = json!({
        "hook_event_name":"PermissionRequest",
        "session_id":"session-1",
        "tool_use_id":"op-1",
        "tool_name":"Bash"
    });
    let draft = project_hook_event(
        "claude",
        "token",
        &payload,
        &state("claude", "SessionStart"),
    )
    .expect("activity");
    assert_eq!(draft.status, "waiting");
    assert_eq!(draft.operation_id.as_deref(), Some("op-1"));

    let turn = json!({
        "hook_event_name":"UserPromptSubmit",
        "session_id":"session-1",
        "prompt_id":"provider-prompt"
    });
    assert!(
        project_hook_event("claude", "token", &turn, &state("claude", "SessionStart")).is_none()
    );
}

#[test]
fn tool_labels_drop_free_form_content_and_apply_a_hard_bound() {
    assert_eq!(
        sanitize_label("Bash $(secret) /tmp/file".into()).as_deref(),
        Some("Bashsecret/tmp/file")
    );
    assert_eq!(
        sanitize_label("x".repeat(100)).map(|value| value.len()),
        Some(64)
    );
}

#[test]
fn serialized_event_contains_only_structured_safe_fields() {
    let event = RuntimeActivityEvent {
        v: 1,
        id: "event".into(),
        ts: "2026-07-28T00:00:00Z".into(),
        group_id: "g1".into(),
        actor_id: "peer".into(),
        runtime: "codex".into(),
        activity_id: "activity".into(),
        kind: "tool".into(),
        status: "started".into(),
        event_type: "PreToolUse".into(),
        session_id: "session-1".into(),
        turn_id: Some("turn-1".into()),
        operation_id: Some("op-1".into()),
        tool_name: Some("Bash".into()),
        duration_ms: None,
    };
    let encoded = serde_json::to_value(event).expect("json");
    assert!(encoded.get("command").is_none());
    assert!(encoded.get("tool_input").is_none());
}

fn record(home: &HomeLayout, runtime: &str, payload: &serde_json::Value) -> CodexHookState {
    codex_hook_state::record_runtime_with_observer(
        home,
        runtime,
        "g_test",
        "peer",
        "token",
        payload,
        |state, authorized| {
            if authorized {
                record_hook_event(home, runtime, "token", payload, state)?;
            }
            Ok(())
        },
    )
    .expect("record hook")
}

#[test]
fn stop_failure_marks_the_turn_failed_while_the_actor_returns_idle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    codex_hook_state::begin_launch(&home, "codex", "g_test", "peer", "token", "HookPending")
        .expect("launch");
    record(
        &home,
        "codex",
        &json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
    );
    record(
        &home,
        "codex",
        &json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session-1",
            "turn_id":"turn-1"
        }),
    );
    let state = record(
        &home,
        "codex",
        &json!({
            "hook_event_name":"StopFailure",
            "session_id":"session-1",
            "turn_id":"turn-1"
        }),
    );

    assert_eq!(state.status, "idle");
    assert_eq!(state.event, "StopFailure");
    let failed = super::read_events(&home, "g_test")
        .expect("events")
        .into_iter()
        .find(|event| event.event_type == "StopFailure")
        .expect("failed turn activity");
    assert_eq!(failed.kind, "turn");
    assert_eq!(failed.status, "failed");
}

#[test]
fn session_end_terminalizes_active_children() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    codex_hook_state::begin_launch(&home, "codex", "g_test", "peer", "token", "HookPending")
        .expect("launch");
    record(
        &home,
        "codex",
        &json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
    );
    record(
        &home,
        "codex",
        &json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session-1",
            "turn_id":"turn-1"
        }),
    );
    record(
        &home,
        "codex",
        &json!({
            "hook_event_name":"PreToolUse",
            "session_id":"session-1",
            "turn_id":"turn-1",
            "tool_use_id":"op-1",
            "tool_name":"Bash"
        }),
    );
    record(
        &home,
        "codex",
        &json!({"hook_event_name":"SessionEnd","session_id":"session-1"}),
    );

    let events = super::read_events(&home, "g_test").expect("events");
    assert!(
        events
            .iter()
            .all(|event| !matches!(event.status.as_str(), "started" | "waiting"))
    );
    assert!(events.iter().any(|event| {
        event.kind == "tool" && event.status == "failed" && event.event_type == "SessionEnded"
    }));
}

#[test]
fn interrupt_terminalizes_the_active_tool() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    codex_hook_state::begin_launch(&home, "codex", "g_test", "peer", "token", "HookPending")
        .expect("launch");
    record(
        &home,
        "codex",
        &json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
    );
    record(
        &home,
        "codex",
        &json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session-1",
            "turn_id":"turn-1"
        }),
    );
    record(
        &home,
        "codex",
        &json!({
            "hook_event_name":"PreToolUse",
            "session_id":"session-1",
            "turn_id":"turn-1",
            "tool_use_id":"op-1",
            "tool_name":"Bash"
        }),
    );

    codex_hook_state::record_interrupt(&home, "codex", "g_test", "peer").expect("interrupt");

    let events = super::read_events(&home, "g_test").expect("events");
    assert!(events.iter().any(|event| {
        event.kind == "tool" && event.status == "failed" && event.event_type == "UserInterrupt"
    }));
}

#[test]
fn unreadable_activity_file_rolls_back_interrupt_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    codex_hook_state::begin_launch(&home, "codex", "g_test", "peer", "token", "HookPending")
        .expect("launch");
    for payload in [
        json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
        json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session-1",
            "turn_id":"turn-1"
        }),
        json!({
            "hook_event_name":"PreToolUse",
            "session_id":"session-1",
            "turn_id":"turn-1",
            "tool_use_id":"op-1",
            "tool_name":"Bash"
        }),
    ] {
        record(&home, "codex", &payload);
    }
    let before = codex_hook_state::read(&home, "g_test", "peer").expect("state");
    std::fs::write(
        super::events_path(&home, "g_test").expect("events path"),
        b"{broken",
    )
    .expect("corrupt events");

    assert!(super::read_events(&home, "g_test").is_err());
    assert!(codex_hook_state::record_interrupt(&home, "codex", "g_test", "peer").is_err());
    assert_eq!(
        codex_hook_state::read(&home, "g_test", "peer").expect("rolled back state"),
        before
    );
}

#[test]
fn a_new_codex_turn_terminalizes_the_previous_turn_tool() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    codex_hook_state::begin_launch(&home, "codex", "g_test", "peer", "token", "HookPending")
        .expect("launch");
    for payload in [
        json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
        json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session-1",
            "turn_id":"turn-1"
        }),
        json!({
            "hook_event_name":"PreToolUse",
            "session_id":"session-1",
            "turn_id":"turn-1",
            "tool_use_id":"op-1",
            "tool_name":"Bash"
        }),
        json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session-1",
            "turn_id":"turn-2"
        }),
    ] {
        record(&home, "codex", &payload);
    }

    let events = super::read_events(&home, "g_test").expect("events");
    let old_tool = events
        .iter()
        .find(|event| event.activity_id.ends_with(":tool:op-1"))
        .expect("old tool");
    assert_eq!(old_tool.status, "failed");
    assert_eq!(old_tool.event_type, "TurnSuperseded");
}

#[test]
fn waiting_revision_keeps_the_tool_start_for_completion_duration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    codex_hook_state::begin_launch(&home, "codex", "g_test", "peer", "token", "HookPending")
        .expect("launch");
    for payload in [
        json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
        json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session-1",
            "turn_id":"turn-1"
        }),
        json!({
            "hook_event_name":"PreToolUse",
            "session_id":"session-1",
            "turn_id":"turn-1",
            "tool_use_id":"op-1",
            "tool_name":"Bash"
        }),
        json!({
            "hook_event_name":"PermissionRequest",
            "session_id":"session-1",
            "turn_id":"turn-1",
            "tool_use_id":"op-1"
        }),
        json!({
            "hook_event_name":"PostToolUse",
            "session_id":"session-1",
            "turn_id":"turn-1",
            "tool_use_id":"op-1"
        }),
    ] {
        record(&home, "codex", &payload);
    }

    let events = super::read_events(&home, "g_test").expect("events");
    let tool = events
        .iter()
        .find(|event| event.kind == "tool")
        .expect("tool");
    assert_eq!(tool.status, "completed");
    assert_eq!(tool.tool_name.as_deref(), Some("Bash"));
    assert!(tool.duration_ms.is_some());
}

#[test]
fn abnormal_terminalization_uses_the_original_tool_start() {
    let mut events = vec![
        RuntimeActivityEvent {
            v: 1,
            id: "started".into(),
            ts: "2026-07-28T00:00:00Z".into(),
            group_id: "g1".into(),
            actor_id: "peer".into(),
            runtime: "codex".into(),
            activity_id: "session-1:tool:op-1".into(),
            kind: "tool".into(),
            status: "started".into(),
            event_type: "PreToolUse".into(),
            session_id: "session-1".into(),
            turn_id: Some("turn-1".into()),
            operation_id: Some("op-1".into()),
            tool_name: Some("Bash".into()),
            duration_ms: None,
        },
        RuntimeActivityEvent {
            id: "waiting".into(),
            ts: "2026-07-28T00:00:05Z".into(),
            status: "waiting".into(),
            event_type: "PermissionRequest".into(),
            ..RuntimeActivityEvent {
                v: 1,
                id: String::new(),
                ts: String::new(),
                group_id: "g1".into(),
                actor_id: "peer".into(),
                runtime: "codex".into(),
                activity_id: "session-1:tool:op-1".into(),
                kind: "tool".into(),
                status: String::new(),
                event_type: String::new(),
                session_id: "session-1".into(),
                turn_id: Some("turn-1".into()),
                operation_id: Some("op-1".into()),
                tool_name: Some("Bash".into()),
                duration_ms: None,
            }
        },
    ];

    assert_eq!(
        terminalize_active_activities(
            &mut events,
            &state("codex", "PermissionRequest"),
            "UserInterrupt",
            "failed",
            "2026-07-28T00:00:10Z",
        ),
        1
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].duration_ms, Some(10_000));
}

#[test]
fn hook_activity_observer_stays_inside_the_state_critical_section() {
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    let temp = tempfile::tempdir().expect("tempdir");
    let home = Arc::new(HomeLayout::from_path(temp.path()).expect("home"));
    home.initialize().expect("initialize");
    codex_hook_state::begin_launch(&home, "codex", "g_test", "peer", "token", "HookPending")
        .expect("launch");
    record(
        &home,
        "codex",
        &json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
    );

    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let start_home = Arc::clone(&home);
    let start = std::thread::spawn(move || {
        let payload = json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session-1",
            "turn_id":"turn-1"
        });
        codex_hook_state::record_runtime_with_observer(
            &start_home,
            "codex",
            "g_test",
            "peer",
            "token",
            &payload,
            |state, authorized| {
                entered_tx.send(()).expect("entered");
                release_rx.recv().expect("release");
                assert!(authorized);
                record_hook_event(&start_home, "codex", "token", &payload, state)?;
                Ok(())
            },
        )
        .expect("start");
    });
    entered_rx.recv().expect("start observer entered");

    let (ended_tx, ended_rx) = mpsc::channel();
    let end_home = Arc::clone(&home);
    let end = std::thread::spawn(move || {
        let payload = json!({"hook_event_name":"SessionEnd","session_id":"session-1"});
        codex_hook_state::record_runtime_with_observer(
            &end_home,
            "codex",
            "g_test",
            "peer",
            "token",
            &payload,
            |state, authorized| {
                assert!(authorized);
                record_hook_event(&end_home, "codex", "token", &payload, state)?;
                Ok(())
            },
        )
        .expect("end");
        ended_tx.send(()).expect("ended");
    });

    assert!(ended_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).expect("release start");
    start.join().expect("start thread");
    end.join().expect("end thread");
    ended_rx.recv().expect("session ended");

    let events = super::read_events(&home, "g_test").expect("events");
    assert!(
        events
            .iter()
            .all(|event| !matches!(event.status.as_str(), "started" | "waiting"))
    );
}

#[test]
fn capacity_prefers_active_events_over_terminal_history() {
    let mut events = (0..=super::EVENT_LIMIT)
        .map(|index| RuntimeActivityEvent {
            id: format!("done-{index}"),
            ts: format!("2026-07-28T00:00:{:02}Z", index % 60),
            status: "completed".into(),
            activity_id: format!("done-{index}"),
            ..RuntimeActivityEvent {
                v: 1,
                id: String::new(),
                ts: String::new(),
                group_id: "g1".into(),
                actor_id: "peer".into(),
                runtime: "codex".into(),
                activity_id: String::new(),
                kind: "tool".into(),
                status: String::new(),
                event_type: "PostToolUse".into(),
                session_id: "session-1".into(),
                turn_id: Some("turn-1".into()),
                operation_id: None,
                tool_name: Some("Bash".into()),
                duration_ms: None,
            }
        })
        .collect::<Vec<_>>();
    events.insert(
        0,
        RuntimeActivityEvent {
            id: "active".into(),
            activity_id: "active".into(),
            status: "started".into(),
            ..events[0].clone()
        },
    );

    enforce_event_limit(&mut events).expect("terminal history can be evicted");

    assert_eq!(events.len(), super::EVENT_LIMIT);
    assert!(events.iter().any(|event| event.id == "active"));
}

#[test]
fn capacity_rejects_an_all_active_overflow() {
    let mut events = (0..=super::EVENT_LIMIT)
        .map(|index| RuntimeActivityEvent {
            v: 1,
            id: format!("active-{index}"),
            ts: "2026-07-28T00:00:00Z".into(),
            group_id: "g1".into(),
            actor_id: format!("peer-{index}"),
            runtime: "codex".into(),
            activity_id: format!("active-{index}"),
            kind: "tool".into(),
            status: "started".into(),
            event_type: "PreToolUse".into(),
            session_id: "session-1".into(),
            turn_id: Some("turn-1".into()),
            operation_id: Some(format!("op-{index}")),
            tool_name: Some("Bash".into()),
            duration_ms: None,
        })
        .collect::<Vec<_>>();

    assert!(enforce_event_limit(&mut events).is_err());
    assert_eq!(events.len(), super::EVENT_LIMIT + 1);
}

#[test]
fn capacity_can_drop_waiting_without_losing_the_started_predecessor() {
    let started = RuntimeActivityEvent {
        v: 1,
        id: "started".into(),
        ts: "2026-07-28T00:00:00Z".into(),
        group_id: "g1".into(),
        actor_id: "peer".into(),
        runtime: "codex".into(),
        activity_id: "tool:1".into(),
        kind: "tool".into(),
        status: "started".into(),
        event_type: "PreToolUse".into(),
        session_id: "session-1".into(),
        turn_id: Some("turn-1".into()),
        operation_id: Some("op-1".into()),
        tool_name: Some("Bash".into()),
        duration_ms: None,
    };
    let mut events = (0..super::EVENT_LIMIT)
        .map(|index| RuntimeActivityEvent {
            id: format!("started-{index}"),
            actor_id: format!("peer-{index}"),
            activity_id: format!("tool:{index}"),
            operation_id: Some(format!("op-{index}")),
            ..started.clone()
        })
        .collect::<Vec<_>>();
    events.push(RuntimeActivityEvent {
        id: "waiting".into(),
        ts: "2026-07-28T00:00:01Z".into(),
        status: "waiting".into(),
        event_type: "PermissionRequest".into(),
        ..started
    });

    enforce_event_limit(&mut events).expect("waiting can be degraded");

    assert_eq!(events.len(), super::EVENT_LIMIT);
    assert!(events.iter().any(|event| event.id == "started-0"));
    assert!(!events.iter().any(|event| event.id == "waiting"));
}

#[test]
fn capacity_overflow_rolls_back_the_hook_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    codex_hook_state::begin_launch(&home, "codex", "g_test", "peer", "token", "HookPending")
        .expect("launch");
    record(
        &home,
        "codex",
        &json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
    );
    record(
        &home,
        "codex",
        &json!({
            "hook_event_name":"UserPromptSubmit",
            "session_id":"session-1",
            "turn_id":"turn-1"
        }),
    );
    let now = cccc_contracts::utc_now();
    let active = (0..super::EVENT_LIMIT)
        .map(|index| RuntimeActivityEvent {
            v: 1,
            id: format!("active-{index}"),
            ts: now.clone(),
            group_id: "g_test".into(),
            actor_id: format!("peer-{index}"),
            runtime: "codex".into(),
            activity_id: format!("active-{index}"),
            kind: "tool".into(),
            status: "started".into(),
            event_type: "PreToolUse".into(),
            session_id: "session-1".into(),
            turn_id: Some("turn-1".into()),
            operation_id: Some(format!("op-{index}")),
            tool_name: Some("Bash".into()),
            duration_ms: None,
        })
        .collect::<Vec<_>>();
    crate::fs::write_json(
        &super::events_path(&home, "g_test").expect("events path"),
        &active,
    )
    .expect("seed capacity");
    let before = codex_hook_state::read(&home, "g_test", "peer").expect("state");
    let payload = json!({
        "hook_event_name":"PreToolUse",
        "session_id":"session-1",
        "turn_id":"turn-1",
        "tool_use_id":"op-new",
        "tool_name":"Bash"
    });

    assert!(
        codex_hook_state::record_runtime_with_observer(
            &home,
            "codex",
            "g_test",
            "peer",
            "token",
            &payload,
            |state, authorized| {
                assert!(authorized);
                record_hook_event(&home, "codex", "token", &payload, state)?;
                Ok(())
            },
        )
        .is_err()
    );
    assert_eq!(
        codex_hook_state::read(&home, "g_test", "peer").expect("rolled back state"),
        before
    );
    assert_eq!(
        super::read_events(&home, "g_test").expect("events").len(),
        super::EVENT_LIMIT
    );
}
