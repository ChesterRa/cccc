"""Durable DeepSeek ACP delivery adapter."""
from __future__ import annotations

import time
import json
from typing import Any, Iterable

from ..actors import deepseek_runtime
from ...contracts.v1.deepseek import DEEPSEEK_TURN_TIMEOUT_SECONDS
from ...kernel.deepseek_acp import permission_request_id, terminal_stop_reason, validate_session_update
from ...kernel.headless_events import append_headless_event, has_headless_event_dedupe
from ...kernel.system_prompt import MESSAGE_DELIVERY_GUIDANCE

_CANCEL_CONFIRM_SECONDS = 5.0
_CREDENTIAL_ERROR_TOKENS = ("no api key", "deepseek_api_key")
_CONTEXT_WINDOW_ERROR_TOKENS = (
    "context_length_exceeded",
    "context_window_exceeded",
    "context length exceeded",
    "maximum context length",
)
DEEPSEEK_MESSAGE_GUIDANCE = f"[cccc] {MESSAGE_DELIVERY_GUIDANCE}"


def _normalize_turn_error(error: Any) -> tuple[Any, bool]:
    """Normalize permanent errors and tell delivery to require a manual restart."""
    try:
        searchable = json.dumps(error, ensure_ascii=False, default=str).lower()
    except (TypeError, ValueError):
        searchable = str(error).lower()
    if any(token in searchable for token in _CREDENTIAL_ERROR_TOKENS):
        return (
            {
                "code": "credential_unavailable",
                "category": "environment",
                "message": "DeepSeek API credential is not configured",
            },
            True,
        )
    if any(token in searchable for token in _CONTEXT_WINDOW_ERROR_TOKENS):
        return (
            {
                "code": "context_window_exceeded",
                "category": "context",
                "message": (
                    "DeepSeek request exceeded the model context window; "
                    "restart the actor to create a fresh session"
                ),
            },
            True,
        )
    return error, False


def _attempt_id(session_id: str, request_id: int) -> str:
    return f"{session_id}:{request_id}"


def _turn_id(event_id: str, attempt_id: str) -> str:
    return f"deepseek:{event_id}:{attempt_id}"


def _message_text(update: Any) -> str:
    if not isinstance(update, dict) or update.get("sessionUpdate") != "agent_message_chunk":
        return ""
    content = update.get("content")
    if not isinstance(content, dict) or content.get("type") != "text":
        return ""
    return str(content.get("text") or "")


def _cancel_and_confirm(supervisor: Any, request_id: int) -> dict[str, Any] | None:
    try:
        supervisor.cancel()
    except Exception:
        return None
    deadline = time.monotonic() + _CANCEL_CONFIRM_SECONDS
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return None
        try:
            frame = supervisor.next_frame(timeout=remaining)
        except Exception:
            return None
        if frame.get("method") == "session/request_permission":
            params = frame.get("params") if isinstance(frame.get("params"), dict) else {}
            try:
                permission_request_id(frame, supervisor.session_id)
                supervisor.respond_permission(frame.get("id"), params.get("options"), stopping=True)
            except Exception:
                return None
            continue
        if frame.get("id") == request_id:
            return frame


def _has_durable_completion(group: Any, event_id: str) -> bool:
    return has_headless_event_dedupe(
        group.path, f"deepseek.turn:headless.turn.completed:{event_id}"
    )


def recover_durable_terminals(group: Any, *, actor_id: str, limit: int = 256) -> int:
    """Settle a contiguous claimed prefix covered by durable provider terminals."""
    from ...kernel.actors import find_actor
    from .runtime_delivery import append_delivery_state, pending_runtime_delivery_events

    aid = str(actor_id or "").strip()
    actor = find_actor(group, aid)
    if not aid or not isinstance(actor, dict):
        return 0
    recovered = 0
    for event in pending_runtime_delivery_events(
        group,
        actor_id=aid,
        actor_created_at=str(actor.get("created_at") or "").strip(),
        transport="deepseek_headless",
        limit=max(1, int(limit or 256)),
        claim_unclaimed_chat=True,
    ):
        event_id = str(event.get("id") or "")
        if not event_id or not _has_durable_completion(group, event_id):
            break
        append_delivery_state(
            group,
            actor_id=aid,
            actor_created_at=str(actor.get("created_at") or "").strip(),
            source_event_id=event_id,
            state="accepted",
            transport="deepseek_headless",
            reason="recovered from durable provider completion",
        )
        recovered += 1
    return recovered


def deliver_messages(
    group: Any,
    *,
    actor_id: str,
    messages: Iterable[Any],
    timeout: float = float(DEEPSEEK_TURN_TIMEOUT_SECONDS),
) -> bool:
    supervisor = deepseek_runtime.get(group_id=str(group.group_id), actor_id=str(actor_id))
    if supervisor is None or not supervisor.session_id:
        return False
    from .delivery import render_single_message  # Avoid import cycle.
    for message in messages:
        event_id = str(getattr(message, "event_id", "") or "").strip()
        if not event_id:
            return False
        if _has_durable_completion(group, event_id):
            continue
        prompt = render_single_message(message)
        if str(getattr(message, "kind", "chat.message") or "chat.message") == "chat.message":
            prompt = f"{prompt.rstrip()}\n\n{DEEPSEEK_MESSAGE_GUIDANCE}"
        request_id: int | None = None
        terminal_received = False
        try:
            request_id = supervisor.submit(prompt)
            attempt_id = _attempt_id(str(supervisor.session_id), request_id)
            turn_id = _turn_id(event_id, attempt_id)
            stream_id = f"{turn_id}:message"
            append_headless_event(
                group.path,
                group_id=str(group.group_id),
                actor_id=str(actor_id),
                event_type="headless.turn.started",
                data={
                    "event_id": event_id,
                    "turn_id": turn_id,
                    "session_id": supervisor.session_id,
                    "request_id": request_id,
                    "status": "started",
                },
                dedupe_key=f"deepseek.turn.started:{event_id}:{attempt_id}",
            )
            update_ordinal = 0
            message_text = ""
            deadline = time.monotonic() + max(0.1, float(timeout))
            while True:
                remaining = deadline - time.monotonic()
                timed_out = False
                if remaining <= 0:
                    frame = _cancel_and_confirm(supervisor, request_id)
                    timed_out = True
                else:
                    try:
                        frame = supervisor.next_frame(timeout=remaining)
                    except TimeoutError:
                        frame = _cancel_and_confirm(supervisor, request_id)
                        timed_out = True
                if frame is None:
                    deepseek_runtime.stop(group_id=str(group.group_id), actor_id=str(actor_id))
                    return False
                method = str(frame.get("method") or "")
                if method == "session/update":
                    params = validate_session_update(frame, supervisor.session_id)
                    update = params.get("update")
                    ordinal = update_ordinal
                    update_ordinal += 1
                    delta = _message_text(update)
                    if delta:
                        message_text += delta
                        event_type = "headless.message.delta"
                        data = {
                            "event_id": event_id,
                            "turn_id": turn_id,
                            "stream_id": stream_id,
                            "delta": delta,
                        }
                    else:
                        update_kind = str(update.get("sessionUpdate") or "ACP update") if isinstance(update, dict) else "ACP update"
                        event_type = "headless.activity.updated"
                        data = {
                            "event_id": event_id,
                            "turn_id": turn_id,
                            "activity_id": f"{turn_id}:update:{ordinal}",
                            "kind": "thinking",
                            "status": "updated",
                            "summary": update_kind,
                            "detail": json.dumps(update, ensure_ascii=False, separators=(",", ":")),
                            "raw_item_type": update_kind,
                        }
                    append_headless_event(
                        group.path,
                        group_id=str(group.group_id),
                        actor_id=str(actor_id),
                        event_type=event_type,
                        data=data,
                        dedupe_key=f"deepseek.update:{event_id}:{attempt_id}:{ordinal}",
                    )
                    continue
                if method == "session/request_permission":
                    params = frame.get("params") if isinstance(frame.get("params"), dict) else {}
                    permission_request_id(frame, supervisor.session_id)
                    supervisor.respond_permission(
                        frame.get("id"),
                        params.get("options"),
                        stopping=False,
                    )
                    append_headless_event(
                        group.path,
                        group_id=str(group.group_id),
                        actor_id=str(actor_id),
                        event_type="headless.permission.responded",
                        data={"event_id": event_id, "turn_id": turn_id, "session_id": supervisor.session_id},
                    )
                    continue
                if frame.get("id") != request_id:
                    # Strict parser rejects unknown response ids; notifications
                    # that are not ACP update/permission are harmless here.
                    continue
                terminal_received = True
                stop_reason = terminal_stop_reason(frame)
                cancelled = stop_reason == "cancelled"
                failed = timed_out or "error" in frame or stop_reason != "end_turn"
                terminal_type = "headless.turn.failed" if failed else "headless.turn.completed"
                if message_text:
                    append_headless_event(
                        group.path,
                        group_id=str(group.group_id),
                        actor_id=str(actor_id),
                        event_type="headless.message.completed",
                        data={
                            "event_id": event_id,
                            "turn_id": turn_id,
                            "stream_id": stream_id,
                            "text": message_text,
                        },
                        dedupe_key=f"deepseek.message.completed:{event_id}:{attempt_id}",
                    )
                error = frame.get("error")
                manual_restart_required = False
                if timed_out:
                    error = {"message": "DeepSeek ACP turn timed out and was cancelled", "code": "timeout"}
                elif cancelled and not error:
                    error = {"message": "DeepSeek ACP turn was cancelled", "code": "cancelled"}
                else:
                    error, manual_restart_required = _normalize_turn_error(error)
                append_headless_event(
                    group.path,
                    group_id=str(group.group_id),
                    actor_id=str(actor_id),
                    event_type=terminal_type,
                    data={
                        "event_id": event_id,
                        "turn_id": turn_id,
                        "session_id": supervisor.session_id,
                        "request_id": request_id,
                        "result": frame.get("result"),
                        "error": error,
                        "status": "failed" if failed else "completed",
                    },
                    dedupe_key=(
                        f"deepseek.turn:{terminal_type}:{event_id}"
                        if terminal_type == "headless.turn.completed"
                        else f"deepseek.turn:{terminal_type}:{event_id}:{attempt_id}"
                    ),
                )
                if failed:
                    if manual_restart_required:
                        deepseek_runtime.stop(group_id=str(group.group_id), actor_id=str(actor_id))
                    return False
                break
        except Exception:
            if request_id is not None and not terminal_received:
                if _cancel_and_confirm(supervisor, request_id) is None:
                    deepseek_runtime.stop(group_id=str(group.group_id), actor_id=str(actor_id))
            return False
    return True
