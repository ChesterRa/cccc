use cccc_core::{HomeLayout, codex_hook_state};
use std::io::Write;
use std::process::{Command, Stdio};

fn run_hook(home: &std::path::Path, action: &str, token: &str, payload: &[u8]) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cccc"))
        .args(["hook", action])
        .env("CCCC_HOME", home)
        .env("CCCC_GROUP_ID", "g_test")
        .env("CCCC_ACTOR_ID", "peer1")
        .env("CCCC_HOOK_LAUNCH_TOKEN", token)
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn hook receiver");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload)
        .expect("write payload");
    assert!(child.wait().expect("wait").success());
}

#[test]
fn hidden_codex_hook_command_records_session_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    codex_hook_state::begin_launch(&home, "codex", "g_test", "peer1", "token", "HookPending")
        .expect("launch");
    run_hook(
        temp.path(),
        "codex-state",
        "token",
        br#"{"hook_event_name":"SessionStart","session_id":"session-1"}"#,
    );
    run_hook(
        temp.path(),
        "codex-state",
        "token",
        br#"{"hook_event_name":"UserPromptSubmit","session_id":"session-1","turn_id":"turn-1"}"#,
    );

    let state = codex_hook_state::read(&home, "g_test", "peer1").expect("hook state");
    assert_eq!(state.status, "working");
    assert_eq!(state.session_id, "session-1");
    assert_eq!(state.turn_id.as_deref(), Some("turn-1"));
}

#[test]
fn hidden_claude_hook_command_is_fail_closed_after_session_start() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    codex_hook_state::begin_launch(&home, "claude", "g_test", "peer1", "token", "HookPending")
        .expect("launch");
    run_hook(
        temp.path(),
        "claude-state",
        "token",
        br#"{"hook_event_name":"SessionStart","session_id":"session-1"}"#,
    );
    run_hook(
        temp.path(),
        "claude-state",
        "token",
        br#"{"hook_event_name":"UserPromptSubmit","session_id":"session-1","prompt_id":"prompt-1"}"#,
    );

    let state =
        codex_hook_state::read_runtime(&home, "claude", "g_test", "peer1").expect("hook state");
    assert_eq!(state.runtime, "claude");
    assert_eq!(state.status, "idle");
    assert_eq!(state.session_id, "session-1");
    assert_eq!(state.turn_id, None);
    assert_eq!(state.observation, "pty_fail_closed");
}

#[test]
fn hidden_hook_receiver_rejects_an_old_process_environment() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let pending = codex_hook_state::begin_launch(
        &home,
        "codex",
        "g_test",
        "peer1",
        "new-token",
        "HookPending",
    )
    .expect("launch");
    run_hook(
        temp.path(),
        "codex-state",
        "old-token",
        br#"{"hook_event_name":"SessionStart","session_id":"old-session"}"#,
    );
    assert_eq!(
        codex_hook_state::read(&home, "g_test", "peer1"),
        Some(pending)
    );
}
