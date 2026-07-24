use cccc_core::{HomeLayout, codex_hook_state};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn hidden_codex_hook_command_records_session_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cccc"))
        .args(["hook", "codex-state"])
        .env("CCCC_HOME", temp.path())
        .env("CCCC_GROUP_ID", "g_test")
        .env("CCCC_ACTOR_ID", "peer1")
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn hook receiver");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"session-1","turn_id":"turn-1"}"#,
        )
        .expect("write payload");
    assert!(child.wait().expect("wait").success());

    let state = codex_hook_state::read(&home, "g_test", "peer1").expect("hook state");
    assert_eq!(state.status, "working");
    assert_eq!(state.session_id, "session-1");
    assert_eq!(state.turn_id.as_deref(), Some("turn-1"));
}
