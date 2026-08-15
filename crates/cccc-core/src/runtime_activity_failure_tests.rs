use super::{read_events, record_hook_event};
use crate::{HomeLayout, codex_hook_state};
use serde_json::{Value, json};

fn record(home: &HomeLayout, payload: &Value) {
    codex_hook_state::record_runtime_with_observer(
        home,
        "claude",
        "g_test",
        "peer",
        "token",
        payload,
        |state, authorized| {
            assert!(authorized);
            record_hook_event(home, "claude", "token", payload, state)?;
            Ok(())
        },
    )
    .expect("record hook");
}

#[test]
fn claude_tool_failure_closes_started_activity_with_duration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    home.initialize().expect("initialize");
    codex_hook_state::begin_launch(&home, "claude", "g_test", "peer", "token", "HookPending")
        .expect("launch");
    for payload in [
        json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
        json!({
            "hook_event_name":"PreToolUse",
            "session_id":"session-1",
            "tool_use_id":"op-1",
            "tool_name":"Bash"
        }),
        json!({
            "hook_event_name":"PostToolUseFailure",
            "session_id":"session-1",
            "tool_use_id":"op-1"
        }),
    ] {
        record(&home, &payload);
    }

    let events = read_events(&home, "g_test").expect("events");
    let tool = events
        .iter()
        .find(|event| event.kind == "tool")
        .expect("tool activity");
    assert_eq!(tool.status, "failed");
    assert_eq!(tool.event_type, "PostToolUseFailure");
    assert_eq!(tool.tool_name.as_deref(), Some("Bash"));
    assert!(tool.duration_ms.is_some());
}
