use crate::codex_hook_state::CodexHookState;
use serde_json::Value;

pub(super) struct ActivityDraft {
    pub activity_id: String,
    pub kind: &'static str,
    pub status: &'static str,
    pub event_type: String,
    pub turn_id: Option<String>,
    pub operation_id: Option<String>,
    pub tool_name: Option<String>,
}

pub(super) fn project_hook_event(
    runtime: &str,
    launch_token: &str,
    payload: &Value,
    state: &CodexHookState,
) -> Option<ActivityDraft> {
    let event_type = field(payload, &["hook_event_name"])?;
    if !accepted_event(runtime, launch_token, &event_type, payload, state) {
        return None;
    }
    let turn_id = field(payload, &["turn_id", "prompt_id"]).or_else(|| state.turn_id.clone());
    let operation_id =
        field(payload, &["tool_use_id", "operation_id"]).or_else(|| state.operation_id.clone());
    let tool_name = field(payload, &["tool_name", "toolName"]).and_then(sanitize_label);
    let (kind, status, identity) = match event_type.as_str() {
        "SessionStart" => ("session", "started", state.session_id.clone()),
        "SessionEnd" => ("session", "completed", state.session_id.clone()),
        "UserPromptSubmit" => ("turn", "started", turn_id.clone()?),
        "PreToolUse" => ("tool", "started", operation_id.clone()?),
        "PermissionRequest" => ("tool", "waiting", operation_id.clone()?),
        "PostToolUse" => ("tool", "completed", operation_id.clone()?),
        "PostToolUseFailure" => ("tool", "failed", operation_id.clone()?),
        "SubagentStart" => (
            "subagent",
            "started",
            field(payload, &["agent_id", "subagent_id", "thread_id"])?,
        ),
        "SubagentStop" => (
            "subagent",
            "completed",
            field(payload, &["agent_id", "subagent_id", "thread_id"])?,
        ),
        "Stop" => ("turn", "completed", turn_id.clone()?),
        "StopFailure" => ("turn", "failed", turn_id.clone()?),
        "Notification" => match field(payload, &["notification_type"])?.as_str() {
            "permission_prompt" | "elicitation_dialog" | "agent_needs_input" => {
                ("turn", "waiting", turn_id.clone()?)
            }
            "idle_prompt" | "agent_completed" => ("turn", "completed", turn_id.clone()?),
            _ => return None,
        },
        _ => return None,
    };
    if runtime == "claude" && kind != "tool" && event_type != "SessionEnd" {
        return None;
    }
    Some(ActivityDraft {
        activity_id: format!(
            "{runtime}:{}:{kind}:{identity}",
            stable_part(&state.session_id)
        ),
        kind,
        status,
        event_type,
        turn_id,
        operation_id,
        tool_name,
    })
}

fn accepted_event(
    runtime: &str,
    launch_token: &str,
    event_type: &str,
    payload: &Value,
    state: &CodexHookState,
) -> bool {
    if state.v != 3
        || launch_token.trim().is_empty()
        || launch_token != state.launch_token
        || state.awaiting_session_start
        || state.session_closed && event_type != "SessionEnd"
    {
        return false;
    }
    let session_id = field(payload, &["session_id"]).unwrap_or_default();
    if session_id != state.session_id {
        return false;
    }
    if runtime == "claude" {
        return true;
    }
    if runtime != "codex" || state.event != event_type {
        return false;
    }
    match event_type {
        "UserPromptSubmit" => {
            let incoming_turn = field(payload, &["turn_id"]);
            incoming_turn.is_some() && incoming_turn == state.turn_id
        }
        "PreToolUse" => {
            let incoming_turn = field(payload, &["turn_id"]);
            let incoming_operation = field(payload, &["tool_use_id", "operation_id"]);
            incoming_turn.is_some()
                && incoming_turn == state.turn_id
                && incoming_operation.is_some()
                && incoming_operation == state.operation_id
        }
        "PermissionRequest" => {
            let incoming_operation = field(payload, &["tool_use_id", "operation_id"]);
            incoming_operation.is_some() && incoming_operation == state.operation_id
        }
        "PostToolUse" | "PostToolUseFailure" => {
            let incoming_operation = field(payload, &["tool_use_id", "operation_id"]);
            incoming_operation.is_some()
                && incoming_operation.as_ref() == state.seen_operation_ids.last()
        }
        "SubagentStart" | "SubagentStop" => field(payload, &["turn_id"])
            .is_some_and(|turn| state.turn_id.as_deref() == Some(turn.as_str())),
        _ => true,
    }
}

fn field(payload: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        payload
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub(super) fn sanitize_label(value: String) -> Option<String> {
    let sanitized = value
        .chars()
        .filter(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '-' | '.' | ':' | '/')
        })
        .take(64)
        .collect::<String>();
    (!sanitized.is_empty()).then_some(sanitized)
}

fn stable_part(value: &str) -> String {
    sanitize_label(value.to_owned()).unwrap_or_else(|| "unknown".into())
}
