from __future__ import annotations

from typing import Any, Mapping, Optional

from .contracts import STATE_VERSION, HookState


def project_hook_activity(
    runtime: str,
    launch_token: str,
    payload: Mapping[str, Any],
    state: HookState,
) -> Optional[dict[str, Any]]:
    event_type = _field(payload, "hook_event_name")
    if event_type is None or not _accepted(
        runtime, launch_token, event_type, payload, state
    ):
        return None
    turn_id = _field(payload, "turn_id", "prompt_id") or state.turn_id
    operation_id = (
        _field(payload, "tool_use_id", "operation_id")
        or state.operation_id
    )
    tool_name = _field(payload, "tool_name", "toolName")
    projected: Optional[tuple[str, str, Optional[str]]] = None
    if event_type == "SessionStart":
        projected = ("session", "started", state.session_id)
    elif event_type == "SessionEnd":
        projected = ("session", "completed", state.session_id)
    elif event_type == "UserPromptSubmit":
        projected = ("turn", "started", turn_id)
    elif event_type == "PreToolUse":
        projected = ("tool", "started", operation_id)
    elif event_type == "PermissionRequest":
        projected = ("tool", "waiting", operation_id)
    elif event_type == "PostToolUse":
        projected = ("tool", "completed", operation_id)
    elif event_type == "PostToolUseFailure":
        projected = ("tool", "failed", operation_id)
    elif event_type in {"SubagentStart", "SubagentStop"}:
        projected = (
            "subagent",
            "started" if event_type == "SubagentStart" else "completed",
            _field(payload, "agent_id", "subagent_id", "thread_id"),
        )
    elif event_type in {"Stop", "StopFailure"}:
        projected = (
            "turn",
            "completed" if event_type == "Stop" else "failed",
            turn_id,
        )
    elif event_type == "Notification":
        notification = _field(payload, "notification_type")
        if notification in {
            "permission_prompt",
            "elicitation_dialog",
            "agent_needs_input",
        }:
            projected = ("turn", "waiting", turn_id)
        elif notification in {"idle_prompt", "agent_completed"}:
            projected = ("turn", "completed", turn_id)
    if projected is None or projected[2] is None:
        return None
    kind, status, identity = projected
    if runtime == "claude" and kind != "tool" and event_type != "SessionEnd":
        return None
    session_part = _sanitize_label(state.session_id) or "unknown"
    return {
        "activity_id": f"{runtime}:{session_part}:{kind}:{identity}",
        "kind": kind,
        "status": status,
        "event_type": event_type,
        "turn_id": turn_id,
        "operation_id": operation_id,
        "tool_name": _sanitize_label(tool_name) if tool_name else None,
    }


def _accepted(
    runtime: str,
    launch_token: str,
    event_type: str,
    payload: Mapping[str, Any],
    state: HookState,
) -> bool:
    if (
        state.v != STATE_VERSION
        or not launch_token.strip()
        or launch_token != state.launch_token
        or state.awaiting_session_start
        or (state.session_closed and event_type != "SessionEnd")
        or _field(payload, "session_id") != state.session_id
    ):
        return False
    if runtime == "claude":
        return True
    if runtime != "codex" or state.event != event_type:
        return False
    incoming_turn = _field(payload, "turn_id")
    incoming_operation = _field(
        payload, "tool_use_id", "operation_id"
    )
    if event_type == "UserPromptSubmit":
        return incoming_turn is not None and incoming_turn == state.turn_id
    if event_type == "PreToolUse":
        return (
            incoming_turn is not None
            and incoming_turn == state.turn_id
            and incoming_operation is not None
            and incoming_operation == state.operation_id
        )
    if event_type == "PermissionRequest":
        return (
            incoming_operation is not None
            and incoming_operation == state.operation_id
        )
    if event_type in {"PostToolUse", "PostToolUseFailure"}:
        return (
            incoming_operation is not None
            and bool(state.seen_operation_ids)
            and incoming_operation == state.seen_operation_ids[-1]
        )
    if event_type in {"SubagentStart", "SubagentStop"}:
        return incoming_turn is not None and incoming_turn == state.turn_id
    return True


def _field(payload: Mapping[str, Any], *names: str) -> Optional[str]:
    for name in names:
        value = payload.get(name)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return None


def _sanitize_label(value: str) -> Optional[str]:
    clean = "".join(
        character
        for character in str(value)
        if character.isalnum() or character in "_-.:/"
    )[:64]
    return clean or None
