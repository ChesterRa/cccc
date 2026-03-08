"""
Context operations for daemon (v3).

All context operations go through the daemon to preserve the single-writer invariant.

v3 truths:
- coordination.brief / coordination recent notes
- task card lifecycle + fields
- per-actor agent_state hot/warm memory
"""

from __future__ import annotations

import json
import logging
from enum import Enum
from typing import Any, Dict, List, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.context import (
    AgentState,
    AgentStateHot,
    AgentStateWarm,
    AgentsData,
    ChecklistItem,
    ChecklistStatus,
    Context,
    ContextStorage,
    Coordination,
    CoordinationBrief,
    CoordinationNote,
    Task,
    TaskStatus,
    WaitingOn,
    _utc_now_iso,
)
from ...kernel.actors import get_effective_role
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...util.conv import coerce_bool
from ..space.group_space_projection import sync_group_space_projection
from ..space.group_space_store import enqueue_space_job, get_space_binding, get_space_provider_state

_CURATED_SPACE_SYNC_PREFIXES = (
    "coordination.",
    "task.",
)

logger = logging.getLogger(__name__)


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _status_value(value: Any) -> str:
    if isinstance(value, Enum):
        return str(value.value or "")
    return str(value or "").strip().lower()


def _parse_task_status(value: Any) -> TaskStatus:
    s = str(value or TaskStatus.PLANNED.value).strip().lower()
    try:
        return TaskStatus(s)
    except ValueError as exc:
        raise ValueError(f"Invalid task status: {value}") from exc


def _parse_waiting_on(value: Any) -> WaitingOn:
    s = str(value or WaitingOn.NONE.value).strip().lower()
    try:
        return WaitingOn(s)
    except ValueError as exc:
        raise ValueError(f"Invalid waiting_on value: {value}") from exc


def _parse_checklist_status(value: Any) -> ChecklistStatus:
    s = str(value or ChecklistStatus.PENDING.value).strip().lower()
    try:
        return ChecklistStatus(s)
    except ValueError as exc:
        raise ValueError(f"Invalid checklist status: {value}") from exc


def _normalize_text(value: Any, *, max_len: int = 4000) -> str:
    text = str(value or "").strip()
    if len(text) <= max_len:
        return text
    return text[:max_len].rstrip()


def _normalize_string_list(value: Any, *, max_items: int = 20, max_len: int = 240) -> List[str]:
    if not isinstance(value, list):
        return []
    out: List[str] = []
    for raw in value:
        if len(out) >= max_items:
            break
        text = _normalize_text(raw, max_len=max_len)
        if text:
            out.append(text)
    return out


def _normalize_checklist(value: Any) -> List[ChecklistItem]:
    if not isinstance(value, list):
        return []
    out: List[ChecklistItem] = []
    for idx, raw in enumerate(value):
        if not isinstance(raw, dict):
            continue
        item_id = _normalize_text(raw.get("id"), max_len=64) or f"C{idx + 1:03d}"
        text = _normalize_text(raw.get("text"), max_len=400)
        if not text:
            continue
        out.append(
            ChecklistItem(
                id=item_id,
                text=text,
                status=_parse_checklist_status(raw.get("status")),
            )
        )
    return out


def _get_storage(group_id: str) -> Optional[ContextStorage]:
    group = load_group(group_id)
    if group is None:
        return None
    return ContextStorage(group)


def _task_to_dict(task: Task) -> Dict[str, Any]:
    current_item = task.current_checklist_item
    return {
        "id": task.id,
        "title": task.title,
        "outcome": task.outcome,
        "parent_id": task.parent_id,
        "status": task.status.value if isinstance(task.status, TaskStatus) else str(task.status),
        "archived_from": task.archived_from,
        "assignee": task.assignee,
        "priority": task.priority,
        "blocked_by": list(task.blocked_by or []),
        "waiting_on": task.waiting_on.value if isinstance(task.waiting_on, WaitingOn) else str(task.waiting_on),
        "handoff_to": task.handoff_to,
        "notes": task.notes,
        "created_at": task.created_at,
        "updated_at": task.updated_at,
        "checklist": [
            {
                "id": item.id,
                "text": item.text,
                "status": item.status.value if isinstance(item.status, ChecklistStatus) else str(item.status),
            }
            for item in task.checklist
        ],
        "current_checklist_item": (
            {
                "id": current_item.id,
                "text": current_item.text,
                "status": current_item.status.value if isinstance(current_item.status, ChecklistStatus) else str(current_item.status),
            }
            if current_item is not None
            else None
        ),
        "progress": task.progress,
        "is_root": task.is_root,
    }


def _agent_state_to_dict(agent: AgentState) -> Dict[str, Any]:
    return {
        "id": agent.id,
        "hot": {
            "active_task_id": agent.hot.active_task_id,
            "focus": agent.hot.focus,
            "next_action": agent.hot.next_action,
            "blockers": list(agent.hot.blockers or []),
        },
        "warm": {
            "what_changed": agent.warm.what_changed,
            "open_loops": list(agent.warm.open_loops or []),
            "commitments": list(agent.warm.commitments or []),
            "environment_summary": agent.warm.environment_summary,
            "user_model": agent.warm.user_model,
            "persona_notes": agent.warm.persona_notes,
            "resume_hint": agent.warm.resume_hint,
        },
        "updated_at": agent.updated_at,
    }


def _note_to_dict(note: CoordinationNote) -> Dict[str, Any]:
    return {
        "at": note.at,
        "by": note.by,
        "summary": note.summary,
        "task_id": note.task_id,
    }


def _sort_tasks(tasks: List[Task]) -> List[Task]:
    def _key(task: Task) -> tuple[str, str, str]:
        updated = str(task.updated_at or "")
        created = str(task.created_at or "")
        return (updated or created, created, task.id)

    return sorted(tasks, key=_key, reverse=True)


def _board_projection(tasks: List[Task]) -> Dict[str, List[Dict[str, Any]]]:
    by_status: Dict[str, List[Dict[str, Any]]] = {
        TaskStatus.PLANNED.value: [],
        TaskStatus.ACTIVE.value: [],
        TaskStatus.DONE.value: [],
        TaskStatus.ARCHIVED.value: [],
    }
    for task in _sort_tasks(tasks):
        status = task.status.value if isinstance(task.status, TaskStatus) else str(task.status)
        by_status.setdefault(status, []).append(_task_to_dict(task))
    return {
        "planned": by_status.get(TaskStatus.PLANNED.value, []),
        "active": by_status.get(TaskStatus.ACTIVE.value, []),
        "done": by_status.get(TaskStatus.DONE.value, []),
        "archived": by_status.get(TaskStatus.ARCHIVED.value, []),
    }


def _attention_projection(tasks: List[Task]) -> Dict[str, List[Dict[str, Any]]]:
    blocked: List[Dict[str, Any]] = []
    waiting_user: List[Dict[str, Any]] = []
    pending_handoffs: List[Dict[str, Any]] = []
    for task in _sort_tasks(tasks):
        if task.status in {TaskStatus.DONE, TaskStatus.ARCHIVED}:
            continue
        task_dict = _task_to_dict(task)
        if task.waiting_on == WaitingOn.USER:
            waiting_user.append(task_dict)
        elif task.blocked_by or task.waiting_on in {WaitingOn.ACTOR, WaitingOn.EXTERNAL}:
            blocked.append(task_dict)
        if str(task.handoff_to or "").strip():
            pending_handoffs.append(task_dict)
    return {
        "blocked": blocked,
        "waiting_user": waiting_user,
        "pending_handoffs": pending_handoffs,
    }


def _tasks_summary(tasks: List[Task], *, attention: Optional[Dict[str, List[Dict[str, Any]]]] = None) -> Dict[str, Any]:
    att = attention or _attention_projection(tasks)
    planned = sum(1 for task in tasks if task.status == TaskStatus.PLANNED)
    active = sum(1 for task in tasks if task.status == TaskStatus.ACTIVE)
    done = sum(1 for task in tasks if task.status == TaskStatus.DONE)
    archived = sum(1 for task in tasks if task.status == TaskStatus.ARCHIVED)
    non_archived = len([task for task in tasks if task.status != TaskStatus.ARCHIVED])
    return {
        "total": non_archived,
        "planned": planned,
        "active": active,
        "done": done,
        "archived": archived,
        "blocked": len(att.get("blocked") or []),
        "waiting_user": len(att.get("waiting_user") or []),
        "pending_handoffs": len(att.get("pending_handoffs") or []),
        "root_count": sum(1 for task in tasks if task.is_root and task.status != TaskStatus.ARCHIVED),
    }


def _should_trigger_group_space_context_sync(changes: List[Dict[str, Any]]) -> bool:
    for item in changes:
        if not isinstance(item, dict):
            continue
        op_name = str(item.get("op") or "").strip()
        if op_name and any(op_name.startswith(prefix) for prefix in _CURATED_SPACE_SYNC_PREFIXES):
            return True
    return False


def _queue_group_space_context_sync(
    *,
    group_id: str,
    version: str,
    context: Context,
    tasks_by_id: Dict[str, Task],
    changes: List[Dict[str, Any]],
) -> Dict[str, Any]:
    binding = get_space_binding(group_id, provider="notebooklm")
    if not isinstance(binding, dict):
        return {"queued": False, "reason": "not_bound"}
    if str(binding.get("status") or "") != "bound":
        return {"queued": False, "reason": "binding_inactive"}
    remote_space_id = str(binding.get("remote_space_id") or "").strip()
    if not remote_space_id:
        return {"queued": False, "reason": "missing_remote_space_id"}

    provider_state = get_space_provider_state("notebooklm")
    if not bool(provider_state.get("enabled")) or str(provider_state.get("mode") or "") == "disabled":
        return {"queued": False, "reason": "provider_disabled"}

    brief = context.coordination.brief if isinstance(context.coordination, Coordination) else CoordinationBrief()
    payload = {
        "group_id": group_id,
        "context_version": str(version or "").strip(),
        "synced_at": _utc_now_iso(),
        "summary": {
            "coordination_brief": {
                "objective": brief.objective,
                "current_focus": brief.current_focus,
                "constraints": list(brief.constraints or []),
                "project_brief": brief.project_brief,
                "project_brief_stale": bool(brief.project_brief_stale),
            },
            "tasks": [_task_to_dict(task) for task in _sort_tasks(list(tasks_by_id.values()))],
            "recent_decisions": [_note_to_dict(note) for note in context.coordination.recent_decisions[:5]],
            "recent_handoffs": [_note_to_dict(note) for note in context.coordination.recent_handoffs[:5]],
        },
        "changes": [
            {
                "index": int(item.get("index") or 0),
                "op": str(item.get("op") or ""),
                "detail": str(item.get("detail") or ""),
            }
            for item in changes
            if isinstance(item, dict)
        ],
    }

    idem = f"context_sync:{group_id}:{version}"
    job, deduped = enqueue_space_job(
        group_id=group_id,
        provider="notebooklm",
        remote_space_id=remote_space_id,
        kind="context_sync",
        payload=payload,
        idempotency_key=idem,
    )
    return {
        "queued": True,
        "deduped": bool(deduped),
        "job_id": str(job.get("job_id") or ""),
        "provider": "notebooklm",
        "kind": "context_sync",
        "idempotency_key": idem,
    }


def _check_permission(
    by: str,
    op_name: str,
    group_id: str,
    *,
    task: Optional[Task] = None,
    target_actor_id: Optional[str] = None,
    create_assignee: Optional[str] = None,
) -> Optional[str]:
    if by == "system":
        return None

    group = load_group(group_id)
    if group is None:
        return None

    role = "user" if by == "user" else get_effective_role(group, by)
    if role in {"user", "foreman"}:
        if op_name in {"agent_state.update", "agent_state.clear"} and target_actor_id and by not in {"system", target_actor_id}:
            return f"Permission denied: {op_name} for {target_actor_id} (caller is {by})"
        return None

    if op_name == "coordination.brief.update":
        return "Permission denied: coordination brief updates require foreman or user"

    if op_name in {"agent_state.update", "agent_state.clear"}:
        if target_actor_id and target_actor_id != by:
            return f"Permission denied: {op_name} for {target_actor_id} (caller is {by})"
        return None

    if op_name == "task.create":
        assignee = str(create_assignee or "").strip()
        if assignee and assignee != by:
            return f"Permission denied: peer cannot create task assigned to {assignee}"
        return None

    if op_name == "meta.merge":
        return "Permission denied: meta.merge requires foreman or user"

    if op_name in {"task.update", "task.move", "task.restore"}:
        if task is None:
            return None
        assignee = str(task.assignee or "").strip()
        handoff_to = str(task.handoff_to or "").strip()
        if assignee and assignee != by and handoff_to != by:
            return f"Permission denied: {op_name} on {task.id} (assigned to {assignee}, caller is {by})"
        return None

    return None


def _get_or_create_agent(agents_state: AgentsData, agent_id: str) -> AgentState:
    canonical = str(agent_id or "").strip().replace("_", "-").lower()
    if not canonical:
        raise ValueError("actor_id must be non-empty")
    for agent in agents_state.agents:
        if agent.id == canonical:
            return agent
    created = AgentState(id=canonical)
    agents_state.agents.append(created)
    return created


def _record_note(notes: List[CoordinationNote], *, by: str, summary: str, task_id: Optional[str]) -> None:
    note = CoordinationNote(by=by, summary=_normalize_text(summary, max_len=400), task_id=(str(task_id or "").strip() or None))
    notes.insert(0, note)
    del notes[5:]


def _filter_agents_to_group(storage: ContextStorage, agents_state: AgentsData) -> AgentsData:
    actor_ids = {
        str(actor.get("id") or "").strip().lower()
        for actor in storage.group.doc.get("actors", [])
        if isinstance(actor, dict) and str(actor.get("id") or "").strip()
    }
    if not actor_ids:
        return agents_state
    return AgentsData(agents=[agent for agent in agents_state.agents if agent.id in actor_ids])


def handle_context_get(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    storage = _get_storage(group_id)
    if storage is None:
        return _error("group_not_found", f"group not found: {group_id}")

    context = storage.load_context()
    tasks = storage.list_tasks()
    agents_state = _filter_agents_to_group(storage, storage.load_agents())
    attention = _attention_projection(tasks)
    board = _board_projection(tasks)

    result = {
        "version": storage.compute_version(),
        "coordination": {
            "brief": {
                "objective": context.coordination.brief.objective,
                "current_focus": context.coordination.brief.current_focus,
                "constraints": list(context.coordination.brief.constraints or []),
                "project_brief": context.coordination.brief.project_brief,
                "project_brief_stale": bool(context.coordination.brief.project_brief_stale),
                "updated_by": context.coordination.brief.updated_by,
                "updated_at": context.coordination.brief.updated_at,
            },
            "tasks": [_task_to_dict(task) for task in _sort_tasks(tasks)],
            "recent_decisions": [_note_to_dict(note) for note in context.coordination.recent_decisions],
            "recent_handoffs": [_note_to_dict(note) for note in context.coordination.recent_handoffs],
        },
        "agent_states": [_agent_state_to_dict(agent) for agent in sorted(agents_state.agents, key=lambda x: x.id)],
        "attention": attention,
        "board": board,
        "tasks_summary": _tasks_summary(tasks, attention=attention),
        "panorama": {
            "mermaid": storage.compute_panorama_mermaid(
                tasks=tasks,
                agents_state=agents_state,
                coordination=context.coordination,
            )
        },
        "meta": context.meta if isinstance(context.meta, dict) else {},
    }
    return DaemonResponse(ok=True, result=result)


def handle_context_sync(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "system").strip() or "system"
    ops = args.get("ops") or []
    dry_run = coerce_bool(args.get("dry_run"), default=False)
    if_version = args.get("if_version")

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not isinstance(ops, list):
        return _error("invalid_ops", "ops must be a list")

    storage = _get_storage(group_id)
    if storage is None:
        return _error("group_not_found", f"group not found: {group_id}")

    current_version = storage.compute_version()
    if if_version is not None and str(if_version).strip() != current_version:
        return _error(
            "version_conflict",
            f"version conflict: expected {if_version}, current {current_version}",
            details={"expected": str(if_version), "current": current_version},
        )

    context = storage.load_context()
    tasks_by_id = {task.id: task for task in storage.list_tasks()}
    agents_state = _filter_agents_to_group(storage, storage.load_agents())

    context_dirty = False
    agents_dirty = False
    dirty_task_ids: List[str] = []
    changes: List[Dict[str, Any]] = []

    def _mark_change(index: int, op_name: str, detail: str) -> None:
        changes.append({"index": index, "op": op_name, "detail": detail})

    try:
        for idx, raw in enumerate(ops):
            if not isinstance(raw, dict):
                raise ValueError(f"op[{idx}] must be an object")
            op_name = str(raw.get("op") or "").strip()
            if not op_name:
                raise ValueError(f"op[{idx}] missing op")

            if op_name == "coordination.brief.update":
                perm_err = _check_permission(by, op_name, group_id)
                if perm_err:
                    raise ValueError(perm_err)
                brief = context.coordination.brief if isinstance(context.coordination, Coordination) else CoordinationBrief()
                updated = False
                for field in ("objective", "current_focus", "project_brief"):
                    if field in raw:
                        value = _normalize_text(raw.get(field), max_len=2000)
                        if getattr(brief, field) != value:
                            setattr(brief, field, value)
                            updated = True
                if "constraints" in raw:
                    constraints = _normalize_string_list(raw.get("constraints"), max_items=12, max_len=160)
                    if brief.constraints != constraints:
                        brief.constraints = constraints
                        updated = True
                if "project_brief_stale" in raw:
                    stale = bool(raw.get("project_brief_stale"))
                    if bool(brief.project_brief_stale) != stale:
                        brief.project_brief_stale = stale
                        updated = True
                if updated:
                    brief.updated_by = by
                    brief.updated_at = _utc_now_iso()
                    context.coordination.brief = brief
                    context_dirty = True
                    _mark_change(idx, op_name, "Updated coordination brief")
                continue

            if op_name == "coordination.note.add":
                note_kind = str(raw.get("kind") or "decision").strip().lower()
                if note_kind not in {"decision", "handoff"}:
                    raise ValueError(f"op[{idx}] coordination.note.add kind must be decision|handoff")
                summary = _normalize_text(raw.get("summary"), max_len=400)
                if not summary:
                    raise ValueError(f"op[{idx}] coordination.note.add summary is required")
                task_id = str(raw.get("task_id") or "").strip() or None
                target = context.coordination.recent_decisions if note_kind == "decision" else context.coordination.recent_handoffs
                _record_note(target, by=by, summary=summary, task_id=task_id)
                context_dirty = True
                _mark_change(idx, op_name, f"Added {note_kind} note")
                continue

            if op_name == "task.create":
                assignee = str(raw.get("assignee") or "").strip() or None
                perm_err = _check_permission(by, op_name, group_id, create_assignee=assignee)
                if perm_err:
                    raise ValueError(perm_err)
                title = _normalize_text(raw.get("title") if raw.get("title") is not None else raw.get("name"), max_len=240)
                if not title:
                    raise ValueError(f"op[{idx}] task.create title is required")
                status = _parse_task_status(raw.get("status")) if "status" in raw else TaskStatus.PLANNED
                task_id = storage.generate_task_id()
                parent_id = str(raw.get("parent_id") or "").strip() or None
                if parent_id and parent_id not in tasks_by_id:
                    raise ValueError(f"op[{idx}] parent task not found: {parent_id}")
                task = Task(
                    id=task_id,
                    title=title,
                    outcome=_normalize_text(raw.get("outcome") if raw.get("outcome") is not None else raw.get("goal"), max_len=400),
                    parent_id=parent_id,
                    status=status,
                    archived_from=None,
                    assignee=assignee,
                    priority=_normalize_text(raw.get("priority"), max_len=32),
                    blocked_by=_normalize_string_list(raw.get("blocked_by"), max_items=8, max_len=120),
                    waiting_on=_parse_waiting_on(raw.get("waiting_on")),
                    handoff_to=str(raw.get("handoff_to") or "").strip() or None,
                    notes=_normalize_text(raw.get("notes"), max_len=4000),
                    checklist=_normalize_checklist(raw.get("checklist")),
                )
                task.updated_at = _utc_now_iso()
                tasks_by_id[task.id] = task
                dirty_task_ids.append(task.id)
                _mark_change(idx, op_name, f"Created task {task.id}: {task.title}")
                continue

            if op_name == "task.update":
                task_id = str(raw.get("task_id") or "").strip()
                task = tasks_by_id.get(task_id)
                if task is None:
                    raise ValueError(f"Task not found: {task_id}")
                perm_err = _check_permission(by, op_name, group_id, task=task)
                if perm_err:
                    raise ValueError(perm_err)
                updated = False
                if "title" in raw:
                    value = _normalize_text(raw.get("title"), max_len=240)
                    if not value:
                        raise ValueError(f"op[{idx}] task.update title cannot be empty")
                    if task.title != value:
                        task.title = value
                        updated = True
                if "outcome" in raw:
                    value = _normalize_text(raw.get("outcome"), max_len=400)
                    if task.outcome != value:
                        task.outcome = value
                        updated = True
                if "assignee" in raw:
                    value = str(raw.get("assignee") or "").strip() or None
                    if by not in {"system", "user"} and value and value != by:
                        raise ValueError(f"Permission denied: peer cannot reassign task to {value}")
                    if task.assignee != value:
                        task.assignee = value
                        updated = True
                if "priority" in raw:
                    value = _normalize_text(raw.get("priority"), max_len=32)
                    if task.priority != value:
                        task.priority = value
                        updated = True
                if "parent_id" in raw:
                    parent_id = str(raw.get("parent_id") or "").strip() or None
                    if parent_id and parent_id not in tasks_by_id:
                        raise ValueError(f"op[{idx}] parent task not found: {parent_id}")
                    if storage.detect_cycle(task.id, parent_id, tasks=list(tasks_by_id.values())):
                        raise ValueError(f"op[{idx}] task.update would create parent cycle")
                    if task.parent_id != parent_id:
                        task.parent_id = parent_id
                        updated = True
                if "blocked_by" in raw:
                    value = _normalize_string_list(raw.get("blocked_by"), max_items=8, max_len=120)
                    if task.blocked_by != value:
                        task.blocked_by = value
                        updated = True
                if "waiting_on" in raw:
                    value = _parse_waiting_on(raw.get("waiting_on"))
                    if task.waiting_on != value:
                        task.waiting_on = value
                        updated = True
                if "handoff_to" in raw:
                    value = str(raw.get("handoff_to") or "").strip() or None
                    if task.handoff_to != value:
                        task.handoff_to = value
                        updated = True
                if "notes" in raw:
                    value = _normalize_text(raw.get("notes"), max_len=4000)
                    if task.notes != value:
                        task.notes = value
                        updated = True
                if "checklist" in raw:
                    value = _normalize_checklist(raw.get("checklist"))
                    current_payload = [(item.id, item.text, item.status.value) for item in task.checklist]
                    next_payload = [(item.id, item.text, item.status.value) for item in value]
                    if current_payload != next_payload:
                        task.checklist = value
                        updated = True
                if updated:
                    task.updated_at = _utc_now_iso()
                    if task.id not in dirty_task_ids:
                        dirty_task_ids.append(task.id)
                    _mark_change(idx, op_name, f"Updated task {task.id}")
                continue

            if op_name == "task.move":
                task_id = str(raw.get("task_id") or "").strip()
                task = tasks_by_id.get(task_id)
                if task is None:
                    raise ValueError(f"Task not found: {task_id}")
                perm_err = _check_permission(by, op_name, group_id, task=task)
                if perm_err:
                    raise ValueError(perm_err)
                new_status = _parse_task_status(raw.get("status"))
                prev_status = task.status
                if prev_status == new_status:
                    continue
                if new_status == TaskStatus.ARCHIVED:
                    task.archived_from = prev_status.value if isinstance(prev_status, TaskStatus) else str(prev_status)
                task.status = new_status
                if new_status != TaskStatus.ARCHIVED:
                    task.archived_from = None if new_status != TaskStatus.ARCHIVED else task.archived_from
                task.updated_at = _utc_now_iso()
                if task.id not in dirty_task_ids:
                    dirty_task_ids.append(task.id)
                _mark_change(idx, op_name, f"Moved task {task.id} to {new_status.value}")

                assignee_id = str(task.assignee or "").strip()
                if assignee_id:
                    agent = _get_or_create_agent(agents_state, assignee_id)
                    auto_changed = False
                    changed_hint = f"{task.id} -> {new_status.value}"
                    if new_status == TaskStatus.ACTIVE:
                        if agent.hot.active_task_id != task.id:
                            agent.hot.active_task_id = task.id
                            auto_changed = True
                        if task.title and agent.hot.focus != task.title:
                            agent.hot.focus = task.title
                            auto_changed = True
                        if agent.warm.what_changed != changed_hint:
                            agent.warm.what_changed = changed_hint
                            auto_changed = True
                    elif new_status in {TaskStatus.DONE, TaskStatus.ARCHIVED, TaskStatus.PLANNED}:
                        if agent.hot.active_task_id == task.id:
                            agent.hot.active_task_id = None
                            auto_changed = True
                        if agent.warm.what_changed != changed_hint:
                            agent.warm.what_changed = changed_hint
                            auto_changed = True
                        if agent.hot.focus == task.title and new_status != TaskStatus.ACTIVE:
                            agent.hot.focus = ""
                            auto_changed = True
                    if auto_changed:
                        agent.updated_at = _utc_now_iso()
                        agents_dirty = True
                        _mark_change(idx, "agent_state.autosync", f"Auto-synced agent {assignee_id} from {task.id}")

                if not dry_run:
                    try:
                        from ..memory.memory_ops import handle_memory_reme_write

                        lifecycle_note = (
                            f"Task status update: id={task.id}, title={task.title}, from={prev_status.value}, "
                            f"to={new_status.value}, by={by}, at={task.updated_at}"
                        )
                        handle_memory_reme_write(
                            {
                                "group_id": group_id,
                                "target": "daily",
                                "date": _utc_now_iso()[:10],
                                "mode": "append",
                                "content": lifecycle_note,
                                "idempotency_key": f"task_status:{task.id}:{prev_status.value}->{new_status.value}:{task.updated_at}",
                                "actor_id": by,
                                "tags": ["task_status", new_status.value],
                                "source_refs": [f"task:{task.id}"],
                            }
                        )
                    except Exception:
                        logger.exception("memory_task_status_hook_failed group_id=%s task_id=%s", group_id, task.id)

                    if new_status == TaskStatus.DONE and task.is_root:
                        try:
                            from ..memory.memory_ops import handle_memory_reme_write

                            promotion_note = (
                                f"Root task completed: id={task.id}, title={task.title}, outcome={task.outcome}, "
                                f"by={by}, at={task.updated_at}"
                            )
                            handle_memory_reme_write(
                                {
                                    "group_id": group_id,
                                    "target": "memory",
                                    "mode": "append",
                                    "content": promotion_note,
                                    "idempotency_key": f"root_task_done:{task.id}",
                                    "actor_id": by,
                                    "tags": ["root_task_done", "stable"],
                                    "source_refs": [f"task:{task.id}"],
                                }
                            )
                        except Exception:
                            logger.exception("memory_root_task_hook_failed group_id=%s task_id=%s", group_id, task.id)
                continue

            if op_name == "task.restore":
                task_id = str(raw.get("task_id") or "").strip()
                task = tasks_by_id.get(task_id)
                if task is None:
                    raise ValueError(f"Task not found: {task_id}")
                perm_err = _check_permission(by, op_name, group_id, task=task)
                if perm_err:
                    raise ValueError(perm_err)
                if task.status != TaskStatus.ARCHIVED:
                    continue
                restore_to = str(task.archived_from or TaskStatus.PLANNED.value).strip().lower() or TaskStatus.PLANNED.value
                try:
                    task.status = TaskStatus(restore_to)
                except ValueError:
                    task.status = TaskStatus.PLANNED
                task.archived_from = None
                task.updated_at = _utc_now_iso()
                if task.id not in dirty_task_ids:
                    dirty_task_ids.append(task.id)
                _mark_change(idx, op_name, f"Restored task {task.id}")
                continue

            if op_name == "agent_state.update":
                actor_id = str(raw.get("actor_id") or raw.get("agent_id") or "").strip().lower()
                if not actor_id:
                    raise ValueError(f"op[{idx}] agent_state.update actor_id is required")
                perm_err = _check_permission(by, op_name, group_id, target_actor_id=actor_id)
                if perm_err:
                    raise ValueError(perm_err)
                agent = _get_or_create_agent(agents_state, actor_id)
                updated = False
                hot = agent.hot if isinstance(agent.hot, AgentStateHot) else AgentStateHot()
                warm = agent.warm if isinstance(agent.warm, AgentStateWarm) else AgentStateWarm()
                if "active_task_id" in raw:
                    value = str(raw.get("active_task_id") or "").strip() or None
                    if hot.active_task_id != value:
                        hot.active_task_id = value
                        updated = True
                if "focus" in raw:
                    value = _normalize_text(raw.get("focus"), max_len=240)
                    if hot.focus != value:
                        hot.focus = value
                        updated = True
                if "next_action" in raw:
                    value = _normalize_text(raw.get("next_action"), max_len=400)
                    if hot.next_action != value:
                        hot.next_action = value
                        updated = True
                if "blockers" in raw:
                    value = _normalize_string_list(raw.get("blockers"), max_items=8, max_len=160)
                    if hot.blockers != value:
                        hot.blockers = value
                        updated = True
                if "what_changed" in raw:
                    value = _normalize_text(raw.get("what_changed"), max_len=500)
                    if warm.what_changed != value:
                        warm.what_changed = value
                        updated = True
                if "open_loops" in raw:
                    value = _normalize_string_list(raw.get("open_loops"), max_items=12, max_len=180)
                    if warm.open_loops != value:
                        warm.open_loops = value
                        updated = True
                if "commitments" in raw:
                    value = _normalize_string_list(raw.get("commitments"), max_items=12, max_len=180)
                    if warm.commitments != value:
                        warm.commitments = value
                        updated = True
                if "environment_summary" in raw or "environment" in raw:
                    source = raw.get("environment_summary") if "environment_summary" in raw else raw.get("environment")
                    value = _normalize_text(source, max_len=600)
                    if warm.environment_summary != value:
                        warm.environment_summary = value
                        updated = True
                if "user_model" in raw or "user_profile" in raw:
                    source = raw.get("user_model") if "user_model" in raw else raw.get("user_profile")
                    value = _normalize_text(source, max_len=600)
                    if warm.user_model != value:
                        warm.user_model = value
                        updated = True
                if "persona_notes" in raw or "notes" in raw:
                    source = raw.get("persona_notes") if "persona_notes" in raw else raw.get("notes")
                    value = _normalize_text(source, max_len=600)
                    if warm.persona_notes != value:
                        warm.persona_notes = value
                        updated = True
                if "resume_hint" in raw:
                    value = _normalize_text(raw.get("resume_hint"), max_len=400)
                    if warm.resume_hint != value:
                        warm.resume_hint = value
                        updated = True
                if updated:
                    agent.hot = hot
                    agent.warm = warm
                    agent.updated_at = _utc_now_iso()
                    agents_dirty = True
                    _mark_change(idx, op_name, f"Updated agent state {actor_id}")
                continue

            if op_name == "agent_state.clear":
                actor_id = str(raw.get("actor_id") or raw.get("agent_id") or "").strip().lower()
                if not actor_id:
                    raise ValueError(f"op[{idx}] agent_state.clear actor_id is required")
                perm_err = _check_permission(by, op_name, group_id, target_actor_id=actor_id)
                if perm_err:
                    raise ValueError(perm_err)
                agent = _get_or_create_agent(agents_state, actor_id)
                agent.hot = AgentStateHot()
                agent.warm = AgentStateWarm()
                agent.updated_at = _utc_now_iso()
                agents_dirty = True
                _mark_change(idx, op_name, f"Cleared agent state {actor_id}")
                continue

            if op_name == "meta.merge":
                perm_err = _check_permission(by, op_name, group_id)
                if perm_err:
                    raise ValueError(perm_err)
                data = raw.get("data")
                if not isinstance(data, dict):
                    raise ValueError(f"op[{idx}] meta.merge requires data dict")
                allowed = {"project_status"}
                bad_keys = set(data.keys()) - allowed
                if bad_keys:
                    raise ValueError(f"op[{idx}] meta.merge disallowed keys {sorted(bad_keys)}")
                if "project_status" in data:
                    project_status = data.get("project_status")
                    if project_status is not None:
                        if not isinstance(project_status, str):
                            raise ValueError(f"op[{idx}] meta.merge: project_status must be a string or null")
                        if len(project_status) > 100:
                            raise ValueError(f"op[{idx}] meta.merge: project_status exceeds 100 characters")
                if not isinstance(context.meta, dict):
                    context.meta = {}
                original = dict(context.meta)
                context.meta.update(data)
                meta_size = len(json.dumps(context.meta, ensure_ascii=False).encode())
                if meta_size > 100_000:
                    context.meta = original
                    raise ValueError(f"op[{idx}] meta.merge: resulting meta size {meta_size} bytes exceeds 100 KB limit")
                context_dirty = True
                _mark_change(idx, op_name, f"Merged meta keys: {', '.join(sorted(data.keys()))}")
                continue

            raise ValueError(f"Unknown operation: {op_name}")

        if not dry_run:
            if context_dirty:
                storage.save_context(context)
            for task_id in sorted(set(dirty_task_ids)):
                task = tasks_by_id.get(task_id)
                if task is not None:
                    storage.save_task(task)
            if agents_dirty:
                storage.save_agents(agents_state)

        version = storage.compute_version() if not dry_run else current_version

        if not dry_run and changes:
            try:
                append_event(
                    storage.group.ledger_path,
                    kind="context.sync",
                    group_id=group_id,
                    scope_key="",
                    by=by,
                    data={"version": version, "changes": changes},
                )
            except Exception:
                pass

        space_sync: Optional[Dict[str, Any]] = None
        if not dry_run and changes and _should_trigger_group_space_context_sync(changes):
            try:
                space_sync = _queue_group_space_context_sync(
                    group_id=group_id,
                    version=version,
                    context=context,
                    tasks_by_id=tasks_by_id,
                    changes=changes,
                )
            except Exception as exc:
                space_sync = {"queued": False, "reason": "enqueue_failed", "error": str(exc)}

        result: Dict[str, Any] = {
            "success": True,
            "dry_run": dry_run,
            "changes": changes,
            "version": version,
        }
        if isinstance(space_sync, dict):
            result["space_sync"] = space_sync
            if bool(space_sync.get("queued")):
                try:
                    _ = sync_group_space_projection(group_id, provider="notebooklm")
                except Exception:
                    pass
        return DaemonResponse(ok=True, result=result)

    except ValueError as exc:
        return _error("context_sync_error", str(exc))
    except Exception as exc:
        return _error("context_sync_error", f"unexpected error: {exc}")


def handle_task_list(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    task_id = args.get("task_id")
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    storage = _get_storage(group_id)
    if storage is None:
        return _error("group_not_found", f"group not found: {group_id}")

    if task_id:
        task = storage.load_task(str(task_id))
        if task is None:
            return _error("task_not_found", f"Task not found: {task_id}")
        all_tasks = storage.list_tasks()
        children = storage.get_task_children(str(task_id), tasks=all_tasks)
        payload = _task_to_dict(task)
        payload["children"] = [_task_to_dict(child) for child in _sort_tasks(children)]
        return DaemonResponse(ok=True, result={"task": payload})

    tasks = storage.list_tasks()
    return DaemonResponse(ok=True, result={"tasks": [_task_to_dict(task) for task in _sort_tasks(tasks)]})


def try_handle_context_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "context_get":
        return handle_context_get(args)
    if op == "context_sync":
        return handle_context_sync(args)
    if op == "task_list":
        return handle_task_list(args)
    return None
