"""Pull-based runtime turn operations for website-hosted model actors."""

from __future__ import annotations

import hashlib
import json
from typing import Any, Dict, List, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor
from ...kernel.group import load_group
from ...kernel.inbox import find_event, is_message_for_actor
from ...kernel.ledger_index import lookup_event_positions
from ...kernel.ledger import append_event
from ...kernel.system_prompt import render_system_prompt
from ...util.time import utc_now_iso
from ..messaging.actor_turn_rendering import render_actor_event_batch_for_delivery
from ..messaging.runtime_delivery import (
    append_delivery_state,
    latest_delivery_state,
    pending_runtime_delivery_events,
    pending_runtime_delivery_sources,
)
from ..runner_state_ops import (
    read_headless_state,
    update_headless_state,
    web_model_actor_running,
)


_MAX_TURN_EVENTS = 20
_MAX_COALESCED_TEXT_CHARS = 24000
_COMPLETE_STATUSES = {"done", "partial", "failed", "cancelled"}
_DELIVERY_PREFERENCES_KEY = "web_model_delivery_preferences"
_DELIVERY_MODES = {"standard", "image_compat"}
_BROWSER_DELIVERY_STATES = {"submitting", "submitted", "bound", "pending", "ambiguous", "failed"}
_BROWSER_DELIVERY_METADATA_FIELDS = (
    "provider",
    "target_url",
    "bound_conversation_url",
    "pending_conversation_url",
    "auto_bind_new_chat",
    "resolved_pending_new_chat",
)


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _clean_text(value: Any) -> str:
    return str(value or "").strip()


def _normalize_browser_delivery(value: Any) -> tuple[Optional[Dict[str, Any]], Optional[DaemonResponse]]:
    if value is None:
        return None, None
    if not isinstance(value, dict):
        return None, _error("invalid_browser_delivery", "browser_delivery must be an object")
    state = _clean_text(value.get("state")).lower()
    if state not in _BROWSER_DELIVERY_STATES:
        return None, _error(
            "invalid_browser_delivery_state",
            "browser delivery state must be submitting, submitted, bound, pending, ambiguous, or failed",
        )
    normalized: Dict[str, Any] = {
        "state": state,
        "detail": str(value.get("detail") or "")[:4096],
    }
    for field in _BROWSER_DELIVERY_METADATA_FIELDS:
        if field in value:
            normalized[field] = value[field]
    return normalized, None


def _append_browser_delivery_event(
    group: Any,
    *,
    actor_id: str,
    turn_id: str,
    event_ids: List[str],
    delivery_id: str,
    browser_delivery: Dict[str, Any],
) -> tuple[Optional[Dict[str, Any]], Optional[DaemonResponse]]:
    state = _clean_text(browser_delivery.get("state")).lower()
    data: Dict[str, Any] = {
        "actor_id": actor_id,
        "turn_id": turn_id,
        "event_ids": event_ids,
        "latest_event_id": event_ids[-1],
        "delivery_id": delivery_id,
        "delivery_transport": "projected_session",
    }
    for field in _BROWSER_DELIVERY_METADATA_FIELDS:
        if field in browser_delivery:
            data[field] = browser_delivery[field]
    detail = _clean_text(browser_delivery.get("detail"))
    if detail:
        data["error" if state in {"failed", "ambiguous"} else "submission_evidence"] = detail
    return (
        append_event(
            group.ledger_path,
            kind=f"web_model.browser_delivery.{state}",
            group_id=group.group_id,
            scope_key="",
            by="system",
            data=data,
        ),
        None,
    )


def _completion_event_id(group_id: str, actor_id: str, turn_id: str) -> str:
    raw = f"runtime-completion\0{group_id}\0{actor_id}\0{turn_id}".encode("utf-8")
    return hashlib.sha256(raw).hexdigest()[:32]


def _completion_receipt(
    group: Any,
    *,
    actor_id: str,
    turn_id: str,
    event_ids: List[str],
    status: str,
    delivery_id: str,
) -> tuple[Optional[Dict[str, Any]], Optional[DaemonResponse]]:
    event = find_event(group, _completion_event_id(group.group_id, actor_id, turn_id))
    if event is None:
        return None, None
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    matches = (
        _clean_text(event.get("kind")) == "runtime.turn.completed"
        and _clean_text(event.get("by")) == actor_id
        and _clean_text(data.get("actor_id")) == actor_id
        and _clean_text(data.get("turn_id")) == turn_id
        and data.get("event_ids") == event_ids
        and _clean_text(data.get("status")) == status
        and _clean_text(data.get("delivery_id")) == delivery_id
    )
    if not matches:
        return None, _error(
            "completion_conflict",
            "turn completion receipt does not match this request",
        )
    return event, None


def _append_completion_receipt(
    group: Any,
    *,
    actor_id: str,
    turn_id: str,
    event_ids: List[str],
    status: str,
    delivery_id: str,
) -> Dict[str, Any]:
    return append_event(
        group.ledger_path,
        kind="runtime.turn.completed",
        group_id=group.group_id,
        scope_key="",
        by=actor_id,
        data={
            "actor_id": actor_id,
            "event_id": event_ids[-1],
            "turn_id": turn_id,
            "event_ids": event_ids,
            "status": status,
            "delivery_id": delivery_id,
        },
        event_id=_completion_event_id(group.group_id, actor_id, turn_id),
    )


def _coerce_limit(value: Any) -> int:
    try:
        limit = int(value or _MAX_TURN_EVENTS)
    except Exception:
        limit = _MAX_TURN_EVENTS
    return max(1, min(limit, _MAX_TURN_EVENTS))


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


def render_web_model_coalesced_text(
    messages: List[Dict[str, Any]], *, group: Any, actor_id: str = ""
) -> str:
    out = render_actor_event_batch_for_delivery(
        messages,
        actor_id=actor_id,
        group=group,
    )
    if len(out) <= _MAX_COALESCED_TEXT_CHARS:
        return out
    truncation = "\n\n[cccc] coalesced turn text truncated"
    hint_marker = "\n\n[cccc] MAIL PENDING:"
    body, marker, hint_tail = out.rpartition(hint_marker)
    suffix = f"{marker}{hint_tail}" if marker else ""
    available = max(1, _MAX_COALESCED_TEXT_CHARS - len(truncation) - len(suffix))
    return f"{(body if marker else out)[:available].rstrip()}{truncation}{suffix}"


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


def web_model_delivery_preference(group: Any, *, actor_id: str) -> Dict[str, Any]:
    preferences = group.doc.get(_DELIVERY_PREFERENCES_KEY) if isinstance(group.doc, dict) else None
    raw = preferences.get(actor_id) if isinstance(preferences, dict) else None
    stored = dict(raw) if isinstance(raw, dict) else {}
    mode = _clean_text(stored.get("mode")).lower()
    if mode not in _DELIVERY_MODES:
        mode = "standard"
    return {
        "mode": mode,
        "updated_at": _clean_text(stored.get("updated_at")),
        "updated_by": _clean_text(stored.get("updated_by")),
    }


def handle_web_model_delivery_preferences_get(args: Dict[str, Any]) -> DaemonResponse:
    group_id = _clean_text(args.get("group_id"))
    actor_id = _clean_text(args.get("actor_id"))
    group, actor, err = _validate_group_actor(group_id, actor_id)
    if err is not None:
        return err
    if _clean_text(actor.get("runtime")).lower() != "web_model":
        return _error("invalid_actor_runtime", "web-model delivery preferences require runtime=web_model")
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group_id,
            "actor_id": actor_id,
            "preference": web_model_delivery_preference(group, actor_id=actor_id),
        },
    )


def handle_web_model_delivery_preferences_update(args: Dict[str, Any]) -> DaemonResponse:
    group_id = _clean_text(args.get("group_id"))
    actor_id = _clean_text(args.get("actor_id"))
    group, actor, err = _validate_group_actor(group_id, actor_id)
    if err is not None:
        return err
    if _clean_text(actor.get("runtime")).lower() != "web_model":
        return _error("invalid_actor_runtime", "web-model delivery preferences require runtime=web_model")
    by = _clean_text(args.get("by"))
    if by != "user":
        return _error("permission_denied", "web-model delivery preferences are user-controlled")
    mode = _clean_text(args.get("mode")).lower()
    if mode not in _DELIVERY_MODES:
        return _error(
            "invalid_web_model_delivery_mode",
            "mode must be standard or image_compat",
            details={"mode": mode},
        )
    preferences = group.doc.get(_DELIVERY_PREFERENCES_KEY)
    if not isinstance(preferences, dict):
        preferences = {}
        group.doc[_DELIVERY_PREFERENCES_KEY] = preferences
    preference = {"mode": mode, "updated_at": utc_now_iso(), "updated_by": by}
    preferences[actor_id] = preference
    group.save()
    return DaemonResponse(
        ok=True,
        result={"group_id": group_id, "actor_id": actor_id, "preference": preference},
    )


def _web_model_actor_running(group_id: str, actor_id: str, actor: Dict[str, Any]) -> bool:
    if _clean_text(actor.get("id")) and _clean_text(actor.get("id")) != actor_id:
        return False
    normalized = dict(actor)
    normalized["id"] = actor_id
    return web_model_actor_running(group_id, normalized)


def web_model_queued_turn_info(group: Any, *, actor_id: str, headless_state: Optional[Dict[str, Any]]) -> Dict[str, Any]:
    """Summarize pending direct work that arrived after the active turn."""

    if not isinstance(headless_state, dict):
        return {"queued_count": 0}
    if _clean_text(headless_state.get("status")).lower() != "working":
        return {"queued_count": 0}
    active_turn_id = _clean_text(headless_state.get("active_turn_id"))
    active_latest_event_id = _clean_text(headless_state.get("latest_event_id"))
    if not active_turn_id or not active_latest_event_id:
        return {"queued_count": 0}

    pending = pending_runtime_delivery_sources(
        group,
        actor_id=actor_id,
        transport="",
        limit=10_000,
    )
    event_ids = [_clean_text(event.get("id")) for event in pending]
    try:
        positions = lookup_event_positions(
            group.ledger_path,
            [active_latest_event_id, *event_ids],
        )
    except Exception:
        return {"queued_count": 0}
    active_position = positions[0] if positions else None
    if active_position is None:
        return {"queued_count": 0}

    queued_count = 0
    queued_oldest_event_id = ""
    queued_latest_event_id = ""
    queued_latest_ts = ""
    for event, position in zip(pending, positions[1:]):
        event_id = _clean_text(event.get("id"))
        if not event_id or position is None or position <= active_position:
            continue
        queued_count += 1
        if not queued_oldest_event_id:
            queued_oldest_event_id = event_id
        queued_latest_event_id = event_id
        queued_latest_ts = _clean_text(event.get("ts"))

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


def handle_runtime_wait_next_turn(args: Dict[str, Any]) -> DaemonResponse:
    group_id = _clean_text(args.get("group_id"))
    actor_id = _clean_text(args.get("actor_id") or args.get("by"))
    by = _clean_text(args.get("by")) or actor_id
    if by != actor_id:
        return _error("permission_denied", "wait_next_turn must be called by the runtime actor")
    if "kind_filter" in args:
        return _error(
            "unsupported_field",
            "runtime delivery does not support Inbox kind filters",
        )
    transport = _clean_text(args.get("transport")) or "web_model_pull"
    if transport not in {"web_model_pull", "web_model_browser"}:
        return _error(
            "invalid_transport",
            "runtime delivery transport must be web_model_pull or web_model_browser",
        )
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
        return DaemonResponse(
            ok=True,
            result={
                "status": "stopped",
                "turn": None,
                "instructions": "This CCCC web_model actor is stopped. Do not continue polling until the actor is started again.",
            },
        )
    active_state = read_headless_state(group_id, actor_id)
    active_turn_id = _clean_text(active_state.get("active_turn_id"))
    if _clean_text(active_state.get("status")).lower() == "working" and active_turn_id:
        return DaemonResponse(
            ok=True,
            result={
                "status": "turn_in_progress",
                "turn": None,
                "active_turn_id": active_turn_id,
                "event_ids": list(active_state.get("active_event_ids") or []),
                "instructions": "Finish the active turn with cccc_runtime_complete_turn before requesting more work.",
            },
        )
    limit = _coerce_limit(args.get("limit"))
    messages = pending_runtime_delivery_events(
        group,
        actor_id=actor_id,
        actor_created_at=_clean_text(actor.get("created_at")),
        transport=transport,
        limit=limit,
        claim_unclaimed_chat=True,
    )
    if not messages:
        update_headless_state(group_id, actor_id, status="waiting", active_turn_id="", latest_event_id="")
        return DaemonResponse(
            ok=True,
            result={
                "status": "idle",
                "turn": None,
                "suggested_retry_after_ms": 5000,
                "instructions": "No pending direct work is available. Call cccc_runtime_wait_next_turn again after a short wait.",
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
        "coalesced_text": render_web_model_coalesced_text(
            compact_messages,
            group=group,
            actor_id=actor_id,
        ),
        "system_prompt": render_system_prompt(group=group, actor=actor),
        "delivery": {
            "mode": "runtime_delivery",
            "transport": transport,
            "max_events": limit,
            "web_model_mode": web_model_delivery_preference(group, actor_id=actor_id)["mode"],
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
        active_event_ids=list(turn.get("event_ids") or []),
    )
    if transport == "web_model_pull":
        for event_id in turn["event_ids"]:
            append_delivery_state(
                group,
                actor_id=actor_id,
                actor_created_at=_clean_text(actor.get("created_at")),
                source_event_id=event_id,
                state="accepted",
                transport=transport,
            )
    return DaemonResponse(ok=True, result={"status": "work_available", "turn": turn})


def handle_web_model_runtime_recover_turn(args: Dict[str, Any]) -> DaemonResponse:
    group_id = _clean_text(args.get("group_id"))
    actor_id = _clean_text(args.get("actor_id") or args.get("by"))
    group, actor, err = _validate_group_actor(group_id, actor_id)
    if err is not None:
        return err
    if _clean_text(actor.get("runtime")).lower() != "web_model":
        return _error("invalid_actor_runtime", "turn recovery is only available for runtime=web_model actors")
    raw_event_ids = args.get("event_ids")
    if not isinstance(raw_event_ids, list) or not raw_event_ids or any(not isinstance(item, str) for item in raw_event_ids):
        return _error("invalid_event_ids", "event_ids must be a non-empty list of strings")
    event_ids = [_clean_text(item) for item in raw_event_ids]
    if any(not item for item in event_ids) or len(set(event_ids)) != len(event_ids):
        return _error("invalid_event_ids", "event_ids must be non-empty and unique")
    events, event_error = _valid_turn_events(group, actor_id=actor_id, event_ids=event_ids)
    if event_error is not None:
        return event_error
    try:
        positions = lookup_event_positions(group.ledger_path, event_ids)
    except Exception as exc:
        return _error("ledger_read_failed", f"failed to locate turn events: {exc}")
    if any(position is None for position in positions):
        missing = [event_id for event_id, position in zip(event_ids, positions) if position is None]
        return _error("event_not_found", f"event not found: {missing[0]}")
    ordered = [
        event
        for _, event in sorted(
            zip((position for position in positions if position is not None), events),
            key=lambda item: item[0],
        )
    ]
    latest = ordered[-1]
    for event in ordered:
        delivery = latest_delivery_state(
            group,
            actor_id=actor_id,
            source_event_id=_clean_text(event.get("id")),
        )
        delivery_data = delivery.get("data") if isinstance(delivery, dict) and isinstance(delivery.get("data"), dict) else {}
        if _clean_text(delivery_data.get("state")) not in {"accepted", "ambiguous"}:
            return _error(
                "turn_not_delivered",
                "turn recovery only accepts events already handed to this runtime",
                details={"event_id": _clean_text(event.get("id"))},
            )
    compact_messages = [_compact_event(event) for event in ordered]
    turn = {
        "turn_id": _turn_id(group_id=group_id, actor_id=actor_id, messages=compact_messages),
        "group_id": group_id,
        "actor_id": actor_id,
        "created_at": utc_now_iso(),
        "event_ids": [_clean_text(event.get("id")) for event in compact_messages],
        "latest_event_id": _clean_text(latest.get("id")),
        "latest_ts": _clean_text(latest.get("ts")),
        "messages": compact_messages,
        "coalesced_text": render_web_model_coalesced_text(
            compact_messages,
            group=group,
            actor_id=actor_id,
        ),
        "system_prompt": render_system_prompt(group=group, actor=actor),
        "delivery": {
            "mode": "recovery_no_delivery_mutation",
            "web_model_mode": web_model_delivery_preference(group, actor_id=actor_id)["mode"],
        },
    }
    return DaemonResponse(ok=True, result={"status": "recovered", "turn": turn})


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
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        is_daemon_notice = (
            str(event.get("kind") or "") == "system.notify"
            and str(data.get("kind") or "") in {"mail_notice", "reply_notice"}
            and str(data.get("target_actor_id") or "").strip() == actor_id
        )
        if not is_daemon_notice and not is_message_for_actor(group, actor_id=actor_id, event=event):
            return [], _error("event_not_for_actor", f"event is not addressed to actor: {actor_id}", details={"event_id": event_id})
        events.append(event)
    return events, None


def _latest_event_by_ledger_order(group: Any, events: List[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    event_ids = [str(event.get("id") or "").strip() for event in events]
    if not event_ids or any(not event_id for event_id in event_ids):
        return None
    try:
        positions = lookup_event_positions(group.ledger_path, event_ids)
    except Exception:
        return None
    ranked = [
        (position, event)
        for position, event in zip(positions, events)
        if position is not None
    ]
    if len(ranked) != len(events):
        return None
    return max(ranked, key=lambda item: item[0])[1]


def record_web_model_delivery_outcome(
    group: Any,
    *,
    actor_id: str,
    turn: Dict[str, Any],
    by: str = "",
    state: str = "accepted",
    reason: str = "",
) -> Dict[str, Any]:
    """Record browser handoff facts without mutating the actor's read cursor."""

    del by
    delivery_state = _clean_text(state).lower()
    if delivery_state not in {"accepted", "failed", "ambiguous"}:
        return {"ok": False, "error": "invalid_delivery_state", "message": "state must be accepted, failed, or ambiguous"}

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

    actor = find_actor(group, actor_id) or {}
    recorded: List[Dict[str, Any]] = []
    for event in events:
        source_event_id = _clean_text(event.get("id"))
        latest = latest_delivery_state(group, actor_id=actor_id, source_event_id=source_event_id)
        latest_data = latest.get("data") if isinstance(latest, dict) and isinstance(latest.get("data"), dict) else {}
        if _clean_text(latest_data.get("state")) == delivery_state:
            continue
        recorded.append(
            append_delivery_state(
                group,
                actor_id=actor_id,
                actor_created_at=_clean_text(actor.get("created_at")),
                source_event_id=source_event_id,
                state=delivery_state,
                transport="web_model_browser",
                reason=_clean_text(reason),
            )
        )
    return {
        "ok": True,
        "delivery_state": delivery_state,
        "delivery_events": recorded,
        "processed_event_ids": [str(event.get("id") or "") for event in events],
    }


def handle_runtime_complete_turn(args: Dict[str, Any]) -> DaemonResponse:
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
    if "latest_event_id" in args:
        return _error(
            "unsupported_field",
            "complete_turn requires the exact active event_ids",
        )
    raw_event_ids = args.get("event_ids")
    if isinstance(raw_event_ids, list) and any(not isinstance(item, str) for item in raw_event_ids):
        return _error("invalid_event_ids", "event_ids must contain only strings")
    event_ids = [item.strip() for item in raw_event_ids] if isinstance(raw_event_ids, list) else []
    if not event_ids:
        return _error("missing_event_ids", "event_ids is required")
    if len(event_ids) > _MAX_TURN_EVENTS:
        return _error("invalid_event_ids", f"event_ids cannot contain more than {_MAX_TURN_EVENTS} entries")

    events, event_err = _valid_turn_events(group, actor_id=actor_id, event_ids=event_ids)
    if event_err is not None:
        return event_err
    active_state = read_headless_state(group_id, actor_id)
    active_turn_id = _clean_text(active_state.get("active_turn_id"))
    turn_id = _clean_text(args.get("turn_id")) or active_turn_id
    delivery_id = _clean_text(args.get("delivery_id")) or f"runtime:{turn_id}"
    receipt, receipt_error = _completion_receipt(
        group,
        actor_id=actor_id,
        turn_id=turn_id,
        event_ids=event_ids,
        status=status,
        delivery_id=delivery_id,
    )
    if receipt_error is not None:
        return receipt_error
    if receipt is not None:
        owns_active_projection = (
            _clean_text(active_state.get("status")) == "working"
            and active_turn_id == turn_id
            and [
                _clean_text(item)
                for item in (
                    active_state.get("active_event_ids")
                    if isinstance(active_state.get("active_event_ids"), list)
                    else []
                )
                if _clean_text(item)
            ]
            == event_ids
        )
        if owns_active_projection:
            update_headless_state(
                group_id,
                actor_id,
                status="waiting",
                active_turn_id="",
                latest_event_id="",
                active_event_ids=[],
            )
        return DaemonResponse(
            ok=True,
            result={
                "status": status,
                "turn_id": turn_id,
                "delivery_id": delivery_id,
                "completion_event": receipt,
                "processed_event_ids": event_ids,
                "followup_delivery_scheduled": False,
                "summary": _clean_text(args.get("summary")),
            },
        )
    if not active_turn_id or turn_id != active_turn_id:
        return _error(
            "stale_turn",
            "turn_id does not match the actor's active structured turn",
        )
    active_event_ids = [
        _clean_text(item)
        for item in (
            active_state.get("active_event_ids")
            if isinstance(active_state.get("active_event_ids"), list)
            else []
        )
        if _clean_text(item)
    ]
    if event_ids != active_event_ids:
        return _error(
            "completion_conflict",
            "event_ids do not match the actor's active structured turn",
        )
    for event_id in event_ids:
        delivery = latest_delivery_state(
            group,
            actor_id=actor_id,
            source_event_id=event_id,
        )
        delivery_data = (
            delivery.get("data")
            if isinstance(delivery, dict) and isinstance(delivery.get("data"), dict)
            else {}
        )
        if _clean_text(delivery_data.get("state")) not in {"accepted", "ambiguous"}:
            return _error(
                "turn_not_delivered",
                "complete_turn only accepts events already handed to this runtime",
                details={"event_id": event_id},
            )
    receipt = _append_completion_receipt(
        group,
        actor_id=actor_id,
        turn_id=turn_id,
        event_ids=event_ids,
        status=status,
        delivery_id=delivery_id,
    )
    followup_delivery_scheduled = False
    latest = _latest_event_by_ledger_order(group, events)
    latest_event_id = _clean_text(latest.get("id")) if isinstance(latest, dict) else ""
    if status in {"done", "partial"}:
        update_headless_state(
            group_id,
            actor_id,
            status="waiting",
            active_turn_id="",
            latest_event_id="",
            active_event_ids=[],
        )
        try:
            from .web_model_browser_delivery import (
                schedule_web_model_browser_delivery,
                web_model_browser_delivery_enabled,
            )

            if web_model_browser_delivery_enabled(group_id, actor):
                followup_delivery_scheduled = schedule_web_model_browser_delivery(
                    group_id=group_id,
                    actor_id=actor_id,
                    trigger_event_id=latest_event_id,
                )
        except Exception:
            followup_delivery_scheduled = False
    elif status in {"failed", "cancelled"}:
        update_headless_state(
            group_id,
            actor_id,
            status="waiting",
            active_turn_id="",
            latest_event_id="",
            active_event_ids=[],
        )

    try:
        from .web_model_browser_recovery_watcher import close_web_model_browser_reload_window

        close_web_model_browser_reload_window(
            group_id,
            actor_id,
            reason=f"complete_turn:{status}",
            detail=_clean_text(args.get("turn_id")),
        )
    except Exception:
        pass

    return DaemonResponse(
        ok=True,
        result={
            "status": status,
            "turn_id": turn_id,
            "delivery_id": delivery_id,
            "completion_event": receipt,
            "processed_event_ids": [str(event.get("id") or "") for event in events],
            "followup_delivery_scheduled": followup_delivery_scheduled,
            "summary": _clean_text(args.get("summary")),
        },
    )


def handle_web_model_browser_delivery_record(args: Dict[str, Any]) -> DaemonResponse:
    group_id = _clean_text(args.get("group_id"))
    actor_id = _clean_text(args.get("actor_id") or args.get("by"))
    by = _clean_text(args.get("by")) or actor_id
    if by != actor_id:
        return _error("permission_denied", "browser delivery records must be written by the runtime actor")
    group, actor, err = _validate_group_actor(group_id, actor_id)
    if err is not None:
        return err
    if _clean_text(actor.get("runtime")).lower() != "web_model":
        return _error("invalid_actor_runtime", "browser delivery records require runtime=web_model")
    turn_id = _clean_text(args.get("turn_id"))
    delivery_id = _clean_text(args.get("delivery_id"))
    if not turn_id:
        return _error("invalid_args", "turn_id is required")
    if not delivery_id:
        return _error("invalid_args", "delivery_id is required")
    raw_event_ids = args.get("event_ids")
    if not isinstance(raw_event_ids, list) or any(not isinstance(item, str) for item in raw_event_ids):
        return _error("invalid_event_ids", "event_ids must contain only strings")
    event_ids = [_clean_text(item) for item in raw_event_ids if _clean_text(item)]
    if not event_ids:
        return _error("missing_event_ids", "event_ids is required")
    if len(event_ids) > _MAX_TURN_EVENTS:
        return _error("invalid_event_ids", f"event_ids cannot contain more than {_MAX_TURN_EVENTS} entries")
    _events, event_err = _valid_turn_events(group, actor_id=actor_id, event_ids=event_ids)
    if event_err is not None:
        return event_err
    browser_delivery, browser_delivery_err = _normalize_browser_delivery(args.get("browser_delivery"))
    if browser_delivery_err is not None:
        return browser_delivery_err
    if browser_delivery is None:
        return _error("missing_browser_delivery", "browser_delivery is required")
    if "cursor_committed" in args:
        return _error(
            "unsupported_field",
            "browser delivery observations do not mutate the Mail cursor",
        )
    event, event_err = _append_browser_delivery_event(
        group,
        actor_id=actor_id,
        turn_id=turn_id,
        event_ids=event_ids,
        delivery_id=delivery_id,
        browser_delivery=browser_delivery,
    )
    if event_err is not None:
        return event_err
    runtime_state = {
        "submitted": "accepted",
        "bound": "accepted",
        "ambiguous": "ambiguous",
        "failed": "failed",
    }.get(_clean_text(browser_delivery.get("state")))
    if runtime_state:
        reason = _clean_text(browser_delivery.get("detail"))
        for source in _events:
            source_event_id = _clean_text(source.get("id"))
            latest = latest_delivery_state(
                group,
                actor_id=actor_id,
                source_event_id=source_event_id,
            )
            latest_data = (
                latest.get("data")
                if isinstance(latest, dict) and isinstance(latest.get("data"), dict)
                else {}
            )
            if _clean_text(latest_data.get("state")) == runtime_state:
                continue
            append_delivery_state(
                group,
                actor_id=actor_id,
                actor_created_at=_clean_text(actor.get("created_at")),
                source_event_id=source_event_id,
                state=runtime_state,
                transport="web_model_browser",
                reason=reason,
            )
    return DaemonResponse(ok=True, result={"event": event})


def try_handle_web_model_runtime_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "web_model_delivery_preferences_get":
        return handle_web_model_delivery_preferences_get(args)
    if op == "web_model_delivery_preferences_update":
        return handle_web_model_delivery_preferences_update(args)
    if op == "runtime_wait_next_turn":
        return handle_runtime_wait_next_turn(args)
    if op == "web_model_runtime_recover_turn":
        return handle_web_model_runtime_recover_turn(args)
    if op == "web_model_browser_delivery_record":
        return handle_web_model_browser_delivery_record(args)
    if op == "runtime_complete_turn":
        return handle_runtime_complete_turn(args)
    return None
