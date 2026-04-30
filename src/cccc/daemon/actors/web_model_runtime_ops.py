"""Pull-based runtime turn operations for website-hosted model actors."""

from __future__ import annotations

import hashlib
import json
from typing import Any, Dict, List, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor
from ...kernel.group import load_group
from ...kernel.inbox import find_event, get_cursor, has_chat_ack, is_message_for_actor, set_cursor, unread_messages
from ...kernel.ledger import append_event
from ...util.time import parse_utc_iso, utc_now_iso
from ..messaging.actor_turn_rendering import render_actor_event_batch_for_delivery
from ..runner_state_ops import update_headless_state, web_model_actor_running


_MAX_TURN_EVENTS = 20
_MAX_COALESCED_TEXT_CHARS = 24000
_COMPLETE_STATUSES = {"done", "partial", "failed", "cancelled"}


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _clean_text(value: Any) -> str:
    return str(value or "").strip()


def _coerce_limit(value: Any) -> int:
    try:
        limit = int(value or _MAX_TURN_EVENTS)
    except Exception:
        limit = _MAX_TURN_EVENTS
    return max(1, min(limit, _MAX_TURN_EVENTS))


def _normalize_kind_filter(value: Any) -> str:
    kind_filter = _clean_text(value).lower() or "all"
    return kind_filter if kind_filter in {"all", "chat", "notify"} else "all"


def _compact_event(event: Dict[str, Any]) -> Dict[str, Any]:
    data = event.get("data")
    return {
        "id": str(event.get("id") or ""),
        "ts": str(event.get("ts") or ""),
        "kind": str(event.get("kind") or ""),
        "by": str(event.get("by") or ""),
        "scope_key": str(event.get("scope_key") or ""),
        "data": data if isinstance(data, dict) else {},
    }


def _coalesced_text(messages: List[Dict[str, Any]], *, actor_id: str = "") -> str:
    out = render_actor_event_batch_for_delivery(messages, actor_id=actor_id)
    if len(out) <= _MAX_COALESCED_TEXT_CHARS:
        return out
    return out[: _MAX_COALESCED_TEXT_CHARS - 80].rstrip() + "\n\n[cccc] coalesced turn text truncated"


def _turn_id(*, group_id: str, actor_id: str, messages: List[Dict[str, Any]]) -> str:
    payload = {
        "group_id": group_id,
        "actor_id": actor_id,
        "event_ids": [str(item.get("id") or "") for item in messages],
    }
    digest = hashlib.sha256(json.dumps(payload, ensure_ascii=False, sort_keys=True).encode("utf-8")).hexdigest()[:20]
    return f"webturn:{actor_id}:{digest}"


def _validate_group_actor(group_id: str, actor_id: str) -> tuple[Any, Dict[str, Any], Optional[DaemonResponse]]:
    if not group_id:
        return None, {}, _error("missing_group_id", "missing group_id")
    if not actor_id:
        return None, {}, _error("missing_actor_id", "missing actor_id")
    group = load_group(group_id)
    if group is None:
        return None, {}, _error("group_not_found", f"group not found: {group_id}")
    actor = find_actor(group, actor_id)
    if not isinstance(actor, dict):
        return None, {}, _error("actor_not_found", f"actor not found: {actor_id}")
    return group, dict(actor), None


def _web_model_actor_running(group_id: str, actor_id: str, actor: Dict[str, Any]) -> bool:
    if _clean_text(actor.get("id")) and _clean_text(actor.get("id")) != actor_id:
        return False
    normalized = dict(actor)
    normalized["id"] = actor_id
    return web_model_actor_running(group_id, normalized)


def web_model_queued_turn_info(group: Any, *, actor_id: str, headless_state: Optional[Dict[str, Any]]) -> Dict[str, Any]:
    """Summarize unread work that arrived after the active web-model turn."""

    if not isinstance(headless_state, dict):
        return {"queued_count": 0}
    if _clean_text(headless_state.get("status")).lower() != "working":
        return {"queued_count": 0}
    active_turn_id = _clean_text(headless_state.get("active_turn_id"))
    active_latest_event_id = _clean_text(headless_state.get("latest_event_id"))
    if not active_turn_id or not active_latest_event_id:
        return {"queued_count": 0}

    queued_count = 0
    queued_oldest_event_id = ""
    queued_latest_event_id = ""
    queued_latest_ts = ""
    seen_active_latest = False
    for event in unread_messages(group, actor_id=actor_id, limit=0, kind_filter="all"):
        event_id = _clean_text(event.get("id"))
        if not event_id:
            continue
        if seen_active_latest:
            queued_count += 1
            if not queued_oldest_event_id:
                queued_oldest_event_id = event_id
            queued_latest_event_id = event_id
            queued_latest_ts = _clean_text(event.get("ts"))
            continue
        if event_id == active_latest_event_id:
            seen_active_latest = True

    if not seen_active_latest:
        return {"queued_count": 0}
    return {
        "queued_count": queued_count,
        "queued_after_event_id": active_latest_event_id,
        "queued_oldest_event_id": queued_oldest_event_id,
        "queued_latest_event_id": queued_latest_event_id,
        "queued_latest_ts": queued_latest_ts,
    }


def decorate_web_model_queued_turn_info(
    actor: Dict[str, Any],
    group: Any,
    *,
    actor_id: str,
    headless_state: Optional[Dict[str, Any]],
) -> None:
    queued_info = web_model_queued_turn_info(group, actor_id=actor_id, headless_state=headless_state)
    actor["web_model_queued_count"] = int(queued_info.get("queued_count") or 0)
    if queued_info.get("queued_after_event_id"):
        actor["web_model_queued_after_event_id"] = queued_info.get("queued_after_event_id")
    if queued_info.get("queued_latest_event_id"):
        actor["web_model_queued_latest_event_id"] = queued_info.get("queued_latest_event_id")
    if queued_info.get("queued_latest_ts"):
        actor["web_model_queued_latest_ts"] = queued_info.get("queued_latest_ts")


def handle_web_model_runtime_wait_next_turn(args: Dict[str, Any]) -> DaemonResponse:
    group_id = _clean_text(args.get("group_id"))
    actor_id = _clean_text(args.get("actor_id") or args.get("by"))
    group, actor, err = _validate_group_actor(group_id, actor_id)
    if err is not None:
        return err
    if _clean_text(actor.get("runtime")).lower() != "web_model":
        return _error(
            "invalid_actor_runtime",
            "cccc_runtime_wait_next_turn is only available for runtime=web_model actors",
            details={"group_id": group_id, "actor_id": actor_id},
        )
    if not _web_model_actor_running(group_id, actor_id, actor):
        cursor_event_id, cursor_ts = get_cursor(group, actor_id)
        return DaemonResponse(
            ok=True,
            result={
                "status": "stopped",
                "turn": None,
                "cursor": {"event_id": cursor_event_id, "ts": cursor_ts},
                "instructions": "This CCCC web_model actor is stopped. Do not continue polling until the actor is started again.",
            },
        )
    limit = _coerce_limit(args.get("limit"))
    kind_filter = _normalize_kind_filter(args.get("kind_filter"))
    messages = unread_messages(group, actor_id=actor_id, limit=limit, kind_filter=kind_filter)  # type: ignore[arg-type]
    cursor_event_id, cursor_ts = get_cursor(group, actor_id)
    if not messages:
        update_headless_state(group_id, actor_id, status="waiting", active_turn_id="", latest_event_id="")
        return DaemonResponse(
            ok=True,
            result={
                "status": "idle",
                "turn": None,
                "cursor": {"event_id": cursor_event_id, "ts": cursor_ts},
                "suggested_retry_after_ms": 5000,
                "instructions": "No unread work is available. Call cccc_runtime_wait_next_turn again after a short wait.",
            },
        )

    compact_messages = [_compact_event(event) for event in messages]
    latest = compact_messages[-1]
    turn = {
        "turn_id": _turn_id(group_id=group_id, actor_id=actor_id, messages=compact_messages),
        "group_id": group_id,
        "actor_id": actor_id,
        "created_at": utc_now_iso(),
        "event_ids": [str(item.get("id") or "") for item in compact_messages if str(item.get("id") or "")],
        "latest_event_id": str(latest.get("id") or ""),
        "latest_ts": str(latest.get("ts") or ""),
        "messages": compact_messages,
        "coalesced_text": _coalesced_text(compact_messages, actor_id=actor_id),
        "delivery": {
            "mode": "cursor_on_complete",
            "cursor_committed": False,
            "max_events": limit,
            "kind_filter": kind_filter,
        },
        "instructions": (
            "Process this coalesced CCCC turn. Use CCCC MCP tools for visible replies, handoffs, repo edits, "
            "shell/git work, validation, and evidence. When finished, call cccc_runtime_complete_turn; "
            "if blocked or failed, still complete it with status=partial or failed and a concise summary."
        ),
    }
    update_headless_state(
        group_id,
        actor_id,
        status="working",
        active_turn_id=str(turn.get("turn_id") or ""),
        latest_event_id=str(turn.get("latest_event_id") or ""),
    )
    return DaemonResponse(ok=True, result={"status": "work_available", "turn": turn, "cursor": {"event_id": cursor_event_id, "ts": cursor_ts}})


def _valid_turn_events(group: Any, *, actor_id: str, event_ids: List[str]) -> tuple[List[Dict[str, Any]], Optional[DaemonResponse]]:
    events: List[Dict[str, Any]] = []
    seen: set[str] = set()
    for raw_id in event_ids:
        event_id = _clean_text(raw_id)
        if not event_id or event_id in seen:
            continue
        seen.add(event_id)
        event = find_event(group, event_id)
        if event is None:
            return [], _error("event_not_found", f"event not found: {event_id}")
        if str(event.get("kind") or "") not in {"chat.message", "system.notify"}:
            return [], _error("invalid_event_kind", "turn event kind must be chat.message or system.notify", details={"event_id": event_id})
        if not is_message_for_actor(group, actor_id=actor_id, event=event):
            return [], _error("event_not_for_actor", f"event is not addressed to actor: {actor_id}", details={"event_id": event_id})
        events.append(event)
    return events, None


def _latest_event_by_ts(events: List[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    latest: Optional[Dict[str, Any]] = None
    latest_dt: Any = None
    for event in events:
        dt = parse_utc_iso(str(event.get("ts") or ""))
        if latest is None:
            latest = event
            latest_dt = dt
            continue
        if dt is not None and latest_dt is not None:
            if dt >= latest_dt:
                latest = event
                latest_dt = dt
        else:
            latest = event
            latest_dt = dt
    return latest


def _cursor_covers_event(group: Any, *, actor_id: str, event: Dict[str, Any]) -> bool:
    _, cursor_ts = get_cursor(group, actor_id)
    cursor_dt = parse_utc_iso(cursor_ts) if cursor_ts else None
    event_dt = parse_utc_iso(str(event.get("ts") or ""))
    return bool(cursor_dt is not None and event_dt is not None and event_dt <= cursor_dt)


def _validate_unread_prefix_complete(group: Any, *, actor_id: str, event_ids: List[str], latest_event_id: str) -> Optional[DaemonResponse]:
    completed = {str(item or "").strip() for item in event_ids if str(item or "").strip()}
    prefix: List[str] = []
    for event in unread_messages(group, actor_id=actor_id, limit=0, kind_filter="all"):
        event_id = str(event.get("id") or "").strip()
        if not event_id:
            continue
        prefix.append(event_id)
        if event_id == latest_event_id:
            break
    if latest_event_id not in prefix:
        return _error("turn_not_unread", "latest_event_id is not currently unread for this actor", details={"latest_event_id": latest_event_id})
    missing = [event_id for event_id in prefix if event_id not in completed]
    if missing:
        return _error(
            "non_contiguous_turn_events",
            "complete_turn event_ids must include every currently unread event up to latest_event_id",
            details={"missing_event_ids": missing, "latest_event_id": latest_event_id},
        )
    return None


def _append_attention_acks(group: Any, *, actor_id: str, by: str, events: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    out: List[Dict[str, Any]] = []
    for event in events:
        if str(event.get("kind") or "") != "chat.message":
            continue
        data = event.get("data")
        if not isinstance(data, dict):
            continue
        if str(data.get("priority") or "normal").strip() != "attention":
            continue
        event_id = str(event.get("id") or "").strip()
        sender = str(event.get("by") or "").strip()
        if not event_id or sender == actor_id or has_chat_ack(group, event_id=event_id, actor_id=actor_id):
            continue
        out.append(
            append_event(
                group.ledger_path,
                kind="chat.ack",
                group_id=group.group_id,
                scope_key="",
                by=by,
                data={"actor_id": actor_id, "event_id": event_id},
            )
        )
    return out


def commit_web_model_delivered_turn(
    group: Any,
    *,
    actor_id: str,
    turn: Dict[str, Any],
    by: str = "",
) -> Dict[str, Any]:
    """Commit a turn after the runtime has received it through a delivery adapter."""

    raw_event_ids = turn.get("event_ids")
    event_ids = [str(item or "").strip() for item in raw_event_ids] if isinstance(raw_event_ids, list) else []
    latest_event_id = _clean_text(turn.get("latest_event_id"))
    if not event_ids and latest_event_id:
        event_ids = [latest_event_id]
    if not event_ids:
        return {"ok": False, "error": "missing_event_ids", "message": "turn event_ids are required"}

    events, event_err = _valid_turn_events(group, actor_id=actor_id, event_ids=event_ids)
    if event_err is not None:
        err = event_err.error
        return {
            "ok": False,
            "error": str(getattr(err, "code", "") or "invalid_turn_events"),
            "message": str(getattr(err, "message", "") or "invalid turn events"),
        }

    cursor = {"event_id": get_cursor(group, actor_id)[0], "ts": get_cursor(group, actor_id)[1]}
    read_event: Optional[Dict[str, Any]] = None
    ack_events: List[Dict[str, Any]] = []
    latest = _latest_event_by_ts(events)
    if latest is None:
        return {"ok": False, "error": "missing_latest_event", "message": "turn has no valid latest event"}

    latest_event_id = str(latest.get("id") or "").strip()
    latest_ts = str(latest.get("ts") or "").strip()
    if _cursor_covers_event(group, actor_id=actor_id, event=latest):
        return {
            "ok": True,
            "cursor_committed": True,
            "cursor": cursor,
            "read_event": None,
            "ack_events": [],
            "processed_event_ids": [str(event.get("id") or "") for event in events],
        }

    prefix_err = _validate_unread_prefix_complete(
        group,
        actor_id=actor_id,
        event_ids=[str(event.get("id") or "") for event in events],
        latest_event_id=latest_event_id,
    )
    if prefix_err is not None:
        err = prefix_err.error
        return {
            "ok": False,
            "error": str(getattr(err, "code", "") or "invalid_unread_prefix"),
            "message": str(getattr(err, "message", "") or "turn does not match unread prefix"),
        }

    committed_by = _clean_text(by) or actor_id
    cursor = set_cursor(group, actor_id, event_id=latest_event_id, ts=latest_ts)
    read_event = append_event(
        group.ledger_path,
        kind="chat.read",
        group_id=group.group_id,
        scope_key="",
        by=committed_by,
        data={"actor_id": actor_id, "event_id": latest_event_id},
    )
    ack_events = _append_attention_acks(group, actor_id=actor_id, by=committed_by, events=events)
    return {
        "ok": True,
        "cursor_committed": True,
        "cursor": cursor,
        "read_event": read_event,
        "ack_events": ack_events,
        "processed_event_ids": [str(event.get("id") or "") for event in events],
    }


def handle_web_model_runtime_complete_turn(args: Dict[str, Any]) -> DaemonResponse:
    group_id = _clean_text(args.get("group_id"))
    actor_id = _clean_text(args.get("actor_id") or args.get("by"))
    by = _clean_text(args.get("by")) or actor_id
    if by != actor_id:
        return _error("permission_denied", "complete_turn must be called by the runtime actor")
    group, actor, err = _validate_group_actor(group_id, actor_id)
    if err is not None:
        return err
    if _clean_text(actor.get("runtime")).lower() != "web_model":
        return _error(
            "invalid_actor_runtime",
            "cccc_runtime_complete_turn is only available for runtime=web_model actors",
            details={"group_id": group_id, "actor_id": actor_id},
        )
    if not _web_model_actor_running(group_id, actor_id, actor):
        return _error("actor_stopped", "web_model actor is stopped; completion was not committed")

    status = _clean_text(args.get("status")).lower() or "done"
    if status not in _COMPLETE_STATUSES:
        return _error("invalid_status", "status must be one of: done, partial, failed, cancelled")
    raw_event_ids = args.get("event_ids")
    event_ids = [str(item or "").strip() for item in raw_event_ids] if isinstance(raw_event_ids, list) else []
    if not event_ids:
        latest_event_id = _clean_text(args.get("latest_event_id"))
        if latest_event_id:
            event_ids = [latest_event_id]
    if not event_ids:
        return _error("missing_event_ids", "event_ids is required")

    events, event_err = _valid_turn_events(group, actor_id=actor_id, event_ids=event_ids)
    if event_err is not None:
        return event_err

    cursor = {"event_id": get_cursor(group, actor_id)[0], "ts": get_cursor(group, actor_id)[1]}
    read_event: Optional[Dict[str, Any]] = None
    ack_events: List[Dict[str, Any]] = []
    cursor_committed = False
    if status in {"done", "partial"}:
        latest = _latest_event_by_ts(events)
        if latest is not None:
            latest_event_id = str(latest.get("id") or "").strip()
            latest_ts = str(latest.get("ts") or "").strip()
            if _cursor_covers_event(group, actor_id=actor_id, event=latest):
                cursor_committed = True
            else:
                prefix_err = _validate_unread_prefix_complete(
                    group,
                    actor_id=actor_id,
                    event_ids=[str(event.get("id") or "") for event in events],
                    latest_event_id=latest_event_id,
                )
                if prefix_err is not None:
                    return prefix_err
                cursor = set_cursor(group, actor_id, event_id=latest_event_id, ts=latest_ts)
                read_event = append_event(
                    group.ledger_path,
                    kind="chat.read",
                    group_id=group.group_id,
                    scope_key="",
                    by=by,
                    data={"actor_id": actor_id, "event_id": latest_event_id},
                )
                ack_events = _append_attention_acks(group, actor_id=actor_id, by=by, events=events)
            cursor_committed = True

    followup_delivery_scheduled = False
    if cursor_committed and status in {"done", "partial"}:
        update_headless_state(group_id, actor_id, status="waiting", active_turn_id="", latest_event_id="")
        try:
            from .web_model_browser_delivery import (
                schedule_web_model_browser_delivery,
                web_model_browser_delivery_enabled,
            )

            if web_model_browser_delivery_enabled(group_id, actor):
                followup_delivery_scheduled = schedule_web_model_browser_delivery(
                    group_id=group_id,
                    actor_id=actor_id,
                    trigger_event_id=str(events[-1].get("id") or "") if events else "",
                )
        except Exception:
            followup_delivery_scheduled = False
    elif status in {"failed", "cancelled"}:
        update_headless_state(group_id, actor_id, status="waiting", active_turn_id="", latest_event_id="")

    return DaemonResponse(
        ok=True,
        result={
            "status": status,
            "turn_id": _clean_text(args.get("turn_id")),
            "cursor_committed": cursor_committed,
            "cursor": cursor,
            "read_event": read_event,
            "ack_events": ack_events,
            "processed_event_ids": [str(event.get("id") or "") for event in events],
            "followup_delivery_scheduled": followup_delivery_scheduled,
            "summary": _clean_text(args.get("summary")),
        },
    )


def try_handle_web_model_runtime_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "web_model_runtime_wait_next_turn":
        return handle_web_model_runtime_wait_next_turn(args)
    if op == "web_model_runtime_complete_turn":
        return handle_web_model_runtime_complete_turn(args)
    return None
