from __future__ import annotations

from dataclasses import replace
from typing import Any, Mapping, Optional

from .contracts import (
    MAX_SEEN_OPERATIONS,
    MAX_SEEN_TURNS,
    STATE_VERSION,
    HookState,
)


def _field(payload: Mapping[str, Any], key: str) -> str:
    value = payload.get(key)
    return value.strip() if isinstance(value, str) else ""


def reduce_hook_event(
    previous: HookState,
    launch_token: str,
    payload: Mapping[str, Any],
    *,
    updated_at: str,
) -> HookState:
    event = _field(payload, "hook_event_name")
    if (
        previous.v != STATE_VERSION
        or not str(launch_token or "").strip()
        or launch_token != previous.launch_token
    ):
        return previous
    session_id = _field(payload, "session_id")
    if event == "SessionStart":
        if not session_id or previous.session_closed or not previous.awaiting_session_start:
            return previous
        return replace(
            previous,
            session_id=session_id,
            status="idle",
            event=event,
            awaiting_session_start=False,
            updated_at=updated_at,
        )
    if (
        previous.awaiting_session_start
        or previous.session_closed
        or not session_id
        or session_id != previous.session_id
    ):
        return previous
    if event == "SessionEnd":
        return replace(
            previous,
            status="stopped",
            event=event,
            turn_id=None,
            operation_id=None,
            seen_turn_ids=[],
            seen_operation_ids=[],
            turn_fence_exhausted=False,
            operation_fence_exhausted=False,
            diagnostic=None,
            session_closed=True,
            updated_at=updated_at,
        )
    if previous.runtime == "claude":
        return _reduce_claude_completion(
            previous, event, payload, updated_at=updated_at
        )
    return _reduce_codex(previous, event, payload, updated_at=updated_at)


def _reduce_claude_completion(
    previous: HookState,
    event: str,
    payload: Mapping[str, Any],
    *,
    updated_at: str,
) -> HookState:
    notification = _field(payload, "notification_type")
    is_completion = event == "Stop" or (
        event == "Notification"
        and notification in {"idle_prompt", "agent_completed"}
    )
    if (
        not is_completion
        or previous.status not in {"working", "waiting"}
        or previous.turn_id is None
        or not previous.turn_id.startswith("local:")
    ):
        return previous
    return replace(
        previous,
        status="idle",
        event=event,
        turn_id=None,
        operation_id=None,
        interrupted=False,
        diagnostic=None,
        updated_at=updated_at,
    )


def _reduce_codex(
    previous: HookState,
    event: str,
    payload: Mapping[str, Any],
    *,
    updated_at: str,
) -> HookState:
    if previous.turn_fence_exhausted:
        return previous
    turn_id = _field(payload, "turn_id") or None
    if event == "UserPromptSubmit":
        if (
            turn_id is None
            or previous.turn_id == turn_id
            or turn_id in previous.seen_turn_ids
        ):
            return previous
        if len(previous.seen_turn_ids) >= MAX_SEEN_TURNS:
            return replace(
                previous,
                status="idle",
                event="turn_fence_exhausted",
                turn_id=None,
                operation_id=None,
                turn_fence_exhausted=True,
                diagnostic="turn_history_exhausted",
                updated_at=updated_at,
            )
        return replace(
            previous,
            status="working",
            event=event,
            turn_id=turn_id,
            operation_id=None,
            interrupted=False,
            turn_generation=min(previous.turn_generation + 1, 2**64 - 1),
            seen_turn_ids=[*previous.seen_turn_ids, turn_id],
            seen_operation_ids=[],
            operation_fence_exhausted=False,
            diagnostic=None,
            updated_at=updated_at,
        )

    operation_id = _field(payload, "tool_use_id") or None
    turn_matches = turn_id is not None and turn_id == previous.turn_id
    if event == "PreToolUse":
        if previous.operation_fence_exhausted or operation_id is None:
            return previous
        if previous.operation_id is not None or operation_id in previous.seen_operation_ids:
            return previous
        if not turn_matches:
            return previous
        if len(previous.seen_operation_ids) >= MAX_SEEN_OPERATIONS:
            return replace(
                previous,
                event="operation_fence_exhausted",
                operation_id=None,
                operation_fence_exhausted=True,
                diagnostic="operation_history_exhausted",
                updated_at=updated_at,
            )
        return replace(
            previous,
            status="working",
            event=event,
            operation_id=operation_id,
            seen_operation_ids=[*previous.seen_operation_ids, operation_id],
            interrupted=False,
            diagnostic=None,
            updated_at=updated_at,
        )

    if event in {"PostToolUse", "PostToolUseFailure", "PermissionRequest"}:
        if (
            previous.operation_fence_exhausted
            or operation_id is None
            or operation_id != previous.operation_id
        ):
            return previous
    qualified = (
        operation_id == previous.operation_id
        if operation_id is not None
        else turn_matches
    )
    if not qualified:
        return previous
    status = _codex_status(event, payload)
    if status is None:
        return previous
    next_operation = (
        None
        if status == "idle" or event in {"PostToolUse", "PostToolUseFailure"}
        else previous.operation_id
    )
    return replace(
        previous,
        status=status,
        event=event,
        turn_id=None if status == "idle" else previous.turn_id,
        operation_id=next_operation,
        interrupted=False,
        diagnostic=None,
        updated_at=updated_at,
    )


def _codex_status(event: str, payload: Mapping[str, Any]) -> Optional[str]:
    if event in {"Stop", "StopFailure"}:
        return "idle"
    if event in {
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
        "SubagentStart",
        "SubagentStop",
    }:
        return "working"
    if event == "PermissionRequest":
        return "waiting"
    if event == "Notification":
        notification = _field(payload, "notification_type")
        if notification in {"permission_prompt", "elicitation_dialog", "agent_needs_input"}:
            return "waiting"
        if notification in {"idle_prompt", "agent_completed"}:
            return "idle"
    return None


def reduce_interrupt(previous: HookState, *, updated_at: str) -> HookState:
    if (
        previous.v != STATE_VERSION
        or previous.status not in {"working", "waiting"}
        or previous.awaiting_session_start
        or previous.session_closed
    ):
        return previous
    return replace(
        previous,
        status="idle",
        event="UserInterrupt",
        turn_id=None,
        operation_id=None,
        interrupted=True,
        updated_at=updated_at,
    )


def reduce_terminal_input(previous: HookState, *, updated_at: str) -> HookState:
    if (
        previous.v != STATE_VERSION
        or previous.runtime != "claude"
        or previous.awaiting_session_start
        or previous.session_closed
    ):
        return previous
    generation = min(previous.turn_generation + 1, 2**64 - 1)
    return replace(
        previous,
        status="working",
        event="TerminalInputFailClosed",
        turn_id=f"local:{generation}",
        operation_id=None,
        interrupted=False,
        turn_generation=generation,
        updated_at=updated_at,
    )
