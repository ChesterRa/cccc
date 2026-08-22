"""Inbox read-path operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.group import load_group
from ...kernel.inbox import (
    commit_read_cursor,
    get_cursor,
    get_cursor_details,
    is_message_for_actor,
    iter_events,
    recover_pending_read,
    unread_messages,
)
from ...kernel.ledger import append_event
from ...kernel.permissions import require_inbox_permission


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _read_limit(value: Any) -> int:
    if value is None:
        return 50
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError("limit must be an integer between 1 and 200")
    limit = value
    if limit < 1 or limit > 200:
        raise ValueError("limit must be an integer between 1 and 200")
    return limit


def _history_limit(value: Any) -> int:
    if value is None:
        return 50
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError("limit must be an integer between 1 and 100")
    limit = value
    if limit < 1 or limit > 100:
        raise ValueError("limit must be an integer between 1 and 100")
    return limit


def handle_inbox_peek(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    try:
        limit = _read_limit(args.get("limit"))
    except ValueError as exc:
        return _error("invalid_limit", str(exc))
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    if actor_id in {"user", "@user"}:
        return _error("invalid_inbox_recipient", "Inbox is only available for agents")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as e:
        return _error("permission_denied", str(e))
    try:
        recover_pending_read(group)
        messages = unread_messages(group, actor_id=actor_id, limit=limit)
        cur_event_id, cur_ts = get_cursor(group, actor_id)
    except Exception as e:
        return _error("io_error", str(e))
    return DaemonResponse(ok=True, result={"messages": messages, "cursor": {"event_id": cur_event_id, "ts": cur_ts}})


def handle_inbox_read(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    try:
        limit = _read_limit(args.get("limit"))
    except ValueError as exc:
        return _error("invalid_limit", str(exc))
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    if actor_id in {"user", "@user"}:
        return _error("invalid_inbox_recipient", "Inbox is only available for agents")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as e:
        return _error("permission_denied", str(e))
    try:
        recover_pending_read(group)
    except Exception as exc:
        return _error("io_error", str(exc))
    for attempt in range(3):
        try:
            cur_event_id, cur_ts, cur_updated_at = get_cursor_details(group, actor_id)
            messages = unread_messages(group, actor_id=actor_id, limit=limit)
            if not messages:
                return DaemonResponse(
                    ok=True,
                    result={
                        "messages": [],
                        "cursor": {
                            "event_id": cur_event_id,
                            "ts": cur_ts,
                            "updated_at": cur_updated_at,
                        },
                        "event": None,
                    },
                )
            boundary = messages[-1]
            event_id = str(boundary.get("id") or "").strip()
            ts = str(boundary.get("ts") or "")
            if not event_id:
                return _error("invalid_event", "unread boundary is missing event_id")
            cursor, read_event = commit_read_cursor(
                group,
                actor_id,
                expected_event_id=cur_event_id,
                expected_ts=cur_ts,
                event_id=event_id,
                ts=ts,
                append_read_event=lambda: append_event(
                    group.ledger_path,
                    kind="mail.read",
                    group_id=group.group_id,
                    scope_key="",
                    by=by,
                    data={"actor_id": actor_id, "event_id": event_id},
                ),
            )
            break
        except RuntimeError as exc:
            if "changed concurrently" in str(exc) and attempt < 2:
                continue
            return _error("concurrent_read", str(exc))
        except Exception as exc:
            return _error("io_error", str(exc))
    else:
        return _error("concurrent_read", "read cursor changed concurrently")
    return DaemonResponse(
        ok=True,
        result={"messages": messages, "cursor": cursor, "event": read_event},
    )


def handle_message_history(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    mode = str(args.get("mode") or "all").strip().replace("-", "_")
    query = str(args.get("query") or "").strip().casefold()
    before_event_id = str(args.get("before_event_id") or "").strip()
    try:
        limit = _history_limit(args.get("limit"))
    except ValueError as exc:
        return _error("invalid_limit", str(exc))
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    if mode not in {"all", "send", "request_reply", "mail"}:
        return _error(
            "invalid_message_mode",
            "mode must be all, send, request_reply, or mail",
        )
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as exc:
        return _error("permission_denied", str(exc))

    visible: list[Dict[str, Any]] = []
    for event in iter_events(group.ledger_path):
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if str(event.get("kind") or "") == "actor.add":
            actor = data.get("actor") if isinstance(data.get("actor"), dict) else {}
            if actor_id != "user" and str(actor.get("id") or "").strip() == actor_id:
                visible = []
            continue
        if str(event.get("kind") or "") != "chat.message":
            continue
        if str(event.get("by") or "").strip() != actor_id and not is_message_for_actor(
            group, actor_id=actor_id, event=event
        ):
            continue
        visible.append(event)

    if before_event_id:
        anchor = next(
            (
                index
                for index, event in enumerate(visible)
                if str(event.get("id") or "").strip() == before_event_id
            ),
            None,
        )
        if anchor is None:
            return _error("event_not_found", f"history anchor not found: {before_event_id}")
        visible = visible[:anchor]

    matches: list[Dict[str, Any]] = []
    for event in reversed(visible):
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if mode != "all" and str(data.get("message_mode") or "") != mode:
            continue
        if query:
            searchable = "\n".join(
                str(data.get(key) or "") for key in ("text", "insight", "quote_text")
            ).casefold()
            if query not in searchable:
                continue
        matches.append(event)
        if len(matches) > limit:
            break
    return DaemonResponse(
        ok=True,
        result={"messages": matches[:limit], "has_more": len(matches) > limit},
    )


def try_handle_inbox_read_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "inbox_peek":
        return handle_inbox_peek(args)
    if op == "inbox_read":
        return handle_inbox_read(args)
    if op == "message_history":
        return handle_message_history(args)
    return None
