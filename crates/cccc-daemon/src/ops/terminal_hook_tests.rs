use super::{is_interrupt_input, write};
use cccc_contracts::{DaemonRequest, RunnerKind};
use cccc_core::HomeLayout;
use cccc_runtime::{LaunchSpec, SessionStatus};
use serde_json::json;
use std::collections::BTreeMap;

fn setup(
    home: &HomeLayout,
    root: &std::path::Path,
    group_id: &str,
    actor_id: &str,
) -> SessionStatus {
    home.initialize().expect("initialize home");
    cccc_core::codex_hook_state::begin_launch(
        home,
        "claude",
        group_id,
        actor_id,
        "token",
        "HookPending",
    )
    .expect("launch");
    cccc_core::codex_hook_state::record_runtime(
        home,
        "claude",
        group_id,
        actor_id,
        "token",
        &json!({"hook_event_name":"SessionStart","session_id":"s1"}),
    )
    .expect("session state");
    let runtime = cccc_runtime::start(LaunchSpec {
        group_id: group_id.into(),
        actor_id: actor_id.into(),
        runner: RunnerKind::Pty,
        command: vec!["sh".into(), "-c".into(), "sleep 2".into()],
        cwd: root.into(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    })
    .expect("runtime");
    crate::ops::runtime_hook_session::bind_for_test(
        home,
        group_id,
        actor_id,
        "claude",
        "token",
        runtime.pid.expect("pid"),
    );
    runtime
}

fn request(group_id: &str, actor_id: &str, data: &str) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: "terminal_write".into(),
        args: json!({"group_id":group_id,"actor_id":actor_id,"data":data})
            .as_object()
            .cloned()
            .expect("args"),
    }
}

#[test]
fn interrupt_input_clears_hook_working_state_without_terminal_output_parsing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group_id = format!("g_terminal_{}", uuid::Uuid::new_v4().simple());
    let actor_id = "claude-peer";
    setup(&home, temp.path(), &group_id, actor_id);
    cccc_core::codex_hook_state::record_terminal_input(&home, "claude", &group_id, actor_id)
        .expect("working state");

    assert!(write(&home, &request(&group_id, actor_id, "\u{3}")).is_ok());
    let state = cccc_core::codex_hook_state::read_runtime(&home, "claude", &group_id, actor_id)
        .expect("state");
    assert_eq!(state.status, "idle");
    assert_eq!(state.event, "UserInterrupt");
    assert_eq!(state.turn_id, None);
    assert!(is_interrupt_input("\u{1b}"));
    assert!(is_interrupt_input("\u{3}"));
    assert!(!is_interrupt_input("escape"));
    let _ = cccc_runtime::stop(&group_id, actor_id);
}

#[test]
fn terminal_input_opens_a_new_fail_closed_generation() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group_id = format!("g_terminal_{}", uuid::Uuid::new_v4().simple());
    let actor_id = "claude-peer";
    setup(&home, temp.path(), &group_id, actor_id);

    assert!(write(&home, &request(&group_id, actor_id, "\r")).is_ok());
    let state = cccc_core::codex_hook_state::read_runtime(&home, "claude", &group_id, actor_id)
        .expect("state");
    assert_eq!(state.status, "working");
    assert_eq!(state.event, "TerminalInputFailClosed");
    assert_eq!(state.turn_id.as_deref(), Some("local:1"));
    let _ = cccc_runtime::stop(&group_id, actor_id);
}
