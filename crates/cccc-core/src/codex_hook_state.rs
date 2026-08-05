use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::HomeLayout;
use crate::fs::{read_json, with_exclusive_lock, write_json_committed as write_json};

const VERSION: u8 = 3;
const LEGACY_VERSION: u8 = 2;
const MAX_SEEN_TURNS: usize = 4096;
const MAX_SEEN_OPERATIONS: usize = 4096;
const CODEX: &str = "codex";
const CLAUDE: &str = "claude";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexHookState {
    pub v: u8,
    #[serde(default = "default_runtime")]
    pub runtime: String,
    pub group_id: String,
    pub actor_id: String,
    pub status: String,
    pub event: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub interrupted: bool,
    #[serde(default)]
    pub awaiting_session_start: bool,
    #[serde(default)]
    pub turn_generation: u64,
    #[serde(default)]
    pub launch_token: String,
    #[serde(default)]
    pub seen_turn_ids: Vec<String>,
    #[serde(default)]
    pub seen_operation_ids: Vec<String>,
    #[serde(default)]
    pub turn_fence_exhausted: bool,
    #[serde(default)]
    pub operation_fence_exhausted: bool,
    #[serde(default)]
    pub diagnostic: Option<String>,
    #[serde(default)]
    pub session_closed: bool,
    #[serde(default)]
    pub observation: String,
    pub updated_at: String,
}

pub fn record(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    launch_token: &str,
    payload: &Value,
) -> io::Result<CodexHookState> {
    record_runtime(home, CODEX, group_id, actor_id, launch_token, payload)
}

pub fn record_runtime(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
    launch_token: &str,
    payload: &Value,
) -> io::Result<CodexHookState> {
    record_runtime_with_observer(
        home,
        runtime,
        group_id,
        actor_id,
        launch_token,
        payload,
        |_, _| Ok(()),
    )
}

pub fn record_runtime_with_observer<F>(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
    launch_token: &str,
    payload: &Value,
    observer: F,
) -> io::Result<CodexHookState>
where
    F: FnOnce(&CodexHookState, bool) -> io::Result<()>,
{
    validate_runtime(runtime)?;
    with_exclusive_lock(&lock_path(home, runtime, group_id, actor_id), || {
        let previous = read_runtime(home, runtime, group_id, actor_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "hook event received before launch configuration",
            )
        })?;
        let state =
            record_runtime_locked(home, runtime, group_id, actor_id, launch_token, payload)?;
        let activity_authorized = state != previous
            || claude_observation_authorized(runtime, launch_token, payload, &previous);
        if let Err(error) = observer(&state, activity_authorized) {
            if state != previous {
                write_json(&path(home, runtime, group_id, actor_id), &previous)?;
            }
            return Err(error);
        }
        Ok(state)
    })
}

fn claude_observation_authorized(
    runtime: &str,
    launch_token: &str,
    payload: &Value,
    state: &CodexHookState,
) -> bool {
    runtime == CLAUDE
        && state.v == VERSION
        && !state.awaiting_session_start
        && !state.session_closed
        && !launch_token.trim().is_empty()
        && launch_token == state.launch_token
        && nonempty_field(payload, "session_id").as_deref() == Some(state.session_id.as_str())
}

fn record_runtime_locked(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
    launch_token: &str,
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
    let previous = read_runtime(home, runtime, group_id, actor_id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "hook event received before launch configuration",
        )
    })?;
    if previous.v != VERSION
        || launch_token.trim().is_empty()
        || launch_token != previous.launch_token
    {
        return Ok(previous);
    }
    let session_id = string_field(payload, "session_id");
    if event == "SessionStart" {
        if session_id.is_empty() || previous.session_closed {
            return Ok(previous);
        }
        if !previous.awaiting_session_start {
            return Ok(previous);
        }
        let state = CodexHookState {
            session_id,
            status: "idle".into(),
            event: event.to_owned(),
            awaiting_session_start: false,
            updated_at: utc_now(),
            ..previous
        };
        write_json(&path(home, runtime, group_id, actor_id), &state)?;
        return Ok(state);
    }
    if previous.awaiting_session_start
        || previous.session_closed
        || session_id.is_empty()
        || session_id != previous.session_id
    {
        return Ok(previous);
    }
    if event == "SessionEnd" {
        let state = CodexHookState {
            status: "stopped".into(),
            event: event.to_owned(),
            turn_id: None,
            operation_id: None,
            seen_turn_ids: Vec::new(),
            seen_operation_ids: Vec::new(),
            turn_fence_exhausted: false,
            operation_fence_exhausted: false,
            diagnostic: None,
            session_closed: true,
            updated_at: utc_now(),
            ..previous
        };
        write_json(&path(home, runtime, group_id, actor_id), &state)?;
        return Ok(state);
    }
    if runtime == CLAUDE {
        return record_claude_completion(home, group_id, actor_id, previous, event, payload);
    }
    record_codex_event(home, group_id, actor_id, previous, event, payload)
}

fn record_claude_completion(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    previous: CodexHookState,
    event: &str,
    payload: &Value,
) -> io::Result<CodexHookState> {
    let notification = string_field(payload, "notification_type");
    let is_completion = event == "Stop"
        || event == "Notification"
            && matches!(notification.as_str(), "idle_prompt" | "agent_completed");
    let owns_local_turn = previous
        .turn_id
        .as_deref()
        .is_some_and(|turn_id| turn_id.starts_with("local:"));
    if !is_completion
        || !matches!(previous.status.as_str(), "working" | "waiting")
        || !owns_local_turn
    {
        return Ok(previous);
    }
    let state = CodexHookState {
        status: "idle".into(),
        event: event.to_owned(),
        turn_id: None,
        operation_id: None,
        interrupted: false,
        diagnostic: None,
        updated_at: utc_now(),
        ..previous
    };
    write_json(&path(home, CLAUDE, group_id, actor_id), &state)?;
    Ok(state)
}

fn record_codex_event(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    previous: CodexHookState,
    event: &str,
    payload: &Value,
) -> io::Result<CodexHookState> {
    if previous.turn_fence_exhausted {
        return Ok(previous);
    }
    let turn_id = nonempty_field(payload, "turn_id");
    if event == "UserPromptSubmit" {
        let Some(turn_id) = turn_id else {
            return Ok(previous);
        };
        if previous.turn_id.as_deref() == Some(turn_id.as_str()) {
            return Ok(previous);
        }
        if previous.seen_turn_ids.iter().any(|seen| seen == &turn_id) {
            return Ok(previous);
        }
        if previous.seen_turn_ids.len() >= MAX_SEEN_TURNS {
            return record_turn_exhausted(home, group_id, actor_id, previous);
        }
        let mut seen_turn_ids = previous.seen_turn_ids.clone();
        seen_turn_ids.push(turn_id.clone());
        let state = CodexHookState {
            status: "working".into(),
            event: event.to_owned(),
            turn_id: Some(turn_id),
            operation_id: None,
            interrupted: false,
            turn_generation: previous.turn_generation.saturating_add(1),
            seen_turn_ids,
            seen_operation_ids: Vec::new(),
            operation_fence_exhausted: false,
            diagnostic: None,
            updated_at: utc_now(),
            ..previous
        };
        write_json(&path(home, CODEX, group_id, actor_id), &state)?;
        return Ok(state);
    }
    let operation_id = nonempty_field(payload, "tool_use_id");
    let turn_matches = turn_id
        .as_deref()
        .is_some_and(|incoming| previous.turn_id.as_deref() == Some(incoming));
    if event == "PreToolUse" {
        if previous.operation_fence_exhausted {
            return Ok(previous);
        }
        let Some(operation_id) = operation_id else {
            return Ok(previous);
        };
        if previous.operation_id.is_some() {
            return Ok(previous);
        }
        if previous
            .seen_operation_ids
            .iter()
            .any(|seen| seen == &operation_id)
        {
            return Ok(previous);
        }
        if !turn_matches {
            return Ok(previous);
        }
        if previous.seen_operation_ids.len() >= MAX_SEEN_OPERATIONS {
            return record_operation_exhausted(home, group_id, actor_id, previous);
        }
        let mut seen_operation_ids = previous.seen_operation_ids.clone();
        seen_operation_ids.push(operation_id.clone());
        let state = CodexHookState {
            status: "working".into(),
            event: event.to_owned(),
            operation_id: Some(operation_id),
            seen_operation_ids,
            interrupted: false,
            diagnostic: None,
            updated_at: utc_now(),
            ..previous
        };
        write_json(&path(home, CODEX, group_id, actor_id), &state)?;
        return Ok(state);
    }
    if is_operation_protocol_event(event) {
        if previous.operation_fence_exhausted {
            return Ok(previous);
        }
        let Some(operation_id) = operation_id.as_deref() else {
            return Ok(previous);
        };
        if previous.operation_id.as_deref() != Some(operation_id) {
            return Ok(previous);
        }
    }
    let qualified = match operation_id.as_deref() {
        Some(incoming) => previous.operation_id.as_deref() == Some(incoming),
        None => turn_matches,
    };
    if !qualified {
        return Ok(previous);
    }
    let Some(status) = status_for_codex_event(event, payload) else {
        return Ok(previous);
    };
    let next_operation = if status == "idle" {
        None
    } else {
        match event {
            "PostToolUse" | "PostToolUseFailure" => None,
            _ => previous.operation_id.clone(),
        }
    };
    let next_turn = if matches!(status, "idle") {
        None
    } else {
        previous.turn_id.clone()
    };
    let state = CodexHookState {
        status: status.to_owned(),
        event: event.to_owned(),
        turn_id: next_turn,
        operation_id: next_operation,
        interrupted: false,
        diagnostic: None,
        updated_at: utc_now(),
        ..previous
    };
    write_json(&path(home, CODEX, group_id, actor_id), &state)?;
    Ok(state)
}

fn record_turn_exhausted(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    previous: CodexHookState,
) -> io::Result<CodexHookState> {
    let state = CodexHookState {
        status: "idle".into(),
        event: "turn_fence_exhausted".into(),
        turn_id: None,
        operation_id: None,
        turn_fence_exhausted: true,
        diagnostic: Some("turn_history_exhausted".into()),
        updated_at: utc_now(),
        ..previous
    };
    write_json(&path(home, CODEX, group_id, actor_id), &state)?;
    Ok(state)
}

fn record_operation_exhausted(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    previous: CodexHookState,
) -> io::Result<CodexHookState> {
    let state = CodexHookState {
        event: "operation_fence_exhausted".into(),
        operation_id: None,
        operation_fence_exhausted: true,
        diagnostic: Some("operation_history_exhausted".into()),
        updated_at: utc_now(),
        ..previous
    };
    write_json(&path(home, CODEX, group_id, actor_id), &state)?;
    Ok(state)
}

fn is_operation_protocol_event(event: &str) -> bool {
    matches!(
        event,
        "PostToolUse" | "PostToolUseFailure" | "PermissionRequest"
    )
}

pub fn read(home: &HomeLayout, group_id: &str, actor_id: &str) -> Option<CodexHookState> {
    read_runtime(home, CODEX, group_id, actor_id)
}

pub fn read_runtime(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
) -> Option<CodexHookState> {
    validate_runtime(runtime).ok()?;
    let state: CodexHookState = read_json(&path(home, runtime, group_id, actor_id)).ok()?;
    (matches!(state.v, VERSION | LEGACY_VERSION)
        && state.runtime == runtime
        && state.group_id == group_id
        && state.actor_id == actor_id)
        .then_some(state)
}

pub fn remove(home: &HomeLayout, group_id: &str, actor_id: &str) {
    remove_runtime(home, CODEX, group_id, actor_id);
}

pub fn remove_runtime(home: &HomeLayout, runtime: &str, group_id: &str, actor_id: &str) {
    if validate_runtime(runtime).is_ok() {
        let _ = with_exclusive_lock(&lock_path(home, runtime, group_id, actor_id), || {
            match fs::remove_file(path(home, runtime, group_id, actor_id)) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        });
    }
}

pub fn record_interrupt(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
) -> io::Result<Option<CodexHookState>> {
    validate_runtime(runtime)?;
    with_exclusive_lock(&lock_path(home, runtime, group_id, actor_id), || {
        let Some(mut state) = read_runtime(home, runtime, group_id, actor_id) else {
            return Ok(None);
        };
        if state.v != VERSION
            || !matches!(state.status.as_str(), "working" | "waiting")
            || state.awaiting_session_start
            || state.session_closed
        {
            return Ok(Some(state));
        }
        let previous = state.clone();
        state.status = "idle".into();
        state.event = "UserInterrupt".into();
        state.turn_id = None;
        state.operation_id = None;
        state.interrupted = true;
        state.updated_at = utc_now();
        write_json(&path(home, runtime, group_id, actor_id), &state)?;
        if let Err(error) =
            crate::runtime_activity::close_actor_activities(home, &state, "UserInterrupt")
        {
            write_json(&path(home, runtime, group_id, actor_id), &previous)?;
            return Err(error);
        }
        Ok(Some(state))
    })
}

pub fn record_terminal_input(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
) -> io::Result<Option<CodexHookState>> {
    validate_runtime(runtime)?;
    with_exclusive_lock(&lock_path(home, runtime, group_id, actor_id), || {
        let Some(mut state) = read_runtime(home, runtime, group_id, actor_id) else {
            return Ok(None);
        };
        if state.v != VERSION
            || runtime != CLAUDE
            || state.awaiting_session_start
            || state.session_closed
        {
            return Ok(Some(state));
        }
        let previous = state.clone();
        let turn_generation = state.turn_generation.saturating_add(1);
        state.status = "working".into();
        state.event = "TerminalInputFailClosed".into();
        state.turn_id = Some(format!("local:{turn_generation}"));
        state.operation_id = None;
        state.interrupted = false;
        state.turn_generation = turn_generation;
        state.updated_at = utc_now();
        write_json(&path(home, runtime, group_id, actor_id), &state)?;
        if let Err(error) =
            crate::runtime_activity::close_actor_activities(home, &state, "TurnSuperseded")
        {
            write_json(&path(home, runtime, group_id, actor_id), &previous)?;
            return Err(error);
        }
        Ok(Some(state))
    })
}

pub fn begin_launch(
    home: &HomeLayout,
    runtime: &str,
    group_id: &str,
    actor_id: &str,
    launch_token: &str,
    event: &str,
) -> io::Result<CodexHookState> {
    validate_runtime(runtime)?;
    if group_id.trim().is_empty() || actor_id.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing group or actor identity",
        ));
    }
    if launch_token.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "launch token must be non-empty",
        ));
    }
    with_exclusive_lock(&lock_path(home, runtime, group_id, actor_id), || {
        let state = CodexHookState {
            v: VERSION,
            runtime: runtime.to_owned(),
            group_id: group_id.to_owned(),
            actor_id: actor_id.to_owned(),
            status: "waiting".into(),
            event: event.to_owned(),
            session_id: String::new(),
            turn_id: None,
            operation_id: None,
            interrupted: false,
            awaiting_session_start: true,
            turn_generation: 0,
            launch_token: launch_token.to_owned(),
            seen_turn_ids: Vec::new(),
            seen_operation_ids: Vec::new(),
            turn_fence_exhausted: false,
            operation_fence_exhausted: false,
            diagnostic: None,
            session_closed: false,
            observation: if runtime == CLAUDE {
                "pty_fail_closed".into()
            } else {
                "full_fidelity".into()
            },
            updated_at: utc_now(),
        };
        write_json(&path(home, runtime, group_id, actor_id), &state)?;
        Ok(state)
    })
}

fn status_for_codex_event(event: &str, payload: &Value) -> Option<&'static str> {
    match event {
        "Stop" | "StopFailure" => Some("idle"),
        "PreToolUse" | "PostToolUse" | "PostToolUseFailure" | "SubagentStart" | "SubagentStop" => {
            Some("working")
        }
        "PermissionRequest" => Some("waiting"),
        "Notification" => match string_field(payload, "notification_type").as_str() {
            "permission_prompt" | "elicitation_dialog" | "agent_needs_input" => Some("waiting"),
            "idle_prompt" | "agent_completed" => Some("idle"),
            _ => None,
        },
        _ => None,
    }
}

fn nonempty_field(payload: &Value, key: &str) -> Option<String> {
    let value = string_field(payload, key);
    (!value.is_empty()).then_some(value)
}

fn validate_runtime(runtime: &str) -> io::Result<()> {
    if matches!(runtime, CODEX | CLAUDE) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported hook runtime: {runtime}"),
        ))
    }
}

fn default_runtime() -> String {
    CODEX.to_owned()
}

fn string_field(payload: &Value, key: &str) -> String {
    payload
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn path(home: &HomeLayout, runtime: &str, group_id: &str, actor_id: &str) -> PathBuf {
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
        .join(format!("{runtime}-hook-state"))
        .join(format!("{key}.json"))
}

fn lock_path(home: &HomeLayout, runtime: &str, group_id: &str, actor_id: &str) -> PathBuf {
    path(home, runtime, group_id, actor_id).with_extension("lock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TOKEN: &str = "launch-current";

    fn launch(home: &HomeLayout, runtime: &str) -> CodexHookState {
        home.initialize().expect("initialize home");
        begin_launch(home, runtime, "g_test", "peer1", TOKEN, "HookPending").expect("launch")
    }

    fn hook(home: &HomeLayout, runtime: &str, token: &str, payload: Value) -> CodexHookState {
        record_runtime(home, runtime, "g_test", "peer1", token, &payload).expect("hook")
    }

    fn start(home: &HomeLayout, runtime: &str) -> CodexHookState {
        launch(home, runtime);
        hook(
            home,
            runtime,
            TOKEN,
            json!({"hook_event_name":"SessionStart","session_id":"s1"}),
        )
    }

    #[test]
    fn launch_token_and_session_start_form_a_strict_barrier() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        for runtime in [CODEX, CLAUDE] {
            let pending = launch(&home, runtime);
            for payload in [
                json!({"hook_event_name":"SessionStart","session_id":"old"}),
                json!({"hook_event_name":"UserPromptSubmit","session_id":"old","turn_id":"t0"}),
                json!({"hook_event_name":"PreToolUse","session_id":"old","turn_id":"t0","tool_use_id":"op0"}),
                json!({"hook_event_name":"PermissionRequest","session_id":"old","turn_id":"t0","tool_use_id":"op0"}),
                json!({"hook_event_name":"PostToolUse","session_id":"old","turn_id":"t0","tool_use_id":"op0"}),
                json!({"hook_event_name":"PostToolUseFailure","session_id":"old","turn_id":"t0","tool_use_id":"op0"}),
                json!({"hook_event_name":"SubagentStart","session_id":"old","turn_id":"t0"}),
                json!({"hook_event_name":"SubagentStop","session_id":"old","turn_id":"t0"}),
                json!({"hook_event_name":"Notification","notification_type":"agent_completed","session_id":"old","turn_id":"t0"}),
                json!({"hook_event_name":"Stop","session_id":"old","turn_id":"t0"}),
                json!({"hook_event_name":"StopFailure","session_id":"old","turn_id":"t0"}),
                json!({"hook_event_name":"SessionEnd","session_id":"old"}),
            ] {
                assert_eq!(hook(&home, runtime, "launch-old", payload), pending);
            }
        }
        let pending = read(&home, "g_test", "peer1").expect("codex pending");
        assert_eq!(
            hook(
                &home,
                CODEX,
                "",
                json!({"hook_event_name":"SessionStart","session_id":"s1"})
            ),
            pending
        );
        assert_eq!(
            hook(
                &home,
                CODEX,
                TOKEN,
                json!({"hook_event_name":"SessionStart","session_id":"  "})
            ),
            pending
        );
        let active = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"SessionStart","session_id":"s1"}),
        );
        assert_eq!(active.status, "idle");
        assert_eq!(active.session_id, "s1");
        assert!(!active.awaiting_session_start);
        assert_eq!(
            hook(
                &home,
                CODEX,
                TOKEN,
                json!({"hook_event_name":"SessionStart","session_id":"s2"})
            ),
            active
        );
    }

    #[test]
    fn codex_seen_turns_cannot_rebind_after_a_new_turn() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        start(&home, CODEX);
        let p1 = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"p1"}),
        );
        assert_eq!(p1.turn_generation, 1);
        assert_eq!(
            hook(
                &home,
                CODEX,
                TOKEN,
                json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"p1"})
            ),
            p1
        );
        let p2 = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"p2"}),
        );
        assert_eq!(p2.turn_generation, 2);
        for payload in [
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"p1"}),
            json!({"hook_event_name":"PermissionRequest","session_id":"s1","turn_id":"p1"}),
            json!({"hook_event_name":"Stop","session_id":"s1","turn_id":"p1"}),
        ] {
            assert_eq!(hook(&home, CODEX, TOKEN, payload), p2);
        }
    }

    #[test]
    fn codex_turn_or_bound_operation_is_required_for_state_writes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        start(&home, CODEX);
        let active = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"t1"}),
        );
        for payload in [
            json!({"hook_event_name":"PreToolUse","session_id":"s1","tool_use_id":"old-op"}),
            json!({"hook_event_name":"PermissionRequest","session_id":"s1","tool_use_id":"old-op"}),
            json!({"hook_event_name":"Stop","session_id":"s1"}),
            json!({"hook_event_name":"StopFailure","session_id":"s1"}),
            json!({"hook_event_name":"Notification","notification_type":"agent_completed","session_id":"s1"}),
        ] {
            assert_eq!(hook(&home, CODEX, TOKEN, payload), active);
        }
        let tool = hook(
            &home,
            CODEX,
            TOKEN,
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"s1",
                "turn_id":"t1",
                "tool_use_id":"op1"
            }),
        );
        assert_eq!(tool.operation_id.as_deref(), Some("op1"));
        let waiting = hook(
            &home,
            CODEX,
            TOKEN,
            json!({
                "hook_event_name":"PermissionRequest",
                "session_id":"s1",
                "tool_use_id":"op1"
            }),
        );
        assert_eq!(waiting.status, "waiting");
        let working = hook(
            &home,
            CODEX,
            TOKEN,
            json!({
                "hook_event_name":"PostToolUse",
                "session_id":"s1",
                "tool_use_id":"op1"
            }),
        );
        assert_eq!(working.status, "working");
        assert_eq!(working.operation_id, None);

        let op2 = hook(
            &home,
            CODEX,
            TOKEN,
            json!({
                "hook_event_name":"PreToolUse",
                "session_id":"s1",
                "turn_id":"t1",
                "tool_use_id":"op2"
            }),
        );
        assert_eq!(op2.operation_id.as_deref(), Some("op2"));
        for payload in [
            json!({"hook_event_name":"PreToolUse","session_id":"s1","turn_id":"t1","tool_use_id":"op1"}),
            json!({"hook_event_name":"PostToolUse","session_id":"s1","turn_id":"t1","tool_use_id":"op1"}),
            json!({"hook_event_name":"PermissionRequest","session_id":"s1","turn_id":"t1","tool_use_id":"op1"}),
            json!({"hook_event_name":"PermissionRequest","session_id":"s1","turn_id":"t1"}),
            json!({"hook_event_name":"PostToolUse","session_id":"s1","turn_id":"t1"}),
        ] {
            assert_eq!(hook(&home, CODEX, TOKEN, payload), op2);
        }
    }

    #[test]
    fn active_operation_must_close_before_a_second_operation_can_start() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        start(&home, CODEX);
        hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"t1"}),
        );
        let op1 = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"PreToolUse","session_id":"s1","turn_id":"t1","tool_use_id":"op1"}),
        );
        assert_eq!(
            hook(
                &home,
                CODEX,
                TOKEN,
                json!({"hook_event_name":"PreToolUse","session_id":"s1","turn_id":"t1","tool_use_id":"op1"})
            ),
            op1
        );
        assert_eq!(
            hook(
                &home,
                CODEX,
                TOKEN,
                json!({"hook_event_name":"PreToolUse","session_id":"s1","turn_id":"t1","tool_use_id":"op2"})
            ),
            op1
        );
        let closed = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"PostToolUse","session_id":"s1","tool_use_id":"op1"}),
        );
        assert_eq!(closed.operation_id, None);
        let op2 = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"PreToolUse","session_id":"s1","turn_id":"t1","tool_use_id":"op2"}),
        );
        assert_eq!(op2.operation_id.as_deref(), Some("op2"));
    }

    #[test]
    fn turn_history_exhaustion_revokes_the_active_turn_until_session_end() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        start(&home, CODEX);
        let mut state = read(&home, "g_test", "peer1").expect("state");
        state.seen_turn_ids = (0..MAX_SEEN_TURNS)
            .map(|index| format!("t{index}"))
            .collect();
        state.turn_id = Some(format!("t{}", MAX_SEEN_TURNS - 1));
        state.status = "working".into();
        write_json(&path(&home, CODEX, "g_test", "peer1"), &state).expect("seed history");

        let exhausted = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"overflow"}),
        );
        assert_eq!(exhausted.event, "turn_fence_exhausted");
        assert_eq!(
            exhausted.diagnostic.as_deref(),
            Some("turn_history_exhausted")
        );
        assert!(exhausted.turn_fence_exhausted);
        assert_eq!(exhausted.status, "idle");
        assert_eq!(exhausted.turn_id, None);
        assert_eq!(exhausted.operation_id, None);
        for payload in [
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"t0"}),
            json!({"hook_event_name":"PermissionRequest","session_id":"s1","turn_id":format!("t{}", MAX_SEEN_TURNS - 1)}),
            json!({"hook_event_name":"Stop","session_id":"s1","turn_id":format!("t{}", MAX_SEEN_TURNS - 1)}),
        ] {
            assert_eq!(hook(&home, CODEX, TOKEN, payload), exhausted);
        }
        let stopped = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"SessionEnd","session_id":"s1"}),
        );
        assert_eq!(stopped.status, "stopped");
        assert!(stopped.seen_turn_ids.is_empty());
        assert!(!stopped.turn_fence_exhausted);
        assert_eq!(stopped.diagnostic, None);
    }

    #[test]
    fn operation_history_exhaustion_revokes_operation_writes_for_that_turn() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        start(&home, CODEX);
        let mut state = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"t1"}),
        );
        state.seen_operation_ids = (0..MAX_SEEN_OPERATIONS)
            .map(|index| format!("op{index}"))
            .collect();
        write_json(&path(&home, CODEX, "g_test", "peer1"), &state).expect("seed operations");

        let exhausted = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"PreToolUse","session_id":"s1","turn_id":"t1","tool_use_id":"overflow"}),
        );
        assert_eq!(exhausted.event, "operation_fence_exhausted");
        assert_eq!(
            exhausted.diagnostic.as_deref(),
            Some("operation_history_exhausted")
        );
        assert!(exhausted.operation_fence_exhausted);
        assert_eq!(exhausted.operation_id, None);
        for payload in [
            json!({"hook_event_name":"PreToolUse","session_id":"s1","turn_id":"t1","tool_use_id":"op0"}),
            json!({"hook_event_name":"PostToolUse","session_id":"s1","turn_id":"t1","tool_use_id":"op0"}),
            json!({"hook_event_name":"PermissionRequest","session_id":"s1","turn_id":"t1","tool_use_id":"op0"}),
        ] {
            assert_eq!(hook(&home, CODEX, TOKEN, payload), exhausted);
        }
        let next_turn = hook(
            &home,
            CODEX,
            TOKEN,
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","turn_id":"t2"}),
        );
        assert!(!next_turn.operation_fence_exhausted);
        assert!(next_turn.seen_operation_ids.is_empty());
        assert_eq!(next_turn.diagnostic, None);
    }

    #[test]
    fn session_end_seals_the_launch_against_hooks_and_terminal_input() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        start(&home, CLAUDE);
        record_terminal_input(&home, CLAUDE, "g_test", "peer1").expect("terminal input");
        let stopped = hook(
            &home,
            CLAUDE,
            TOKEN,
            json!({"hook_event_name":"SessionEnd","session_id":"s1"}),
        );
        assert!(stopped.session_closed);
        assert_eq!(stopped.status, "stopped");
        assert_eq!(
            record_terminal_input(&home, CLAUDE, "g_test", "peer1")
                .expect("sealed input")
                .expect("state"),
            stopped
        );
        for payload in [
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"old"}),
            json!({"hook_event_name":"SessionStart","session_id":"s1"}),
        ] {
            assert_eq!(hook(&home, CLAUDE, TOKEN, payload), stopped);
        }
    }

    #[test]
    fn claude_pty_hooks_are_fail_closed_and_terminal_input_owns_generations() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let idle = start(&home, CLAUDE);
        assert_eq!(idle.observation, "pty_fail_closed");
        for payload in [
            json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"p1"}),
            json!({"hook_event_name":"PreToolUse","session_id":"s1","tool_use_id":"op1"}),
            json!({"hook_event_name":"PermissionRequest","session_id":"s1","tool_use_id":"op1"}),
            json!({"hook_event_name":"PostToolUse","session_id":"s1","tool_use_id":"op1"}),
            json!({"hook_event_name":"Stop","session_id":"s1"}),
            json!({"hook_event_name":"Notification","notification_type":"agent_completed","session_id":"s1"}),
        ] {
            assert_eq!(hook(&home, CLAUDE, TOKEN, payload), idle);
        }
        let g1 = record_terminal_input(&home, CLAUDE, "g_test", "peer1")
            .expect("input")
            .expect("state");
        assert_eq!(g1.status, "working");
        assert_eq!(g1.turn_id.as_deref(), Some("local:1"));
        let completed = hook(
            &home,
            CLAUDE,
            TOKEN,
            json!({"hook_event_name":"Stop","session_id":"s1"}),
        );
        assert_eq!(completed.status, "idle");
        assert_eq!(completed.event, "Stop");
        assert_eq!(completed.turn_id, None);

        let g2 = record_terminal_input(&home, CLAUDE, "g_test", "peer1")
            .expect("input")
            .expect("state");
        assert_eq!(g2.turn_id.as_deref(), Some("local:2"));
        let notified = hook(
            &home,
            CLAUDE,
            TOKEN,
            json!({
                "hook_event_name":"Notification",
                "notification_type":"agent_completed",
                "session_id":"s1"
            }),
        );
        assert_eq!(notified.status, "idle");
        assert_eq!(notified.event, "Notification");
        assert_eq!(notified.turn_id, None);

        let g3 = record_terminal_input(&home, CLAUDE, "g_test", "peer1")
            .expect("input")
            .expect("state");
        assert_eq!(g3.turn_id.as_deref(), Some("local:3"));
        let interrupted = record_interrupt(&home, CLAUDE, "g_test", "peer1")
            .expect("interrupt")
            .expect("state");
        assert_eq!(interrupted.status, "idle");
        assert!(interrupted.interrupted);
        assert_eq!(
            hook(
                &home,
                CLAUDE,
                TOKEN,
                json!({"hook_event_name":"UserPromptSubmit","session_id":"s1","prompt_id":"old"})
            ),
            interrupted
        );
        let g4 = record_terminal_input(&home, CLAUDE, "g_test", "peer1")
            .expect("new input")
            .expect("state");
        assert_eq!(g4.turn_id.as_deref(), Some("local:4"));
        assert!(!g4.interrupted);
    }

    #[test]
    fn v2_state_is_readable_but_permanently_unfenced() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        write_json(
            &path(&home, CODEX, "g_test", "peer1"),
            &json!({
                "v":2,
                "runtime":"codex",
                "group_id":"g_test",
                "actor_id":"peer1",
                "status":"working",
                "event":"UserPromptSubmit",
                "session_id":"legacy",
                "turn_id":"old",
                "updated_at":"legacy"
            }),
        )
        .expect("legacy state");
        let legacy = read(&home, "g_test", "peer1").expect("read legacy");
        assert_eq!(legacy.v, 2);
        assert_eq!(
            record(
                &home,
                "g_test",
                "peer1",
                TOKEN,
                &json!({"hook_event_name":"SessionEnd","session_id":"legacy"})
            )
            .expect("legacy event rejected"),
            legacy
        );
    }

    #[test]
    fn concurrent_interrupt_and_late_hook_finish_idle() {
        use std::sync::{Arc, Barrier};

        let temp = tempfile::tempdir().expect("tempdir");
        let home = Arc::new(HomeLayout::from_path(temp.path()).expect("home"));
        start(&home, CLAUDE);
        record_terminal_input(&home, CLAUDE, "g_test", "peer1").expect("working");
        let barrier = Arc::new(Barrier::new(3));
        let hook_home = Arc::clone(&home);
        let hook_barrier = Arc::clone(&barrier);
        let hook = std::thread::spawn(move || {
            hook_barrier.wait();
            record_runtime(
                &hook_home,
                CLAUDE,
                "g_test",
                "peer1",
                TOKEN,
                &json!({
                    "hook_event_name":"UserPromptSubmit",
                    "session_id":"s1",
                    "prompt_id":"old"
                }),
            )
            .expect("hook")
        });
        let interrupt_home = Arc::clone(&home);
        let interrupt_barrier = Arc::clone(&barrier);
        let interrupt = std::thread::spawn(move || {
            interrupt_barrier.wait();
            record_interrupt(&interrupt_home, CLAUDE, "g_test", "peer1")
                .expect("interrupt")
                .expect("state")
        });
        barrier.wait();
        hook.join().expect("hook thread");
        interrupt.join().expect("interrupt thread");

        let final_state = read_runtime(&home, CLAUDE, "g_test", "peer1").expect("final state");
        assert_eq!(final_state.status, "idle");
        assert_eq!(final_state.event, "UserInterrupt");
        assert!(final_state.interrupted);
    }
}
