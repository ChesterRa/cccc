use super::working_state::fields;
use cccc_contracts::{Actor, ActorRuntime, RunnerKind, RuntimeStateSource};
use cccc_core::HomeLayout;
use serde_json::json;

fn bind(home: &HomeLayout, group_id: &str, runtime: &str, token: &str, pid: u32) {
    super::runtime_hook_session::bind_for_test(home, group_id, "peer1", runtime, token, pid);
}

#[test]
fn codex_state_comes_only_from_the_current_hook_process() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group_id = "g_codex_projection";
    let mut actor = Actor::new("peer1");
    actor.runtime = ActorRuntime::Codex;
    cccc_core::codex_hook_state::begin_launch(
        &home,
        "codex",
        group_id,
        "peer1",
        "token",
        "HookPending",
    )
    .expect("launch");
    for payload in [
        json!({"hook_event_name":"SessionStart","session_id":"s1"}),
        json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"turn-1"}),
    ] {
        cccc_core::codex_hook_state::record(&home, group_id, "peer1", "token", &payload)
            .expect("hook state");
    }
    bind(&home, group_id, "codex", "token", 42);

    let current = fields(&home, &actor, group_id, true, "pty", Some(42));
    assert_eq!(current["effective_working_state"], "working");
    assert_eq!(current["effective_active_task_id"], "turn-1");

    let stale = fields(&home, &actor, group_id, true, "pty", Some(43));
    assert_eq!(stale["effective_working_state"], "waiting");
    assert_eq!(stale["effective_working_reason"], "codex_hook_pending");
}

#[test]
fn app_server_codex_uses_the_bound_hook_process_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group_id = "g_app_server_codex_projection";
    let mut actor = Actor::new("peer1");
    actor.runtime = ActorRuntime::Codex;
    actor.runtime_state_source = RuntimeStateSource::AppServer;
    cccc_core::codex_hook_state::begin_launch(
        &home,
        "codex",
        group_id,
        "peer1",
        "token",
        "HookPending",
    )
    .expect("launch");
    for payload in [
        json!({"hook_event_name":"SessionStart","session_id":"s1"}),
        json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"turn-1"}),
    ] {
        cccc_core::codex_hook_state::record(&home, group_id, "peer1", "token", &payload)
            .expect("hook state");
    }
    bind(&home, group_id, "codex", "token", 42);

    let current = fields(&home, &actor, group_id, true, "pty", Some(42));
    assert_eq!(current["effective_working_state"], "working");
    assert_eq!(
        current["effective_working_reason"],
        "codex_hook_UserPromptSubmit"
    );
    assert_eq!(current["effective_active_task_id"], "turn-1");

    let stale = fields(&home, &actor, group_id, true, "pty", Some(43));
    assert_eq!(stale["effective_working_state"], "waiting");
    assert_eq!(stale["effective_working_reason"], "codex_hook_pending");
}

#[test]
fn claude_state_requires_matching_identity_and_capability() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group_id = "g_claude_projection";
    let mut actor = Actor::new("peer1");
    actor.runtime = ActorRuntime::Claude;
    cccc_core::codex_hook_state::begin_launch(
        &home,
        "claude",
        group_id,
        "peer1",
        "token",
        "HookPending",
    )
    .expect("launch");
    cccc_core::codex_hook_state::record_runtime(
        &home,
        "claude",
        group_id,
        "peer1",
        "token",
        &json!({"hook_event_name":"SessionStart","session_id":"session-1"}),
    )
    .expect("hook state");
    bind(&home, group_id, "claude", "token", 42);

    let state = fields(&home, &actor, group_id, true, "pty", Some(42));
    assert_eq!(state["effective_working_state"], "idle");
    assert_eq!(
        state["effective_working_reason"],
        "claude_pty_fail_closed_SessionStart"
    );
}

#[test]
fn missing_capability_fails_closed_and_headless_remains_idle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let mut claude = Actor::new("peer1");
    claude.runtime = ActorRuntime::Claude;
    let state = fields(&home, &claude, "g_test", true, "pty", Some(42));
    assert_eq!(state["effective_working_state"], "waiting");
    assert_eq!(state["effective_working_reason"], "claude_hook_pending");

    let mut custom = Actor::new("peer1");
    custom.runtime = ActorRuntime::Custom;
    custom.runner = RunnerKind::Headless;
    let state = fields(&home, &custom, "g_test", true, "headless", None);
    assert_eq!(state["effective_working_state"], "idle");
    assert_eq!(state["effective_working_reason"], "headless_running");
}
