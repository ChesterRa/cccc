use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::HomeLayout;
use crate::fs::{read_json, write_json};

const VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexHookState {
    pub v: u8,
    pub group_id: String,
    pub actor_id: String,
    pub status: String,
    pub event: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub updated_at: String,
}

pub fn record(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    payload: &Value,
) -> io::Result<CodexHookState> {
    if group_id.trim().is_empty() || actor_id.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing CCCC_GROUP_ID or CCCC_ACTOR_ID",
        ));
    }
    let event = payload
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    let status = status_for_event(event).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported Codex hook event: {event}"),
        )
    })?;
    let state = CodexHookState {
        v: VERSION,
        group_id: group_id.to_owned(),
        actor_id: actor_id.to_owned(),
        status: status.to_owned(),
        event: event.to_owned(),
        session_id: string_field(payload, "session_id"),
        turn_id: matches!(status, "working" | "waiting")
            .then(|| string_field(payload, "turn_id"))
            .filter(|value| !value.is_empty()),
        updated_at: utc_now(),
    };
    write_json(&path(home, group_id, actor_id), &state)?;
    Ok(state)
}

pub fn read(home: &HomeLayout, group_id: &str, actor_id: &str) -> Option<CodexHookState> {
    let state: CodexHookState = read_json(&path(home, group_id, actor_id)).ok()?;
    (state.v == VERSION && state.group_id == group_id && state.actor_id == actor_id)
        .then_some(state)
}

pub fn remove(home: &HomeLayout, group_id: &str, actor_id: &str) {
    let _ = fs::remove_file(path(home, group_id, actor_id));
}

fn status_for_event(event: &str) -> Option<&'static str> {
    match event {
        "SessionStart" | "Stop" => Some("idle"),
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "SubagentStart" | "SubagentStop" => {
            Some("working")
        }
        "PermissionRequest" => Some("waiting"),
        "SessionEnd" => Some("stopped"),
        _ => None,
    }
}

fn string_field(payload: &Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn path(home: &HomeLayout, group_id: &str, actor_id: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(group_id.as_bytes());
    hasher.update([0]);
    hasher.update(actor_id.as_bytes());
    let key = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    home.daemon_dir()
        .join("codex-hook-state")
        .join(format!("{key}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_codex_lifecycle_events_to_runtime_state() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");

        for (event, expected) in [
            ("SessionStart", "idle"),
            ("UserPromptSubmit", "working"),
            ("PreToolUse", "working"),
            ("PermissionRequest", "waiting"),
            ("PostToolUse", "working"),
            ("Stop", "idle"),
            ("SessionEnd", "stopped"),
        ] {
            let state = record(
                &home,
                "g_test",
                "peer1",
                &json!({"hook_event_name":event,"session_id":"session-1","turn_id":"turn-1"}),
            )
            .expect("record hook");
            assert_eq!(state.status, expected);
            assert_eq!(read(&home, "g_test", "peer1"), Some(state));
        }
    }

    #[test]
    fn idle_events_clear_the_active_turn() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let state = record(
            &home,
            "g_test",
            "peer1",
            &json!({"hook_event_name":"Stop","turn_id":"turn-1"}),
        )
        .expect("record hook");
        assert_eq!(state.turn_id, None);
    }
}
