"""Inbox read-path operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.group import load_group
from ...kernel.inbox import (
    find_event,
    get_cursor,
    has_chat_ack,
    is_message_for_actor,
    set_cursor,
    unread_messages,
)
from ...kernel.ledger import append_event
from ...kernel.permissions import require_inbox_permission


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_inbox_list(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    limit_raw = args.get("limit")
    limit = int(limit_raw) if isinstance(limit_raw, int) else 50
    kind_filter = str(args.get("kind_filter") or "all").strip()
    if kind_filter not in ("all", "chat", "notify"):
        kind_filter = "all"
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as e:
        return _error("permission_denied", str(e))
    messages = unread_messages(group, actor_id=actor_id, limit=limit, kind_filter=kind_filter)  # type: ignore
    cur_event_id, cur_ts = get_cursor(group, actor_id)
    return DaemonResponse(ok=True, result={"messages": messages, "cursor": {"event_id": cur_event_id, "ts": cur_ts}})


def handle_inbox_mark_read(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    event_id = str(args.get("event_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    if not event_id:
        return _error("missing_event_id", "missing event_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as e:
        return _error("permission_denied", str(e))
    event = find_event(group, event_id)
    if event is None:
        return _error("event_not_found", f"event not found: {event_id}")
    if str(event.get("kind") or "") not in ("chat.message", "system.notify"):
        return _error("invalid_event_kind", "event kind must be chat.message or system.notify")
    if not is_message_for_actor(group, actor_id=actor_id, event=event):
        return _error("event_not_for_actor", f"event is not addressed to actor: {actor_id}")
    ts = str(event.get("ts") or "")
    cursor = set_cursor(group, actor_id, event_id=event_id, ts=ts)
    read_event = append_event(
        group.ledger_path,
        kind="chat.read",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"actor_id": actor_id, "event_id": event_id},
    )
    ack_event: Optional[dict[str, Any]] = None
    try:
        if by == actor_id and str(event.get("kind") or "") == "chat.message":
            data = event.get("data")
            if isinstance(data, dict) and str(data.get("priority") or "normal").strip() == "attention":
                sender = str(event.get("by") or "").strip()
                if sender and sender != actor_id and not has_chat_ack(group, event_id=event_id, actor_id=actor_id):
                    ack_event = append_event(
                        group.ledger_path,
                        kind="chat.ack",
                        group_id=group.group_id,
                        scope_key="",
                        by=by,
                        data={"actor_id": actor_id, "event_id": event_id},
                    )
    except Exception:
        ack_event = None
    return DaemonResponse(ok=True, result={"cursor": cursor, "event": read_event, "ack_event": ack_event})


def try_handle_inbox_read_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "inbox_list":
        return handle_inbox_list(args)
    if op == "inbox_mark_read":
        return handle_inbox_mark_read(args)
    return None
