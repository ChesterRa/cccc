"""
Context operations for daemon (v2).

All context operations go through the daemon to preserve the single-writer invariant.

v2 ops:
- vision.update
- overview.manual.update
- task.create / task.update / task.status / task.move / task.restore
- agent.update / agent.clear
"""

from __future__ import annotations

import logging
from enum import Enum
from typing import Any, Dict, List, Optional

from ...contracts.v1 import DaemonResponse, DaemonError
from ..space.group_space_projection import sync_group_space_projection
from ..space.group_space_store import enqueue_space_job, get_space_binding, get_space_provider_state
from ...kernel.group import load_group
from ...kernel.context import (
    ContextStorage,
    Context,
    Overview,
    OverviewManual,
    Task,
    TaskStatus,
    Step,
    StepStatus,
    AgentState,
    _utc_now_iso,
)
from ...kernel.ledger import append_event
from ...util.conv import coerce_bool

_CURATED_SPACE_SYNC_PREFIXES = (
    "vision.",
    "overview.",
    "task.",
)

logger = logging.getLogger(__name__)


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _normalize_status_token(value: str) -> str:
    s = str(value or "").strip().lower()
    if s == "pending":
        return "planned"
    return s


def _parse_task_status(value: Any) -> TaskStatus:
    s = _normalize_status_token(str(value or "planned"))
    try:
        return TaskStatus(s)
    except ValueError as e:
        raise ValueError(f"Invalid task status: {value}") from e


def _status_value(value: Any) -> str:
    if isinstance(value, Enum):
        return value.value
    return str(value or "").strip().lower()


def _get_storage(group_id: str) -> Optional[ContextStorage]:
    """Get context storage for a group."""
    group = load_group(group_id)
    if group is None:
        return None
    return ContextStorage(group)


def _task_to_dict(task: Task) -> Dict[str, Any]:
    """Convert Task to dict (v2: parent_id instead of milestone)."""
    current_step = task.current_step
    return {
        "id": task.id,
        "name": task.name,
        "goal": task.goal,
        "parent_id": task.parent_id,
        "status": task.status.value if isinstance(task.status, TaskStatus) else task.status,
        "assignee": task.assignee,
        "created_at": task.created_at,
        "updated_at": task.updated_at,
        "steps": [
            {
                "id": s.id,
                "name": s.name,
                "acceptance": s.acceptance,
                "status": s.status.value if isinstance(s.status, StepStatus) else s.status,
            }
            for s in task.steps
        ],
        "current_step": current_step.id if current_step else None,
        "progress": task.progress,
    }


def _should_trigger_group_space_context_sync(changes: List[Dict[str, Any]]) -> bool:
    for item in changes:
        if not isinstance(item, dict):
            continue
        op_name = str(item.get("op") or "").strip()
        if not op_name:
            continue
        if any(op_name.startswith(prefix) for prefix in _CURATED_SPACE_SYNC_PREFIXES):
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

    tasks = [_task_to_dict(t) for t in sorted(tasks_by_id.values(), key=lambda x: str(x.id or ""))]
    compact_changes = [
        {
            "index": int(item.get("index") or 0),
            "op": str(item.get("op") or ""),
            "detail": str(item.get("detail") or ""),
        }
        for item in changes
        if isinstance(item, dict)
    ]

    # Build overview.manual for space sync
    manual = context.overview.manual if context.overview else OverviewManual()
    overview_manual = {}
    if manual.current_focus:
        overview_manual["current_focus"] = manual.current_focus
    if manual.roles:
        overview_manual["roles"] = manual.roles
    if manual.collaboration_mode:
        overview_manual["collaboration_mode"] = manual.collaboration_mode

    payload = {
        "group_id": group_id,
        "context_version": str(version or "").strip(),
        "synced_at": _utc_now_iso(),
        "summary": {
            "vision": context.vision,
            "overview_manual": overview_manual,
            "tasks": tasks,
        },
        "changes": compact_changes,
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


# =============================================================================
# Permission Helpers
# =============================================================================


def _check_permission(
    by: str, op_name: str, group_id: str,
    task: Optional[Task] = None,
    agent_id: Optional[str] = None,
) -> Optional[str]:
    """Check permission for an operation. Returns error message or None if allowed."""
    # user and system always allowed
    if by in ("user", "system"):
        return None

    # Resolve caller role
    try:
        from ...kernel.actors import get_effective_role
        group = load_group(group_id)
        if group is None:
            return None  # let the op fail naturally
        role = get_effective_role(group, by)
    except Exception:
        role = "peer"

    # foreman allowed everything
    if role == "foreman":
        return None

    # Peer restrictions
    if op_name in ("vision.update", "overview.manual.update", "task.move"):
        return f"Permission denied: {op_name} requires foreman or user"

    if op_name in ("task.restore",):
        return f"Permission denied: {op_name} requires foreman or user"

    if op_name in ("task.update", "task.status"):
        if task is not None and task.assignee and task.assignee != by:
            return f"Permission denied: {op_name} on {task.id} (assigned to {task.assignee}, caller is {by})"

    if op_name in ("agent.update", "agent.clear"):
        if agent_id and agent_id != by:
            return f"Permission denied: {op_name} for {agent_id} (caller is {by})"

    return None


# =============================================================================
# Context Get (v2)
# =============================================================================


def handle_context_get(args: Dict[str, Any]) -> DaemonResponse:
    """Return the full context for a group (v2 shape)."""
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    storage = _get_storage(group_id)
    if storage is None:
        return _error("group_not_found", f"group not found: {group_id}")

    context = storage.load_context()
    tasks = storage.list_tasks()
    presence = storage.load_presence()

    # Build tasks summary
    non_archived = [t for t in tasks if t.status != TaskStatus.ARCHIVED]
    done_count = sum(1 for t in non_archived if t.status == TaskStatus.DONE)
    active_count = sum(1 for t in non_archived if t.status == TaskStatus.ACTIVE)
    planned_count = sum(1 for t in non_archived if t.status == TaskStatus.PLANNED)
    root_count = sum(1 for t in non_archived if t.is_root)

    # Active tasks (non-archived)
    active_tasks = [_task_to_dict(t) for t in non_archived if t.status != TaskStatus.DONE]

    # Build overview.manual
    manual = context.overview.manual if context.overview else OverviewManual()
    overview_manual = {
        "roles": manual.roles,
        "collaboration_mode": manual.collaboration_mode,
        "current_focus": manual.current_focus,
        "updated_by": manual.updated_by,
        "updated_at": manual.updated_at,
    }

    # Compute overview.mermaid (daemon projection)
    overview_mermaid = storage.compute_overview_mermaid(
        tasks=tasks,
        presence=presence,
        overview=context.overview,
    )

    # Presence serialization (flat AgentState)
    agents_out = [
        {
            "id": a.id,
            "active_task_id": a.active_task_id,
            "focus": a.focus,
            "blockers": a.blockers,
            "next_action": a.next_action,
            "what_changed": a.what_changed,
            "decision_delta": a.decision_delta,
            "environment": a.environment,
            "user_profile": a.user_profile,
            "notes": a.notes,
            "updated_at": a.updated_at,
        }
        for a in presence.agents
    ]

    result = {
        "version": storage.compute_version(),
        "vision": context.vision,
        "overview": {
            "manual": overview_manual,
            "mermaid": overview_mermaid,
        },
        "tasks_summary": {
            "total": len(non_archived),
            "done": done_count,
            "active": active_count,
            "planned": planned_count,
            "root_count": root_count,
        },
        "active_tasks": active_tasks,
        "presence": {
            "agents": agents_out,
        },
    }

    return DaemonResponse(ok=True, result=result)


# =============================================================================
# Context Sync (batch operations, v2)
# =============================================================================


def handle_context_sync(args: Dict[str, Any]) -> DaemonResponse:
    """Apply a batch of context operations (v2)."""
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

    # CAS: check version before applying
    if if_version is not None:
        current_version = storage.compute_version()
        if str(if_version).strip() != current_version:
            return _error(
                "version_conflict",
                f"version conflict: expected {if_version}, current {current_version}",
                details={"expected": str(if_version), "current": current_version},
            )

    context = storage.load_context()
    presence = storage.load_presence()
    tasks_by_id: Dict[str, Task] = {t.id: t for t in storage.list_tasks()}

    changes: List[Dict[str, Any]] = []
    context_dirty = False
    presence_dirty = False
    dirty_task_ids: set = set()

    def _mark_change(idx: int, op_name: str, detail: str) -> None:
        changes.append({"index": idx, "op": op_name, "detail": detail})

    def _get_or_create_agent(agent_id: str) -> AgentState:
        canonical_id = storage._canonicalize_agent_id(agent_id)  # noqa: SLF001
        if not canonical_id:
            raise ValueError("agent_id must be a non-empty string")
        for a in presence.agents:
            if a.id == canonical_id:
                return a
        agent = AgentState(id=canonical_id)
        presence.agents.append(agent)
        return agent

    try:
        for idx, item in enumerate(ops):
            if not isinstance(item, dict):
                raise ValueError(f"op[{idx}] must be a dict")

            op_name = str(item.get("op") or "")

            # --- Vision ---
            if op_name == "vision.update":
                perm_err = _check_permission(by, op_name, group_id)
                if perm_err:
                    raise ValueError(perm_err)
                vision = str(item.get("vision") or "")
                context.vision = vision
                context_dirty = True
                _mark_change(idx, op_name, "Set vision")

            # --- Overview Manual ---
            elif op_name == "overview.manual.update":
                perm_err = _check_permission(by, op_name, group_id)
                if perm_err:
                    raise ValueError(perm_err)
                manual = context.overview.manual if context.overview else OverviewManual()
                if "roles" in item:
                    roles_raw = item["roles"]
                    manual.roles = list(roles_raw) if isinstance(roles_raw, list) else []
                if "collaboration_mode" in item:
                    manual.collaboration_mode = str(item["collaboration_mode"])
                if "current_focus" in item:
                    manual.current_focus = str(item["current_focus"])
                manual.updated_by = by
                manual.updated_at = _utc_now_iso()
                if context.overview is None:
                    context.overview = Overview(manual=manual)
                else:
                    context.overview.manual = manual
                context_dirty = True
                _mark_change(idx, op_name, "Updated overview.manual")

            # --- Task Create ---
            elif op_name == "task.create":
                name = str(item.get("name") or "")
                goal = str(item.get("goal") or "")
                parent_id = item.get("parent_id")
                assignee = item.get("assignee")
                steps_raw = item.get("steps") or []

                # Validate parent exists if specified
                if parent_id is not None:
                    parent_id = str(parent_id)
                    if parent_id not in tasks_by_id:
                        raise ValueError(f"Parent task not found: {parent_id}")

                # Account for in-flight tasks not yet on disk
                task_id = storage.generate_task_id()
                while task_id in tasks_by_id:
                    num = int(task_id[1:]) + 1
                    task_id = f"T{num:03d}"
                task_steps = []
                for i, s in enumerate(steps_raw, start=1):
                    if isinstance(s, dict):
                        task_steps.append(Step(
                            id=f"S{i}",
                            name=str(s.get("name") or ""),
                            acceptance=str(s.get("acceptance") or ""),
                            status=StepStatus.PENDING,
                        ))

                now = _utc_now_iso()
                task = Task(
                    id=task_id,
                    name=name,
                    goal=goal,
                    parent_id=parent_id,
                    status=TaskStatus.PLANNED,
                    assignee=str(assignee) if assignee else None,
                    created_at=now,
                    updated_at=now,
                    steps=task_steps,
                )
                tasks_by_id[task_id] = task
                dirty_task_ids.add(task_id)
                _mark_change(idx, op_name, f"Created {task_id}")

            # --- Task Update (metadata only, not status) ---
            elif op_name == "task.update":
                task_id = str(item.get("task_id") or "")
                task = tasks_by_id.get(task_id)
                if task is None:
                    raise ValueError(f"Task not found: {task_id}")

                perm_err = _check_permission(by, op_name, group_id, task=task)
                if perm_err:
                    raise ValueError(perm_err)

                if "name" in item:
                    task.name = str(item["name"])
                if "goal" in item:
                    task.goal = str(item["goal"])
                if "assignee" in item:
                    task.assignee = str(item["assignee"]) if item["assignee"] else None

                # Step update
                if "step_id" in item and "step_status" in item:
                    step_id = str(item["step_id"])
                    step_status_str = str(item["step_status"])
                    found = False
                    for step in task.steps:
                        if step.id == step_id:
                            try:
                                step.status = StepStatus(step_status_str)
                            except ValueError:
                                raise ValueError(f"Invalid step status: {step_status_str}")
                            found = True
                            break
                    if not found:
                        raise ValueError(f"Step not found: {step_id}")

                task.updated_at = _utc_now_iso()
                dirty_task_ids.add(task_id)
                _mark_change(idx, op_name, f"Updated {task_id}")

            # --- Task Status (separate from update for clarity) ---
            elif op_name == "task.status":
                task_id = str(item.get("task_id") or "")
                task = tasks_by_id.get(task_id)
                if task is None:
                    raise ValueError(f"Task not found: {task_id}")

                perm_err = _check_permission(by, op_name, group_id, task=task)
                if perm_err:
                    raise ValueError(perm_err)

                new_status = _parse_task_status(item.get("status"))
                prev_status_value = _normalize_status_token(_status_value(task.status))

                if new_status == TaskStatus.ARCHIVED:
                    if prev_status_value and prev_status_value != "archived":
                        task.archived_from = prev_status_value
                else:
                    task.archived_from = None

                task.status = new_status
                task.updated_at = _utc_now_iso()
                dirty_task_ids.add(task_id)
                _mark_change(idx, op_name, f"Status {task_id} -> {new_status.value}")

                # Auto-refresh short-term agent state from task lifecycle.
                # This keeps presence usable even when agents forget explicit updates.
                assignee_id = str(task.assignee or "").strip()
                if assignee_id:
                    agent = _get_or_create_agent(assignee_id)
                    auto_changed = False
                    status_label = new_status.value

                    if new_status == TaskStatus.ACTIVE:
                        if str(agent.active_task_id or "").strip() != task_id:
                            agent.active_task_id = task_id
                            auto_changed = True
                        focus_hint = str(task.name or "").strip()
                        if focus_hint and str(agent.focus or "").strip() != focus_hint:
                            agent.focus = focus_hint
                            auto_changed = True
                        changed_hint = f"{task_id} -> active"
                        if str(agent.what_changed or "").strip() != changed_hint:
                            agent.what_changed = changed_hint
                            auto_changed = True
                    elif new_status in {TaskStatus.DONE, TaskStatus.ARCHIVED}:
                        if str(agent.active_task_id or "").strip() == task_id:
                            agent.active_task_id = None
                            auto_changed = True
                        changed_hint = f"{task_id} -> {status_label}"
                        if str(agent.what_changed or "").strip() != changed_hint:
                            agent.what_changed = changed_hint
                            auto_changed = True
                        if str(agent.focus or "").strip() == str(task.name or "").strip():
                            agent.focus = ""
                            auto_changed = True

                    if auto_changed:
                        agent.updated_at = _utc_now_iso()
                        presence_dirty = True
                        _mark_change(
                            idx,
                            "agent.autosync",
                            f"Auto-synced agent {assignee_id} from {task_id} status={status_label}",
                        )

                # Memory hook: root task completion triggers solidify+export.
                # dry_run must be side-effect free.
                if (not dry_run) and new_status == TaskStatus.DONE and task.is_root:
                    try:
                        from ..memory.memory_ops import handle_memory_solidify_batch, handle_memory_export
                        solidify_result = handle_memory_solidify_batch({
                            "group_id": group_id,
                            "task_id": task_id,
                        })
                        export_result = handle_memory_export({
                            "group_id": group_id,
                        })
                        try:
                            from ..memory.memory_ops import _get_memory_store
                            _store = _get_memory_store(group_id)
                            if _store is not None:
                                import json as _json
                                _store.set_meta(
                                    f"root_task_hook:{task_id}",
                                    _json.dumps({
                                        "solidified": solidify_result.result.get("solidified", 0) if solidify_result.ok else 0,
                                        "exported": export_result.ok,
                                        "at": _utc_now_iso(),
                                    }),
                                )
                        except Exception:
                            logger.exception(
                                "memory_root_task_hook_meta_record_failed group_id=%s task_id=%s",
                                group_id, task_id,
                            )
                    except Exception:
                        logger.exception(
                            "memory_root_task_hook_failed group_id=%s task_id=%s",
                            group_id, task_id,
                        )

            # --- Task Move ---
            elif op_name == "task.move":
                task_id = str(item.get("task_id") or "")
                task = tasks_by_id.get(task_id)
                if task is None:
                    raise ValueError(f"Task not found: {task_id}")

                perm_err = _check_permission(by, op_name, group_id)
                if perm_err:
                    raise ValueError(perm_err)

                new_parent_id = item.get("new_parent_id")
                if new_parent_id is not None:
                    new_parent_id = str(new_parent_id)
                    if new_parent_id not in tasks_by_id:
                        raise ValueError(f"Target parent not found: {new_parent_id}")

                # Cycle detection
                all_tasks = list(tasks_by_id.values())
                if storage.detect_cycle(task_id, new_parent_id, tasks=all_tasks):
                    raise ValueError(f"Moving {task_id} under {new_parent_id} would create a cycle")

                task.parent_id = new_parent_id
                task.updated_at = _utc_now_iso()
                dirty_task_ids.add(task_id)
                _mark_change(idx, op_name, f"Moved {task_id} -> parent={new_parent_id}")

            # --- Task Restore ---
            elif op_name == "task.restore":
                task_id = str(item.get("task_id") or "")
                task = tasks_by_id.get(task_id)
                if task is None:
                    raise ValueError(f"Task not found: {task_id}")

                perm_err = _check_permission(by, op_name, group_id)
                if perm_err:
                    raise ValueError(perm_err)

                if task.status != TaskStatus.ARCHIVED:
                    raise ValueError(f"Task is not archived: {task_id}")

                target_raw = str(getattr(task, "archived_from", "") or "planned")
                target = _normalize_status_token(target_raw)
                if target == "archived":
                    target = "planned"
                new_status = _parse_task_status(target)

                task.status = new_status
                task.archived_from = None
                task.updated_at = _utc_now_iso()
                dirty_task_ids.add(task_id)
                _mark_change(idx, op_name, f"Restored {task_id} -> {new_status.value}")

            # --- Agent Update (flat short-term memory) ---
            elif op_name == "agent.update":
                agent_id = str(item.get("agent_id") or "")
                perm_err = _check_permission(by, op_name, group_id, agent_id=agent_id)
                if perm_err:
                    raise ValueError(perm_err)

                agent = _get_or_create_agent(agent_id)
                if "active_task_id" in item:
                    active_task_id = str(item.get("active_task_id") or "").strip()
                    agent.active_task_id = active_task_id or None
                if "focus" in item:
                    agent.focus = str(item.get("focus") or "")
                if "blockers" in item:
                    raw_blockers = item.get("blockers")
                    agent.blockers = list(raw_blockers) if isinstance(raw_blockers, list) else []
                if "next_action" in item:
                    agent.next_action = str(item.get("next_action") or "")
                if "what_changed" in item:
                    agent.what_changed = str(item.get("what_changed") or "")
                if "decision_delta" in item:
                    agent.decision_delta = str(item.get("decision_delta") or "")
                if "environment" in item:
                    agent.environment = str(item.get("environment") or "")
                if "user_profile" in item:
                    agent.user_profile = str(item.get("user_profile") or "")
                if "notes" in item:
                    agent.notes = str(item.get("notes") or "")
                agent.updated_at = _utc_now_iso()
                presence_dirty = True
                _mark_change(idx, op_name, f"Updated agent {agent_id}")

            # --- Agent Clear ---
            elif op_name == "agent.clear":
                agent_id = str(item.get("agent_id") or "")
                perm_err = _check_permission(by, op_name, group_id, agent_id=agent_id)
                if perm_err:
                    raise ValueError(perm_err)

                agent = _get_or_create_agent(agent_id)
                agent.active_task_id = None
                agent.focus = ""
                agent.blockers = []
                agent.next_action = ""
                agent.what_changed = ""
                agent.decision_delta = ""
                agent.environment = ""
                agent.user_profile = ""
                agent.notes = ""
                agent.updated_at = _utc_now_iso()
                presence_dirty = True
                _mark_change(idx, op_name, f"Cleared agent {agent_id}")

            else:
                raise ValueError(f"Unknown operation: {op_name}")

        if not dry_run:
            if context_dirty:
                storage.save_context(context)
            for task_id in sorted(dirty_task_ids):
                if task_id in tasks_by_id:
                    storage.save_task(tasks_by_id[task_id])
            if presence_dirty:
                storage.save_presence(presence)

        version = storage.compute_version()

        # Emit a lightweight ledger signal so UIs can refresh context state.
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
            except Exception as e:
                space_sync = {"queued": False, "reason": "enqueue_failed", "error": str(e)}

        result = {
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

    except ValueError as e:
        return _error("context_sync_error", str(e))
    except Exception as e:
        return _error("context_sync_error", f"unexpected error: {e}")


# =============================================================================
# Task List
# =============================================================================


def handle_task_list(args: Dict[str, Any]) -> DaemonResponse:
    """List tasks or get a single task."""
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
        # Include children info
        all_tasks = storage.list_tasks()
        children = storage.get_task_children(str(task_id), tasks=all_tasks)
        task_dict = _task_to_dict(task)
        task_dict["children"] = [_task_to_dict(c) for c in children]
        return DaemonResponse(ok=True, result={"task": task_dict})

    tasks = storage.list_tasks()
    return DaemonResponse(ok=True, result={"tasks": [_task_to_dict(t) for t in tasks]})


def try_handle_context_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "context_get":
        return handle_context_get(args)
    if op == "context_sync":
        return handle_context_sync(args)
    if op == "task_list":
        return handle_task_list(args)
    return None
