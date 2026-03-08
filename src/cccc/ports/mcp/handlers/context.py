from __future__ import annotations

from typing import Any, Callable, Dict, List, Optional

from ..common import MCPError, _call_daemon_or_raise


def context_get(*, group_id: str, include_archived: bool = False) -> Dict[str, Any]:
    result = _call_daemon_or_raise({"op": "context_get", "args": {"group_id": group_id}})
    if include_archived:
        return result

    coordination = result.get("coordination") if isinstance(result.get("coordination"), dict) else {}
    tasks = coordination.get("tasks") if isinstance(coordination.get("tasks"), list) else []
    filtered_tasks = [
        item
        for item in tasks
        if isinstance(item, dict) and str(item.get("status") or "").strip().lower() != "archived"
    ]
    if coordination:
        coordination = dict(coordination)
        coordination["tasks"] = filtered_tasks
        result["coordination"] = coordination

    board = result.get("board") if isinstance(result.get("board"), dict) else {}
    if board:
        board = dict(board)
        board["archived"] = []
        result["board"] = board

    summary = result.get("tasks_summary") if isinstance(result.get("tasks_summary"), dict) else {}
    if summary:
        summary = dict(summary)
        summary["total"] = int(summary.get("planned") or 0) + int(summary.get("active") or 0) + int(summary.get("done") or 0)
        result["tasks_summary"] = summary
    return result


def context_sync(
    *,
    group_id: str,
    ops: List[Dict[str, Any]],
    dry_run: bool = False,
    if_version: Optional[str] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    args: Dict[str, Any] = {"group_id": group_id, "ops": ops, "dry_run": dry_run}
    if if_version is not None:
        args["if_version"] = if_version
    caller = str(by or "").strip()
    if caller:
        args["by"] = caller
    return _call_daemon_or_raise({"op": "context_sync", "args": args})


def coordination_get(*, group_id: str, include_archived: bool = False) -> Dict[str, Any]:
    snapshot = context_get(group_id=group_id, include_archived=include_archived)
    return {
        "version": snapshot.get("version"),
        "coordination": snapshot.get("coordination") if isinstance(snapshot.get("coordination"), dict) else {},
        "attention": snapshot.get("attention") if isinstance(snapshot.get("attention"), dict) else {},
        "board": snapshot.get("board") if isinstance(snapshot.get("board"), dict) else {},
        "tasks_summary": snapshot.get("tasks_summary") if isinstance(snapshot.get("tasks_summary"), dict) else {},
    }


def coordination_update_brief(
    *,
    group_id: str,
    objective: Optional[str] = None,
    current_focus: Optional[str] = None,
    constraints: Optional[List[str]] = None,
    project_brief: Optional[str] = None,
    project_brief_stale: Optional[bool] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "coordination.brief.update"}
    if objective is not None:
        op["objective"] = objective
    if current_focus is not None:
        op["current_focus"] = current_focus
    if constraints is not None:
        op["constraints"] = constraints
    if project_brief is not None:
        op["project_brief"] = project_brief
    if project_brief_stale is not None:
        op["project_brief_stale"] = bool(project_brief_stale)
    return context_sync(group_id=group_id, ops=[op], by=by)


def coordination_add_note(
    *,
    group_id: str,
    kind: str,
    summary: str,
    task_id: Optional[str] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "coordination.note.add", "kind": kind, "summary": summary}
    if task_id is not None:
        op["task_id"] = task_id
    return context_sync(group_id=group_id, ops=[op], by=by)


def task_list(*, group_id: str, task_id: Optional[str] = None, include_archived: bool = False) -> Dict[str, Any]:
    args: Dict[str, Any] = {"group_id": group_id}
    if task_id:
        args["task_id"] = task_id
    result = _call_daemon_or_raise({"op": "task_list", "args": args})
    if include_archived:
        return result
    if isinstance(result.get("task"), dict):
        task = result["task"]
        if str(task.get("status") or "").strip().lower() == "archived":
            raise MCPError(code="archived_hidden", message="archived task is hidden by default")
        return result
    tasks = result.get("tasks") if isinstance(result.get("tasks"), list) else []
    result["tasks"] = [
        item
        for item in tasks
        if isinstance(item, dict) and str(item.get("status") or "").strip().lower() != "archived"
    ]
    return result


def task_create(
    *,
    group_id: str,
    title: str,
    outcome: str = "",
    status: str = "planned",
    parent_id: Optional[str] = None,
    assignee: Optional[str] = None,
    priority: Optional[str] = None,
    blocked_by: Optional[List[str]] = None,
    waiting_on: Optional[str] = None,
    handoff_to: Optional[str] = None,
    notes: Optional[str] = None,
    checklist: Optional[List[Dict[str, Any]]] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        by=by,
        ops=[
            {
                "op": "task.create",
                "title": title,
                "outcome": outcome,
                "status": status,
                "parent_id": parent_id,
                "assignee": assignee,
                "priority": priority,
                "blocked_by": blocked_by or [],
                "waiting_on": waiting_on,
                "handoff_to": handoff_to,
                "notes": notes,
                "checklist": checklist or [],
            }
        ],
    )


def task_update(
    *,
    group_id: str,
    task_id: str,
    title: Optional[str] = None,
    outcome: Optional[str] = None,
    parent_id: Optional[str] = None,
    assignee: Optional[str] = None,
    priority: Optional[str] = None,
    blocked_by: Optional[List[str]] = None,
    waiting_on: Optional[str] = None,
    handoff_to: Optional[str] = None,
    notes: Optional[str] = None,
    checklist: Optional[List[Dict[str, Any]]] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "task.update", "task_id": task_id}
    if title is not None:
        op["title"] = title
    if outcome is not None:
        op["outcome"] = outcome
    if parent_id is not None:
        op["parent_id"] = parent_id
    if assignee is not None:
        op["assignee"] = assignee
    if priority is not None:
        op["priority"] = priority
    if blocked_by is not None:
        op["blocked_by"] = blocked_by
    if waiting_on is not None:
        op["waiting_on"] = waiting_on
    if handoff_to is not None:
        op["handoff_to"] = handoff_to
    if notes is not None:
        op["notes"] = notes
    if checklist is not None:
        op["checklist"] = checklist
    return context_sync(group_id=group_id, ops=[op], by=by)


def task_move(*, group_id: str, task_id: str, status: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        ops=[{"op": "task.move", "task_id": task_id, "status": status}],
        by=by,
    )


def task_restore(*, group_id: str, task_id: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "task.restore", "task_id": task_id}], by=by)


def agent_state_get(*, group_id: str, actor_id: Optional[str] = None, include_warm: bool = True) -> Dict[str, Any]:
    snapshot = context_get(group_id=group_id, include_archived=True)
    states = snapshot.get("agent_states") if isinstance(snapshot.get("agent_states"), list) else []
    if actor_id:
        target = str(actor_id or "").strip().lower()
        for item in states:
            if not isinstance(item, dict):
                continue
            if str(item.get("id") or "").strip().lower() != target:
                continue
            if include_warm:
                return {"agent_state": item, "version": snapshot.get("version")}
            return {
                "version": snapshot.get("version"),
                "agent_state": {
                    "id": item.get("id"),
                    "hot": item.get("hot") if isinstance(item.get("hot"), dict) else {},
                    "updated_at": item.get("updated_at"),
                },
            }
        return {"version": snapshot.get("version"), "agent_state": None}

    if not include_warm:
        return {
            "version": snapshot.get("version"),
            "agent_states": [
                {
                    "id": item.get("id"),
                    "hot": item.get("hot") if isinstance(item.get("hot"), dict) else {},
                    "updated_at": item.get("updated_at"),
                }
                for item in states
                if isinstance(item, dict)
            ],
        }
    return {"version": snapshot.get("version"), "agent_states": states}


def agent_state_update(
    *,
    group_id: str,
    actor_id: str,
    active_task_id: Optional[str] = None,
    focus: Optional[str] = None,
    blockers: Optional[List[str]] = None,
    next_action: Optional[str] = None,
    what_changed: Optional[str] = None,
    open_loops: Optional[List[str]] = None,
    commitments: Optional[List[str]] = None,
    environment_summary: Optional[str] = None,
    user_model: Optional[str] = None,
    persona_notes: Optional[str] = None,
    resume_hint: Optional[str] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "agent_state.update", "actor_id": actor_id}
    for field, value in (
        ("active_task_id", active_task_id),
        ("focus", focus),
        ("next_action", next_action),
        ("what_changed", what_changed),
        ("environment_summary", environment_summary),
        ("user_model", user_model),
        ("persona_notes", persona_notes),
        ("resume_hint", resume_hint),
    ):
        if value is not None:
            op[field] = value
    if blockers is not None:
        op["blockers"] = blockers
    if open_loops is not None:
        op["open_loops"] = open_loops
    if commitments is not None:
        op["commitments"] = commitments
    return context_sync(group_id=group_id, ops=[op], by=by)


def agent_state_clear(*, group_id: str, actor_id: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "agent_state.clear", "actor_id": actor_id}], by=by)


def _handle_context_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
    resolve_self_actor_id: Callable[[Dict[str, Any]], str],
    coerce_bool: Callable[..., bool],
    context_get_fn: Callable[..., Dict[str, Any]],
    context_sync_fn: Callable[..., Dict[str, Any]],
    coordination_get_fn: Callable[..., Dict[str, Any]],
    coordination_update_brief_fn: Callable[..., Dict[str, Any]],
    coordination_add_note_fn: Callable[..., Dict[str, Any]],
    task_list_fn: Callable[..., Dict[str, Any]],
    task_create_fn: Callable[..., Dict[str, Any]],
    task_update_fn: Callable[..., Dict[str, Any]],
    task_move_fn: Callable[..., Dict[str, Any]],
    task_restore_fn: Callable[..., Dict[str, Any]],
    agent_state_get_fn: Callable[..., Dict[str, Any]],
    agent_state_update_fn: Callable[..., Dict[str, Any]],
    agent_state_clear_fn: Callable[..., Dict[str, Any]],
) -> Optional[Dict[str, Any]]:
    if name == "cccc_context_get":
        gid = resolve_group_id(arguments)
        return context_get_fn(group_id=gid, include_archived=coerce_bool(arguments.get("include_archived"), default=False))

    if name == "cccc_context_sync":
        gid = resolve_group_id(arguments)
        by = resolve_self_actor_id(arguments)
        ops_raw = arguments.get("ops")
        if_version = arguments.get("if_version")
        return context_sync_fn(
            group_id=gid,
            ops=list(ops_raw) if isinstance(ops_raw, list) else [],
            dry_run=coerce_bool(arguments.get("dry_run"), default=False),
            if_version=str(if_version) if if_version is not None else None,
            by=by,
        )

    if name == "cccc_coordination":
        gid = resolve_group_id(arguments)
        by = resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "get").strip().lower()
        if action == "get":
            return coordination_get_fn(group_id=gid, include_archived=coerce_bool(arguments.get("include_archived"), default=False))
        if action == "update_brief":
            kwargs: Dict[str, Any] = {"group_id": gid, "by": by}
            for field in ("objective", "current_focus", "project_brief"):
                if field in arguments:
                    kwargs[field] = arguments[field]
            if "constraints" in arguments:
                raw = arguments.get("constraints")
                kwargs["constraints"] = list(raw) if isinstance(raw, list) else []
            if "project_brief_stale" in arguments:
                kwargs["project_brief_stale"] = coerce_bool(arguments.get("project_brief_stale"), default=False)
            return coordination_update_brief_fn(**kwargs)
        if action in {"add_decision", "add_handoff"}:
            summary = str(arguments.get("summary") or "")
            return coordination_add_note_fn(
                group_id=gid,
                kind=("decision" if action == "add_decision" else "handoff"),
                summary=summary,
                task_id=(str(arguments.get("task_id") or "").strip() or None),
                by=by,
            )
        raise MCPError(code="invalid_request", message="cccc_coordination action must be get|update_brief|add_decision|add_handoff")

    if name == "cccc_task":
        gid = resolve_group_id(arguments)
        by = resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "list").strip().lower()
        if action == "list":
            return task_list_fn(
                group_id=gid,
                task_id=(str(arguments.get("task_id") or "").strip() or None),
                include_archived=coerce_bool(arguments.get("include_archived"), default=False),
            )
        if action == "create":
            checklist_raw = arguments.get("checklist")
            blocked_by_raw = arguments.get("blocked_by")
            return task_create_fn(
                group_id=gid,
                title=str(arguments.get("title") or ""),
                outcome=str(arguments.get("outcome") or ""),
                status=str(arguments.get("status") or "planned"),
                parent_id=(str(arguments.get("parent_id") or "").strip() or None),
                assignee=(str(arguments.get("assignee") or "").strip() or None),
                priority=(str(arguments.get("priority") or "").strip() or None),
                blocked_by=list(blocked_by_raw) if isinstance(blocked_by_raw, list) else None,
                waiting_on=(str(arguments.get("waiting_on") or "").strip() or None),
                handoff_to=(str(arguments.get("handoff_to") or "").strip() or None),
                notes=(str(arguments.get("notes") or "") if "notes" in arguments else None),
                checklist=list(checklist_raw) if isinstance(checklist_raw, list) else None,
                by=by,
            )
        if action == "update":
            checklist_raw = arguments.get("checklist")
            blocked_by_raw = arguments.get("blocked_by")
            kwargs: Dict[str, Any] = {
                "group_id": gid,
                "task_id": str(arguments.get("task_id") or ""),
                "by": by,
            }
            for field in ("title", "outcome", "parent_id", "assignee", "priority", "waiting_on", "handoff_to", "notes"):
                if field in arguments:
                    value = arguments.get(field)
                    kwargs[field] = str(value) if value is not None else None
            if "blocked_by" in arguments:
                kwargs["blocked_by"] = list(blocked_by_raw) if isinstance(blocked_by_raw, list) else []
            if "checklist" in arguments:
                kwargs["checklist"] = list(checklist_raw) if isinstance(checklist_raw, list) else []
            return task_update_fn(**kwargs)
        if action == "move":
            return task_move_fn(
                group_id=gid,
                task_id=str(arguments.get("task_id") or ""),
                status=str(arguments.get("status") or ""),
                by=by,
            )
        if action == "restore":
            return task_restore_fn(group_id=gid, task_id=str(arguments.get("task_id") or ""), by=by)
        raise MCPError(code="invalid_request", message="cccc_task action must be list|create|update|move|restore")

    if name == "cccc_agent_state":
        gid = resolve_group_id(arguments)
        self_actor = resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "get").strip().lower()
        actor_id = str(arguments.get("actor_id") or arguments.get("agent_id") or "").strip() or self_actor
        if action == "get":
            return agent_state_get_fn(
                group_id=gid,
                actor_id=actor_id or None,
                include_warm=coerce_bool(arguments.get("include_warm"), default=True),
            )
        if action == "clear":
            return agent_state_clear_fn(group_id=gid, actor_id=actor_id, by=self_actor)
        if action != "update":
            raise MCPError(code="invalid_request", message="cccc_agent_state action must be get|update|clear")
        kwargs: Dict[str, Any] = {"group_id": gid, "actor_id": actor_id, "by": self_actor}
        for field in (
            "active_task_id",
            "focus",
            "next_action",
            "what_changed",
            "environment_summary",
            "user_model",
            "persona_notes",
            "resume_hint",
        ):
            if field in arguments:
                kwargs[field] = arguments[field]
        if "blockers" in arguments:
            raw = arguments.get("blockers")
            kwargs["blockers"] = list(raw) if isinstance(raw, list) else []
        if "open_loops" in arguments:
            raw = arguments.get("open_loops")
            kwargs["open_loops"] = list(raw) if isinstance(raw, list) else []
        if "commitments" in arguments:
            raw = arguments.get("commitments")
            kwargs["commitments"] = list(raw) if isinstance(raw, list) else []
        if "environment" in arguments and "environment_summary" not in kwargs:
            kwargs["environment_summary"] = arguments.get("environment")
        if "user_profile" in arguments and "user_model" not in kwargs:
            kwargs["user_model"] = arguments.get("user_profile")
        if "notes" in arguments and "persona_notes" not in kwargs:
            kwargs["persona_notes"] = arguments.get("notes")
        return agent_state_update_fn(**kwargs)

    return None
