from __future__ import annotations

import logging
import re
from typing import Any, Dict, Set

from ...contracts.v1.notify import SystemNotifyData
from ...daemon.messaging.delivery import emit_system_notify
from ...kernel.context import ContextStorage
from ...kernel.group import get_group_state, load_group
from ...kernel.ledger_index import lookup_event_by_id
from ...kernel.pet_actor import PET_ACTOR_ID, get_pet_actor, is_desktop_pet_enabled
from ...kernel.pet_task_evidence import build_pet_task_evidence
from ...kernel.pet_task_triage import enum_text, trim_task_text
from . import assistive_jobs

PET_REVIEW_DEBOUNCE_SECONDS = 1.2
PET_REVIEW_MIN_INTERVAL_SECONDS = 3.0
PET_REVIEW_MAX_DELAY_SECONDS = 10.0
PET_REVIEW_LEASE_SECONDS = 90.0
_REVIEW_PACKET_SCHEMA = 1
_TASK_ID_RE = re.compile(r"\bT\d+\b")
LOGGER = logging.getLogger(__name__)


def _normalize_reason(value: str) -> str:
    return str(value or "").strip().lower() or "state_changed"


def _can_review_now(group_id: str) -> bool:
    group = load_group(group_id)
    if group is None:
        return False
    if not is_desktop_pet_enabled(group):
        return False
    if get_group_state(group) not in {"active", "idle"}:
        return False
    actor = get_pet_actor(group)
    if not isinstance(actor, dict):
        return False
    if not bool(actor.get("enabled", True)):
        return False
    return True


def _review_unavailable_reason(group_id: str) -> str:
    group = load_group(group_id)
    if group is None:
        return "group_not_found"
    if not is_desktop_pet_enabled(group):
        return "desktop_pet_disabled"
    if get_group_state(group) not in {"active", "idle"}:
        return f"group_state_{str(get_group_state(group) or '').strip().lower() or 'unavailable'}"
    actor = get_pet_actor(group)
    if not isinstance(actor, dict):
        return "pet_actor_missing"
    if not bool(actor.get("enabled", True)):
        return "pet_actor_disabled"
    return "review_unavailable"


def _truncate_text(value: Any, *, max_len: int = 160) -> str:
    text = " ".join(str(value or "").strip().split())
    if len(text) <= max_len:
        return text
    return text[: max(1, max_len - 1)].rstrip() + "…"


def _reason_rank(reason: str) -> int:
    priority = {
        "task_waiting_user": 0,
        "chat_reply": 1,
        "system_error": 2,
        "actor_stop": 3,
        "actor_restart": 4,
        "actor_start": 5,
        "task_handoff": 6,
        "task_blocked": 7,
        "coordination_brief_changed": 8,
        "group_state_active": 9,
        "group_state_changed": 10,
        "group_start": 11,
        "pet_enabled": 12,
        "chat_message": 13,
    }
    normalized = _normalize_reason(reason)
    return priority.get(normalized, 100)


def _task_counts(tasks: list[Any]) -> Dict[str, int]:
    waiting_user_count = 0
    blocked_count = 0
    handoff_count = 0
    planned_count = 0
    for task in tasks:
        status = enum_text(getattr(task, "status", ""))
        if status in {"done", "completed", "archived"}:
            continue
        waiting_on = enum_text(getattr(task, "waiting_on", ""))
        blocked_by = list(getattr(task, "blocked_by", []) or [])
        handoff_to = str(getattr(task, "handoff_to", "") or "").strip()
        if waiting_on == "user":
            waiting_user_count += 1
        if blocked_by or waiting_on in {"actor", "external"}:
            blocked_count += 1
        if handoff_to:
            handoff_count += 1
        if status == "planned":
            planned_count += 1
    return {
        "waiting_user_count": waiting_user_count,
        "blocked_count": blocked_count,
        "handoff_count": handoff_count,
        "planned_count": planned_count,
    }


def _task_focus_payload(task: Any) -> Dict[str, Any]:
    payload: Dict[str, Any] = {
        "id": str(getattr(task, "id", "") or "").strip(),
        "title": trim_task_text(getattr(task, "title", ""), max_len=96),
        "status": enum_text(getattr(task, "status", "")),
    }
    assignee = str(getattr(task, "assignee", "") or "").strip()
    if assignee:
        payload["assignee"] = assignee
    waiting_on = enum_text(getattr(task, "waiting_on", ""))
    if waiting_on and waiting_on != "none":
        payload["waiting_on"] = waiting_on
    blocked_by = [str(item or "").strip() for item in list(getattr(task, "blocked_by", []) or []) if str(item or "").strip()]
    if blocked_by:
        payload["blocked_by"] = blocked_by[:3]
    handoff_to = str(getattr(task, "handoff_to", "") or "").strip()
    if handoff_to:
        payload["handoff_to"] = handoff_to
    task_type = str(getattr(task, "task_type", "") or "").strip()
    if task_type:
        payload["task_type"] = task_type
    return payload


def _task_id_from_source_event(event: Dict[str, Any]) -> str:
    top_level_task_id = str(event.get("task_id") or "").strip()
    if top_level_task_id:
        return top_level_task_id
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    direct_task_id = str(data.get("task_id") or "").strip()
    if direct_task_id:
        return direct_task_id
    changes = data.get("changes") if isinstance(data.get("changes"), list) else []
    for item in changes:
        if not isinstance(item, dict):
            continue
        structured_task_id = str(item.get("task_id") or "").strip()
        if structured_task_id:
            return structured_task_id
        detail = str(item.get("detail") or "").strip()
        match = _TASK_ID_RE.search(detail)
        if match:
            return str(match.group(0) or "").strip()
    return ""


def _pick_focus_task(tasks: list[Any], *, reasons: Set[str], source_event: Dict[str, Any] | None) -> Dict[str, Any]:
    tasks_by_id: Dict[str, Any] = {}
    waiting_user: list[Any] = []
    handoff: list[Any] = []
    blocked: list[Any] = []
    planned: list[Any] = []
    for task in tasks:
        task_id = str(getattr(task, "id", "") or "").strip()
        if task_id:
            tasks_by_id[task_id] = task
        status = enum_text(getattr(task, "status", ""))
        if status in {"done", "completed", "archived"}:
            continue
        waiting_on = enum_text(getattr(task, "waiting_on", ""))
        blocked_by = list(getattr(task, "blocked_by", []) or [])
        handoff_to = str(getattr(task, "handoff_to", "") or "").strip()
        if waiting_on == "user":
            waiting_user.append(task)
        if handoff_to:
            handoff.append(task)
        if blocked_by or waiting_on in {"actor", "external"}:
            blocked.append(task)
        if status == "planned":
            planned.append(task)

    if isinstance(source_event, dict):
        source_task_id = _task_id_from_source_event(source_event)
        if source_task_id and source_task_id in tasks_by_id:
            return _task_focus_payload(tasks_by_id[source_task_id])

    ordered_reasons = sorted({_normalize_reason(item) for item in reasons if item}, key=_reason_rank)
    for reason in ordered_reasons:
        if reason == "task_waiting_user" and waiting_user:
            return _task_focus_payload(waiting_user[0])
        if reason == "task_handoff" and handoff:
            return _task_focus_payload(handoff[0])
        if reason == "task_blocked" and blocked:
            return _task_focus_payload(blocked[0])
        if reason == "coordination_brief_changed" and waiting_user:
            return _task_focus_payload(waiting_user[0])
        if reason in {"group_start", "group_state_active", "group_state_changed", "pet_enabled"} and planned:
            return _task_focus_payload(planned[0])

    if waiting_user:
        return _task_focus_payload(waiting_user[0])
    if handoff:
        return _task_focus_payload(handoff[0])
    if blocked:
        return _task_focus_payload(blocked[0])
    return {}


def _source_event_payload(group: Any, source_event_id: str) -> Dict[str, Any]:
    event_id = str(source_event_id or "").strip()
    if not event_id:
        return {}
    try:
        event = lookup_event_by_id(group.ledger_path, event_id)
    except Exception:
        return {}
    if not isinstance(event, dict):
        return {}
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    payload: Dict[str, Any] = {
        "id": event_id,
        "kind": str(event.get("kind") or "").strip(),
        "by": str(event.get("by") or "").strip(),
    }
    text = ""
    for key in ("text", "title", "message", "quote_text"):
        value = _truncate_text(data.get(key), max_len=180)
        if value:
            text = value
            break
    if text:
        payload["text"] = text
    reply_to = str(data.get("reply_to") or "").strip()
    if reply_to:
        payload["reply_to"] = reply_to
    if bool(data.get("reply_required") is True):
        payload["reply_required"] = True
    task_id = _task_id_from_source_event(event)
    if task_id:
        payload["task_id"] = task_id
    return payload


def _build_review_packet(group: Any, reasons: Set[str], source_event_id: str) -> Dict[str, Any]:
    normalized_reasons = sorted({_normalize_reason(item) for item in reasons if item}, key=_reason_rank)
    primary_reason = normalized_reasons[0] if normalized_reasons else "state_changed"
    packet: Dict[str, Any] = {
        "schema": _REVIEW_PACKET_SCHEMA,
        "primary_reason": primary_reason,
        "reasons": normalized_reasons,
        "group_state": get_group_state(group),
        "attention": {
            "waiting_user_count": 0,
            "blocked_count": 0,
            "handoff_count": 0,
            "planned_count": 0,
        },
    }
    source_event_id = str(source_event_id or "").strip()
    if source_event_id:
        packet["source_event_id"] = source_event_id
    try:
        storage = ContextStorage(group)
        context = storage.load_context()
        tasks = storage.list_tasks()
        agents = getattr(storage.load_agents(), "agents", []) or []
        coordination = getattr(context, "coordination", None)
        brief = getattr(coordination, "brief", None)
        brief_focus = _truncate_text(getattr(brief, "current_focus", ""), max_len=140)
        if brief_focus:
            packet["brief_focus"] = brief_focus
        packet["attention"] = _task_counts(tasks)
        source_event = _source_event_payload(group, source_event_id)
        if source_event:
            packet["source_event"] = source_event
        focus_task = _pick_focus_task(tasks, reasons=reasons, source_event=source_event if source_event else None)
        if focus_task:
            packet["focus_task"] = focus_task
        task_evidence = build_pet_task_evidence(tasks, agents, limit=4)
        if task_evidence:
            packet["task_evidence"] = task_evidence
    except Exception as exc:
        LOGGER.debug("pet_review_packet_build_failed group_id=%s err=%s", getattr(group, "group_id", ""), exc)
    return packet


def _pet_review_notify(group: Any, reasons: Set[str], source_event_id: str) -> SystemNotifyData:
    return SystemNotifyData(
        kind="info",
        priority="normal",
        title="Pet review requested",
        message="Re-evaluate current coordination state and refresh pet decisions.",
        target_actor_id=PET_ACTOR_ID,
        requires_ack=False,
        related_event_id=source_event_id or None,
        context={
            "kind": "pet_review",
            "reasons": sorted({item for item in reasons if item}),
            "source_event_id": source_event_id or None,
            "review_packet": _build_review_packet(group, reasons, source_event_id),
        },
    )


def _emit_pet_review(group_id: str, reasons: Set[str], source_event_id: str) -> None:
    group = load_group(group_id)
    if group is None:
        return
    if not _can_review_now(group_id):
        return
    emit_system_notify(group, by="system", notify=_pet_review_notify(group, reasons, source_event_id), async_flush=True)


def request_manual_pet_review(
    group_id: str,
    *,
    reason: str,
    source_event_id: str = "",
) -> bool:
    return assistive_jobs.request_job(
        group_id,
        job_kind=assistive_jobs.JOB_KIND_PET_REVIEW,
        trigger_class=assistive_jobs.TRIGGER_MANUAL,
        reason=_normalize_reason(reason),
        source_event_id=source_event_id,
        immediate=True,
    )


def request_pet_review(
    group_id: str,
    *,
    reason: str,
    source_event_id: str = "",
    immediate: bool = False,
) -> None:
    assistive_jobs.request_job(
        group_id,
        job_kind=assistive_jobs.JOB_KIND_PET_REVIEW,
        trigger_class=assistive_jobs.TRIGGER_EVENT,
        reason=_normalize_reason(reason),
        source_event_id=source_event_id,
        immediate=immediate,
    )


def cancel_pet_review(group_id: str) -> None:
    assistive_jobs.cancel_job(group_id, assistive_jobs.JOB_KIND_PET_REVIEW)


def recover_pending_pet_reviews() -> None:
    assistive_jobs.recover_jobs(job_kinds=(assistive_jobs.JOB_KIND_PET_REVIEW,))
