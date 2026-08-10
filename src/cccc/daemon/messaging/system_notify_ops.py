"""System notification operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse, SystemNotifyData
from ...kernel.actors import find_actor
from ...kernel.group import load_group
from ...kernel.inbox import find_event, is_message_for_actor
from ...kernel.ledger import append_event
from ...kernel.permissions import require_inbox_permission
from .delivery import emit_system_notify


def _error(
    code: str, message: str, *, details: Optional[Dict[str, Any]] = None
) -> DaemonResponse:
    return DaemonResponse(
        ok=False, error=DaemonError(code=code, message=message, details=(details or {}))
    )


def handle_system_notify(
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "system").strip()
    kind = str(args.get("kind") or "info").strip()
    priority = str(args.get("priority") or "normal").strip()
    title = str(args.get("title") or "").strip()
    message = str(args.get("message") or "").strip()
    target_actor_id = str(args.get("target_actor_id") or "").strip() or None
    im_visibility = str(args.get("im_visibility") or "internal").strip().lower()
    requires_ack = coerce_bool(args.get("requires_ack"))
    context = args.get("context") if isinstance(args.get("context"), dict) else {}

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    valid_kinds = {
        "nudge",
        "keepalive",
        "help_nudge",
        "actor_idle",
        "silence_check",
        "auto_idle",
        "automation",
        "status_change",
        "error",
        "info",
    }
    valid_priorities = {"low", "normal", "high", "urgent"}
    if kind not in valid_kinds:
        kind = "info"
    if priority not in valid_priorities:
        priority = "normal"
    if im_visibility not in {"internal", "public"}:
        im_visibility = "internal"

    notify = SystemNotifyData(
        kind=kind,
        priority=priority,
        title=title,
        message=message,
        target_actor_id=target_actor_id,
        im_visibility=im_visibility,
        requires_ack=requires_ack,
        context=context,
    )
    event = emit_system_notify(group, by=by, notify=notify)
    return DaemonResponse(ok=True, result={"event": event})


def handle_notify_ack(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    notify_event_id = str(args.get("notify_event_id") or "").strip()

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    if not notify_event_id:
        return _error("missing_notify_event_id", "missing notify_event_id")
    by = str(args.get("by") or actor_id).strip() or actor_id
    if by != actor_id:
        return _error("permission_denied", "ack must be performed by the recipient (by must equal actor_id)")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if actor_id != "user" and not isinstance(find_actor(group, actor_id), dict):
        return _error("unknown_actor", f"unknown actor: {actor_id}")
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as exc:
        return _error("permission_denied", str(exc))

    notify_event = find_event(group, notify_event_id)
    if notify_event is None:
        return _error("event_not_found", f"event not found: {notify_event_id}")
    if str(notify_event.get("kind") or "") != "system.notify":
        return _error("invalid_event_kind", "event is not a system.notify")
    if not is_message_for_actor(group, actor_id=actor_id, event=notify_event):
        return _error("event_not_for_actor", f"event is not addressed to actor: {actor_id}")

    event = append_event(
        group.ledger_path,
        kind="system.notify_ack",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={
            "notify_event_id": notify_event_id,
            "actor_id": actor_id,
        },
    )

    return DaemonResponse(ok=True, result={"event": event})


def try_handle_system_notify_op(
    op: str,
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
) -> Optional[DaemonResponse]:
    if op == "system_notify":
        return handle_system_notify(args, coerce_bool=coerce_bool)
    if op == "notify_ack":
        return handle_notify_ack(args)
    return None
