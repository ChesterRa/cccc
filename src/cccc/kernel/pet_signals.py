from __future__ import annotations

import json
from statistics import median
from typing import TYPE_CHECKING, Any, Dict, Iterable, List, Optional

from ..util.time import parse_utc_iso, utc_now_iso
from .actors import find_foreman
from .inbox import get_obligation_status_batch
from .ledger_index import lookup_events_by_ids, search_event_ids_indexed

if TYPE_CHECKING:
    from .group import Group


_RECENT_CHAT_LIMIT = 240
_RECENT_PAGE_SIZE = 120
_TREND_WINDOW_SECONDS = 3600
_RECENT_CONTEXT_SYNC_LIMIT = 120
_ACTIVE_TAIL_SCAN_LINES = 240


def _read_active_ledger_tail_lines(group: Group, *, max_lines: int = _ACTIVE_TAIL_SCAN_LINES) -> List[str]:
    path = group.ledger_path
    if not path.exists():
        return []
    wanted = max(1, int(max_lines or _ACTIVE_TAIL_SCAN_LINES))
    chunks: List[bytes] = []
    line_count = 0
    chunk_size = 8192
    try:
        with path.open("rb") as handle:
            handle.seek(0, 2)
            pos = handle.tell()
            while pos > 0 and line_count <= wanted:
                read_size = min(chunk_size, pos)
                pos -= read_size
                handle.seek(pos)
                chunk = handle.read(read_size)
                chunks.append(chunk)
                line_count += chunk.count(b"\n")
    except Exception:
        return []
    raw = b"".join(reversed(chunks))
    return [line for line in raw.decode("utf-8", errors="replace").splitlines()[-wanted:] if line.strip()]


def _iter_recent_chat_events(
    group: Group,
    *,
    limit: int = _RECENT_CHAT_LIMIT,
    source: str = "indexed",
) -> Iterable[Dict[str, Any]]:
    remaining = max(1, int(limit or _RECENT_CHAT_LIMIT))
    if str(source or "").strip().lower() == "active_tail":
        found = 0
        for line in reversed(_read_active_ledger_tail_lines(group, max_lines=max(_ACTIVE_TAIL_SCAN_LINES, remaining * 20))):
            try:
                event = json.loads(line)
            except Exception:
                continue
            if not isinstance(event, dict) or str(event.get("kind") or "") != "chat.message":
                continue
            yield event
            found += 1
            if found >= remaining:
                break
        return

    before_id = ""
    while remaining > 0:
        page_limit = min(_RECENT_PAGE_SIZE, remaining)
        event_ids, has_more = search_event_ids_indexed(
            group.ledger_path,
            allowed_kinds={"chat.message"},
            before_id=before_id,
            limit=page_limit,
        )
        if not event_ids:
            break
        events = lookup_events_by_ids(group.ledger_path, event_ids)
        ordered = [event for event in events if isinstance(event, dict)]
        for event in ordered:
            yield event
        remaining -= len(ordered)
        if not has_more:
            break
        before_id = str(event_ids[0] or "").strip()
        if not before_id:
            break


def _iter_recent_context_sync_events(
    group: Group,
    *,
    limit: int = _RECENT_CONTEXT_SYNC_LIMIT,
) -> Iterable[Dict[str, Any]]:
    remaining = max(1, int(limit or _RECENT_CONTEXT_SYNC_LIMIT))
    before_id = ""
    while remaining > 0:
        page_limit = min(_RECENT_PAGE_SIZE, remaining)
        event_ids, has_more = search_event_ids_indexed(
            group.ledger_path,
            allowed_kinds={"context.sync"},
            before_id=before_id,
            limit=page_limit,
        )
        if not event_ids:
            break
        events = lookup_events_by_ids(group.ledger_path, event_ids)
        ordered = [event for event in events if isinstance(event, dict)]
        for event in ordered:
            yield event
        remaining -= len(ordered)
        if not has_more:
            break
        before_id = str(event_ids[0] or "").strip()
        if not before_id:
            break


def _safe_seconds(start_iso: str, end_iso: str) -> Optional[float]:
    start_dt = parse_utc_iso(str(start_iso or "").strip()) if str(start_iso or "").strip() else None
    end_dt = parse_utc_iso(str(end_iso or "").strip()) if str(end_iso or "").strip() else None
    if start_dt is None or end_dt is None:
        return None
    delta = (end_dt - start_dt).total_seconds()
    if delta < 0:
        return None
    return float(delta)


def _median_int(values: List[float]) -> int:
    if not values:
        return 0
    return int(round(float(median(values))))


def _is_within_window(ts: str, *, window_seconds: int = _TREND_WINDOW_SECONDS) -> bool:
    event_dt = parse_utc_iso(str(ts or "").strip()) if str(ts or "").strip() else None
    now_dt = parse_utc_iso(utc_now_iso())
    if event_dt is None or now_dt is None:
        return False
    delta = (now_dt - event_dt).total_seconds()
    return 0 <= delta <= int(window_seconds)


def _build_reply_pressure_signal(
    group: Group,
    chat_events: List[Dict[str, Any]],
    *,
    include_obligation_status: bool = True,
) -> Dict[str, Any]:
    relevant = []
    for event in chat_events:
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if not bool(data.get("reply_required") is True):
            continue
        relevant.append(event)

    status_by_message = get_obligation_status_batch(group, relevant) if include_obligation_status else {}
    local_replies_by_message: Dict[str, str] = {}
    if not include_obligation_status:
        for event in chat_events:
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            reply_to = str(data.get("reply_to") or "").strip()
            if reply_to:
                local_replies_by_message.setdefault(reply_to, str(event.get("ts") or "").strip())
    now_iso = utc_now_iso()
    pending_ages: List[float] = []
    reply_latencies: List[float] = []
    pending_count = 0
    overdue_count = 0
    baseline_median_reply_seconds = 0

    for event in relevant:
        event_id = str(event.get("id") or "").strip()
        if not event_id:
            continue
        if not include_obligation_status:
            reply_ts = local_replies_by_message.get(event_id, "")
            if reply_ts:
                latency_seconds = _safe_seconds(str(event.get("ts") or ""), reply_ts)
                if latency_seconds is not None:
                    reply_latencies.append(latency_seconds)
            else:
                pending_count += 1
                age_seconds = _safe_seconds(str(event.get("ts") or ""), now_iso)
                if age_seconds is not None:
                    pending_ages.append(age_seconds)
            continue
        status = status_by_message.get(event_id) if isinstance(status_by_message.get(event_id), dict) else {}
        pending_for_any = False
        replied_for_any = False
        for recipient_status in status.values():
            if not isinstance(recipient_status, dict):
                continue
            if bool(recipient_status.get("replied")):
                replied_for_any = True
            elif bool(recipient_status.get("reply_required")):
                pending_for_any = True
        if pending_for_any:
            pending_count += 1
            age_seconds = _safe_seconds(str(event.get("ts") or ""), now_iso)
            if age_seconds is not None:
                pending_ages.append(age_seconds)
        if replied_for_any:
            reply_to_id = event_id
            for candidate in chat_events:
                candidate_data = candidate.get("data") if isinstance(candidate.get("data"), dict) else {}
                if str(candidate_data.get("reply_to") or "").strip() != reply_to_id:
                    continue
                latency_seconds = _safe_seconds(str(event.get("ts") or ""), str(candidate.get("ts") or ""))
                if latency_seconds is not None:
                    reply_latencies.append(latency_seconds)
                    break

    baseline_median_reply_seconds = _median_int(reply_latencies)
    oldest_pending_seconds = int(max(pending_ages) if pending_ages else 0)
    if baseline_median_reply_seconds > 0:
        overdue_count = sum(1 for age in pending_ages if age >= baseline_median_reply_seconds * 2)
    else:
        overdue_count = sum(1 for age in pending_ages if age >= 600)

    severity = "low"
    if overdue_count > 0 or oldest_pending_seconds >= 1200:
        severity = "high"
    elif pending_count >= 2 or oldest_pending_seconds >= 600:
        severity = "medium"

    return {
        "kind": "reply_pressure",
        "severity": severity,
        "pending_count": pending_count,
        "overdue_count": overdue_count,
        "oldest_pending_seconds": oldest_pending_seconds,
        "baseline_median_reply_seconds": baseline_median_reply_seconds,
    }


def _build_coordination_rhythm_signal(group: Group, chat_events: List[Dict[str, Any]]) -> Dict[str, Any]:
    foreman = find_foreman(group)
    foreman_id = str(foreman.get("id") or "").strip() if isinstance(foreman, dict) else ""
    foreman_events = [
        event for event in chat_events
        if str(event.get("by") or "").strip() == foreman_id
    ]
    latest_foreman_ts = str(foreman_events[0].get("ts") or "").strip() if foreman_events else ""
    silence_seconds = _safe_seconds(latest_foreman_ts, utc_now_iso()) if latest_foreman_ts else None
    gaps: List[float] = []
    for idx in range(len(foreman_events) - 1):
        newer_ts = str(foreman_events[idx].get("ts") or "").strip()
        older_ts = str(foreman_events[idx + 1].get("ts") or "").strip()
        gap = _safe_seconds(older_ts, newer_ts)
        if gap is not None:
            gaps.append(gap)
    baseline_median_gap_seconds = _median_int(gaps[:6])
    silence_value = int(silence_seconds or 0)
    severity = "low"
    if baseline_median_gap_seconds > 0 and silence_value >= max(900, baseline_median_gap_seconds * 3):
        severity = "high"
    elif silence_value >= max(420, baseline_median_gap_seconds * 2 if baseline_median_gap_seconds > 0 else 420):
        severity = "medium"
    return {
        "kind": "coordination_rhythm",
        "severity": severity,
        "foreman_id": foreman_id,
        "silence_seconds": silence_value,
        "baseline_median_gap_seconds": baseline_median_gap_seconds,
    }


def _build_task_pressure_signal(context_payload: Dict[str, Any]) -> Dict[str, Any]:
    attention = context_payload.get("attention") if isinstance(context_payload.get("attention"), dict) else {}
    blocked_tasks = (
        attention.get("blocked")
        if isinstance(attention.get("blocked"), list)
        else context_payload.get("blocked_tasks")
        if isinstance(context_payload.get("blocked_tasks"), list)
        else []
    )
    waiting_user_tasks = (
        attention.get("waiting_user")
        if isinstance(attention.get("waiting_user"), list)
        else context_payload.get("waiting_user_tasks")
        if isinstance(context_payload.get("waiting_user_tasks"), list)
        else []
    )
    handoff_tasks = (
        attention.get("pending_handoffs")
        if isinstance(attention.get("pending_handoffs"), list)
        else context_payload.get("handoff_tasks")
        if isinstance(context_payload.get("handoff_tasks"), list)
        else []
    )
    planned_backlog_tasks = context_payload.get("planned_backlog_tasks") if isinstance(context_payload.get("planned_backlog_tasks"), list) else []
    recent_blocked_updates = sum(
        1 for item in blocked_tasks
        if isinstance(item, dict) and _is_within_window(str(item.get("updated_at") or item.get("created_at") or ""))
    )
    recent_waiting_user_updates = sum(
        1 for item in waiting_user_tasks
        if isinstance(item, dict) and _is_within_window(str(item.get("updated_at") or item.get("created_at") or ""))
    )
    recent_handoff_updates = sum(
        1 for item in handoff_tasks
        if isinstance(item, dict) and _is_within_window(str(item.get("updated_at") or item.get("created_at") or ""))
    )
    pressure_score = (
        len(waiting_user_tasks) * 5
        + len(blocked_tasks) * 3
        + len(handoff_tasks) * 2
        + len(planned_backlog_tasks)
    )
    trend_score = recent_waiting_user_updates * 3 + recent_blocked_updates * 2 + recent_handoff_updates
    severity = "low"
    if pressure_score >= 10 or trend_score >= 6 or len(waiting_user_tasks) >= 2:
        severity = "high"
    elif pressure_score >= 5 or trend_score >= 3:
        severity = "medium"
    return {
        "kind": "task_pressure",
        "severity": severity,
        "score": pressure_score,
        "trend_score": trend_score,
        "blocked_count": len(blocked_tasks),
        "waiting_user_count": len(waiting_user_tasks),
        "handoff_count": len(handoff_tasks),
        "planned_backlog_count": len(planned_backlog_tasks),
        "recent_blocked_updates": recent_blocked_updates,
        "recent_waiting_user_updates": recent_waiting_user_updates,
        "recent_handoff_updates": recent_handoff_updates,
    }


def _collect_recent_task_op_counts(group: Group, *, limit: int = _RECENT_CONTEXT_SYNC_LIMIT) -> Dict[str, int]:
    counts = {
        "task_create": 0,
        "task_update": 0,
        "task_move": 0,
        "task_restore": 0,
        "task_delete": 0,
        "task_changes": 0,
        "context_sync_events": 0,
    }
    if int(limit or 0) <= 0:
        return counts

    for event in _iter_recent_context_sync_events(group, limit=limit):
        if not _is_within_window(str(event.get("ts") or "")):
            continue
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        changes = data.get("changes") if isinstance(data.get("changes"), list) else []
        task_change_seen = False
        for change in changes:
            if not isinstance(change, dict):
                continue
            op_name = str(change.get("op") or "").strip().lower()
            if op_name == "task.create":
                counts["task_create"] += 1
                task_change_seen = True
            elif op_name == "task.update":
                counts["task_update"] += 1
                task_change_seen = True
            elif op_name == "task.move":
                counts["task_move"] += 1
                task_change_seen = True
            elif op_name == "task.restore":
                counts["task_restore"] += 1
                task_change_seen = True
            elif op_name == "task.delete":
                counts["task_delete"] += 1
                task_change_seen = True
        if task_change_seen:
            counts["context_sync_events"] += 1
    counts["task_changes"] = (
        counts["task_create"]
        + counts["task_update"]
        + counts["task_move"]
        + counts["task_restore"]
        + counts["task_delete"]
    )
    return counts


def _merge_task_pressure_with_ledger_trend(
    task_signal: Dict[str, Any],
    *,
    recent_task_ops: Dict[str, int],
) -> Dict[str, Any]:
    merged = dict(task_signal)
    task_create = int(recent_task_ops.get("task_create") or 0)
    task_update = int(recent_task_ops.get("task_update") or 0)
    task_move = int(recent_task_ops.get("task_move") or 0)
    task_restore = int(recent_task_ops.get("task_restore") or 0)
    task_delete = int(recent_task_ops.get("task_delete") or 0)
    task_changes = int(recent_task_ops.get("task_changes") or 0)
    context_sync_events = int(recent_task_ops.get("context_sync_events") or 0)
    ledger_trend_score = task_create + task_move * 2 + task_update + task_restore + task_delete
    trend_score = int(merged.get("trend_score") or 0) + ledger_trend_score
    score = int(merged.get("score") or 0) + min(6, task_changes)
    severity = str(merged.get("severity") or "low")
    if score >= 12 or trend_score >= 8 or context_sync_events >= 3:
        severity = "high"
    elif severity == "low" and (score >= 6 or trend_score >= 4 or context_sync_events >= 2):
        severity = "medium"
    merged.update(
        {
            "severity": severity,
            "score": score,
            "trend_score": trend_score,
            "recent_task_create_ops": task_create,
            "recent_task_update_ops": task_update,
            "recent_task_move_ops": task_move,
            "recent_task_restore_ops": task_restore,
            "recent_task_delete_ops": task_delete,
            "recent_task_change_count": task_changes,
            "recent_task_context_sync_events": context_sync_events,
            "ledger_trend_score": ledger_trend_score,
        }
    )
    return merged


def _build_proposal_ready_signal(
    *,
    reply_pressure: Dict[str, Any],
    coordination_rhythm: Dict[str, Any],
    task_pressure: Dict[str, Any],
) -> Dict[str, Any]:
    pending_count = int(reply_pressure.get("pending_count") or 0)
    overdue_count = int(reply_pressure.get("overdue_count") or 0)
    oldest_pending_seconds = int(reply_pressure.get("oldest_pending_seconds") or 0)
    waiting_user_count = int(task_pressure.get("waiting_user_count") or 0)
    blocked_count = int(task_pressure.get("blocked_count") or 0)
    handoff_count = int(task_pressure.get("handoff_count") or 0)
    recent_task_change_count = int(task_pressure.get("recent_task_change_count") or 0)
    foreman_silence_seconds = int(coordination_rhythm.get("silence_seconds") or 0)

    ready = False
    focus = "none"
    severity = "low"
    summary = "No high-signal proposal is ready right now."

    if overdue_count > 0 or oldest_pending_seconds >= 900:
        ready = True
        focus = "reply_pressure"
        severity = "high"
        summary = "Reply loop is overdue; prioritize one follow-up action that closes the waiting thread."
    elif waiting_user_count > 0:
        ready = True
        focus = "waiting_user"
        severity = "high" if waiting_user_count >= 2 else "medium"
        summary = "User-facing task is waiting; prefer one task proposal that helps foreman close the user dependency."
    elif handoff_count > 0:
        ready = True
        focus = "handoff"
        severity = "medium"
        summary = "Pending handoff needs closure; prefer one task proposal that clarifies ownership."
    elif blocked_count > 0 and recent_task_change_count == 0:
        ready = True
        focus = "blocked"
        severity = "medium"
        summary = "Blocked work is not moving; prefer one task proposal that asks foreman to unblock the path."
    elif str(coordination_rhythm.get("severity") or "low") == "high" and pending_count == 0:
        ready = True
        focus = "coordination_rhythm"
        severity = "medium"
        summary = "Coordination rhythm is unusually quiet; emit at most one nudge only if it clearly advances the next step."

    return {
        "kind": "proposal_ready",
        "ready": ready,
        "focus": focus,
        "severity": severity,
        "summary": summary,
        "pending_reply_count": pending_count,
        "overdue_reply_count": overdue_count,
        "waiting_user_count": waiting_user_count,
        "blocked_count": blocked_count,
        "handoff_count": handoff_count,
        "recent_task_change_count": recent_task_change_count,
        "foreman_silence_seconds": foreman_silence_seconds,
    }


def load_pet_signals(
    group: Group,
    *,
    context_payload: Optional[Dict[str, Any]] = None,
    recent_chat_limit: int = _RECENT_CHAT_LIMIT,
    recent_chat_source: str = "indexed",
    context_sync_limit: int = _RECENT_CONTEXT_SYNC_LIMIT,
    include_reply_obligation_status: bool = True,
) -> Dict[str, Any]:
    effective_context = context_payload if isinstance(context_payload, dict) else {}
    chat_events = list(_iter_recent_chat_events(group, limit=recent_chat_limit, source=recent_chat_source))
    reply_pressure = _build_reply_pressure_signal(
        group,
        chat_events,
        include_obligation_status=include_reply_obligation_status,
    )
    coordination_rhythm = _build_coordination_rhythm_signal(group, chat_events)
    recent_task_ops = _collect_recent_task_op_counts(group, limit=context_sync_limit)
    task_pressure = _merge_task_pressure_with_ledger_trend(
        _build_task_pressure_signal(effective_context),
        recent_task_ops=recent_task_ops,
    )
    proposal_ready = _build_proposal_ready_signal(
        reply_pressure=reply_pressure,
        coordination_rhythm=coordination_rhythm,
        task_pressure=task_pressure,
    )
    signals = [reply_pressure, coordination_rhythm, task_pressure, proposal_ready]
    return {
        "signals": signals,
        "reply_pressure": reply_pressure,
        "coordination_rhythm": coordination_rhythm,
        "task_pressure": task_pressure,
        "proposal_ready": proposal_ready,
    }


def build_pet_signal_summary_lines(payload: Dict[str, Any]) -> List[str]:
    lines: List[str] = []
    reply_pressure = payload.get("reply_pressure") if isinstance(payload.get("reply_pressure"), dict) else {}
    if reply_pressure:
        lines.append(
            "Reply Pressure: severity={severity}, pending={pending}, overdue={overdue}, oldest={oldest}s, baseline={baseline}s".format(
                severity=str(reply_pressure.get("severity") or "low"),
                pending=int(reply_pressure.get("pending_count") or 0),
                overdue=int(reply_pressure.get("overdue_count") or 0),
                oldest=int(reply_pressure.get("oldest_pending_seconds") or 0),
                baseline=int(reply_pressure.get("baseline_median_reply_seconds") or 0),
            )
        )
    rhythm = payload.get("coordination_rhythm") if isinstance(payload.get("coordination_rhythm"), dict) else {}
    if rhythm:
        lines.append(
            "Coordination Rhythm: severity={severity}, foreman_silence={silence}s, baseline_gap={baseline}s".format(
                severity=str(rhythm.get("severity") or "low"),
                silence=int(rhythm.get("silence_seconds") or 0),
                baseline=int(rhythm.get("baseline_median_gap_seconds") or 0),
            )
        )
    task_pressure = payload.get("task_pressure") if isinstance(payload.get("task_pressure"), dict) else {}
    if task_pressure:
        lines.append(
            "Task Pressure: severity={severity}, score={score}, trend={trend}, waiting_user={waiting_user}, blocked={blocked}, handoff={handoff}, ledger_ops={ledger_ops}".format(
                severity=str(task_pressure.get("severity") or "low"),
                score=int(task_pressure.get("score") or 0),
                trend=int(task_pressure.get("trend_score") or 0),
                waiting_user=int(task_pressure.get("waiting_user_count") or 0),
                blocked=int(task_pressure.get("blocked_count") or 0),
                handoff=int(task_pressure.get("handoff_count") or 0),
                ledger_ops=int(task_pressure.get("recent_task_change_count") or 0),
            )
        )
    proposal_ready = payload.get("proposal_ready") if isinstance(payload.get("proposal_ready"), dict) else {}
    if proposal_ready:
        lines.append(
            "Proposal Ready: ready={ready}, focus={focus}, severity={severity}, summary={summary}".format(
                ready="yes" if bool(proposal_ready.get("ready")) else "no",
                focus=str(proposal_ready.get("focus") or "none"),
                severity=str(proposal_ready.get("severity") or "low"),
                summary=str(proposal_ready.get("summary") or "").strip(),
            )
        )
    return lines
