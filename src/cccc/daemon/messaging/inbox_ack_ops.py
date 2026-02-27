"""Inbox acknowledgement operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor
from ...kernel.group import load_group
from ...kernel.inbox import (
    find_event,
    get_cursor,
    has_chat_ack,
    is_message_for_actor,
    latest_unread_event,
    set_cursor,
)
from ...kernel.ledger import append_event
from ...kernel.permissions import require_inbox_permission


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_chat_ack(args: Dict[str, Any]) -> DaemonResponse:
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
    if not by:
        by = "user"
    if by != actor_id:
        return _error("permission_denied", "ack must be performed by the recipient (by must equal actor_id)")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    if actor_id != "user":
        actor = find_actor(group, actor_id)
        if not isinstance(actor, dict):
            return _error("unknown_actor", f"unknown actor: {actor_id}")

    target = find_event(group, event_id)
    if target is None:
        return _error("event_not_found", f"event not found: {event_id}")
    if str(target.get("kind") or "") != "chat.message":
        return _error("invalid_event_kind", "event kind must be chat.message")

    sender = str(target.get("by") or "").strip()
    if sender and sender == actor_id:
        return _error("cannot_ack_own_message", "cannot acknowledge your own message")

    data = target.get("data")
    if not isinstance(data, dict):
        return _error("invalid_event_data", "invalid message data")
    if str(data.get("priority") or "normal").strip() != "attention":
        return _error("not_an_attention_message", "message priority is not attention")

    if actor_id == "user":
        to_raw = data.get("to")
        to_tokens = [str(item).strip() for item in to_raw] if isinstance(to_raw, list) else []
        to_set = {item for item in to_tokens if item}
        if "user" not in to_set and "@user" not in to_set:
            return _error("event_not_for_actor", "message is not addressed to user")
    else:
        try:
            from ...util.time import parse_utc_iso

            msg_dt = parse_utc_iso(str(target.get("ts") or ""))
            actor = find_actor(group, actor_id)
            created_ts = str(actor.get("created_at") or "").strip() if isinstance(actor, dict) else ""
            created_dt = parse_utc_iso(created_ts) if created_ts else None
            if msg_dt is not None and created_dt is not None and created_dt > msg_dt:
                return _error("event_not_for_actor", f"actor did not exist at message time: {actor_id}")
        except Exception:
            pass
        if not is_message_for_actor(group, actor_id=actor_id, event=target):
            return _error("event_not_for_actor", f"event is not addressed to actor: {actor_id}")

    if has_chat_ack(group, event_id=event_id, actor_id=actor_id):
        return DaemonResponse(ok=True, result={"acked": True, "already": True, "event": None})

    ack_event = append_event(
        group.ledger_path,
        kind="chat.ack",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"actor_id": actor_id, "event_id": event_id},
    )
    return DaemonResponse(ok=True, result={"acked": True, "already": False, "event": ack_event})


def handle_inbox_mark_all_read(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
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

    last = latest_unread_event(group, actor_id=actor_id, kind_filter=kind_filter)  # type: ignore
    if last is None:
        cur_event_id, cur_ts = get_cursor(group, actor_id)
        return DaemonResponse(ok=True, result={"cursor": {"event_id": cur_event_id, "ts": cur_ts}, "event": None})

    event_id = str(last.get("id") or "").strip()
    ts = str(last.get("ts") or "").strip()
    if not event_id or not ts:
        cur_event_id, cur_ts = get_cursor(group, actor_id)
        return DaemonResponse(ok=True, result={"cursor": {"event_id": cur_event_id, "ts": cur_ts}, "event": None})

    cursor = set_cursor(group, actor_id, event_id=event_id, ts=ts)
    read_event = append_event(
        group.ledger_path,
        kind="chat.read",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"actor_id": actor_id, "event_id": event_id},
    )
    return DaemonResponse(ok=True, result={"cursor": cursor, "event": read_event})


def try_handle_inbox_ack_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "chat_ack":
        return handle_chat_ack(args)
    if op == "inbox_mark_all_read":
        return handle_inbox_mark_all_read(args)
    return None
