from __future__ import annotations

from typing import Any, Callable, Dict, List, Optional

from ..common import MCPError, _call_daemon_or_raise


def context_get(*, group_id: str, include_archived: bool = False) -> Dict[str, Any]:
    """Get full context (v2)."""
    result = _call_daemon_or_raise({"op": "context_get", "args": {"group_id": group_id}})
    if include_archived:
        return result

    # Filter archived tasks from active_tasks
    active_tasks = result.get("active_tasks")
    if isinstance(active_tasks, list):
        result["active_tasks"] = [
            t
            for t in active_tasks
            if isinstance(t, dict) and str(t.get("status") or "").strip().lower() != "archived"
        ]

    tasks_summary = result.get("tasks_summary")
    if isinstance(tasks_summary, dict):
        try:
            active = int(tasks_summary.get("active") or 0)
            planned = int(tasks_summary.get("planned") or 0)
            done = int(tasks_summary.get("done") or 0)
            tasks_summary["total"] = active + planned + done
        except Exception:
            pass

    return result


def context_sync(
    *,
    group_id: str,
    ops: List[Dict[str, Any]],
    dry_run: bool = False,
    if_version: Optional[str] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    """Batch sync context operations (v2)."""
    args: Dict[str, Any] = {"group_id": group_id, "ops": ops, "dry_run": dry_run}
    if if_version is not None:
        args["if_version"] = if_version
    by_norm = str(by or "").strip()
    if by_norm:
        args["by"] = by_norm
    return _call_daemon_or_raise({"op": "context_sync", "args": args})


def task_list(
    *, group_id: str, task_id: Optional[str] = None, include_archived: bool = False
) -> Dict[str, Any]:
    """List tasks (v2: tree structure with parent_id)."""
    args: Dict[str, Any] = {"group_id": group_id}
    if task_id:
        args["task_id"] = task_id
    result = _call_daemon_or_raise({"op": "task_list", "args": args})
    if include_archived:
        return result

    if "task" in result and isinstance(result.get("task"), dict):
        task = result.get("task")
        status = str(task.get("status") or "").strip().lower() if isinstance(task, dict) else ""
        if status == "archived":
            raise MCPError(code="archived_hidden", message="archived task is hidden by default")
        return result

    tasks = result.get("tasks")
    if isinstance(tasks, list):
        result["tasks"] = [
            t
            for t in tasks
            if isinstance(t, dict) and str(t.get("status") or "").strip().lower() != "archived"
        ]
    return result


def vision_update(*, group_id: str, vision: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "vision.update", "vision": vision}], by=by)


def overview_manual_update(
    *, group_id: str,
    roles: Optional[List[str]] = None,
    collaboration_mode: Optional[str] = None,
    current_focus: Optional[str] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "overview.manual.update"}
    if roles is not None:
        op["roles"] = roles
    if collaboration_mode is not None:
        op["collaboration_mode"] = collaboration_mode
    if current_focus is not None:
        op["current_focus"] = current_focus
    return context_sync(group_id=group_id, ops=[op], by=by)


def task_create(
    *,
    group_id: str,
    name: str,
    goal: str,
    steps: List[Dict[str, str]],
    parent_id: Optional[str] = None,
    assignee: Optional[str] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        ops=[
            {
                "op": "task.create",
                "name": name,
                "goal": goal,
                "steps": steps,
                "parent_id": parent_id,
                "assignee": assignee,
            }
        ],
        by=by,
    )


def task_update(
    *,
    group_id: str,
    task_id: str,
    name: Optional[str] = None,
    goal: Optional[str] = None,
    assignee: Optional[str] = None,
    step_id: Optional[str] = None,
    step_status: Optional[str] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "task.update", "task_id": task_id}
    if name is not None:
        op["name"] = name
    if goal is not None:
        op["goal"] = goal
    if assignee is not None:
        op["assignee"] = assignee
    if step_id is not None and step_status is not None:
        op["step_id"] = step_id
        op["step_status"] = step_status
    return context_sync(group_id=group_id, ops=[op], by=by)


def task_status(*, group_id: str, task_id: str, status: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        ops=[{"op": "task.status", "task_id": task_id, "status": status}],
        by=by,
    )


def task_move(*, group_id: str, task_id: str, new_parent_id: Optional[str], by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        ops=[{"op": "task.move", "task_id": task_id, "new_parent_id": new_parent_id}],
        by=by,
    )


def task_restore(*, group_id: str, task_id: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        ops=[{"op": "task.restore", "task_id": task_id}],
        by=by,
    )


def context_agent_update(
    *,
    group_id: str,
    agent_id: str,
    active_task_id: Optional[str] = None,
    focus: Optional[str] = None,
    blockers: Optional[List[str]] = None,
    next_action: Optional[str] = None,
    what_changed: Optional[str] = None,
    decision_delta: Optional[str] = None,
    environment: Optional[str] = None,
    user_profile: Optional[str] = None,
    notes: Optional[str] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "agent.update", "agent_id": agent_id}
    if active_task_id is not None:
        op["active_task_id"] = active_task_id
    if focus is not None:
        op["focus"] = focus
    if blockers is not None:
        op["blockers"] = blockers
    if next_action is not None:
        op["next_action"] = next_action
    if what_changed is not None:
        op["what_changed"] = what_changed
    if decision_delta is not None:
        op["decision_delta"] = decision_delta
    if environment is not None:
        op["environment"] = environment
    if user_profile is not None:
        op["user_profile"] = user_profile
    if notes is not None:
        op["notes"] = notes
    return context_sync(group_id=group_id, ops=[op], by=by)


def context_agent_clear(*, group_id: str, agent_id: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "agent.clear", "agent_id": agent_id}], by=by)


def _handle_context_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
    resolve_self_actor_id: Callable[[Dict[str, Any]], str],
    coerce_bool: Callable[..., bool],
    context_get_fn: Callable[..., Dict[str, Any]],
    context_sync_fn: Callable[..., Dict[str, Any]],
    vision_update_fn: Callable[..., Dict[str, Any]],
    overview_manual_update_fn: Callable[..., Dict[str, Any]],
    task_list_fn: Callable[..., Dict[str, Any]],
    task_create_fn: Callable[..., Dict[str, Any]],
    task_update_fn: Callable[..., Dict[str, Any]],
    task_status_fn: Callable[..., Dict[str, Any]],
    task_move_fn: Callable[..., Dict[str, Any]],
    task_restore_fn: Callable[..., Dict[str, Any]],
    context_agent_update_fn: Callable[..., Dict[str, Any]],
    context_agent_clear_fn: Callable[..., Dict[str, Any]],
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

    if name == "cccc_context_admin":
        gid = resolve_group_id(arguments)
        by = resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "vision_update").strip().lower()
        if action == "vision_update":
            return vision_update_fn(group_id=gid, vision=str(arguments.get("vision") or ""), by=by)
        if action == "overview_update":
            kwargs: Dict[str, Any] = {"group_id": gid}
            if "roles" in arguments:
                roles_raw = arguments["roles"]
                kwargs["roles"] = list(roles_raw) if isinstance(roles_raw, list) else []
            if "collaboration_mode" in arguments:
                kwargs["collaboration_mode"] = str(arguments["collaboration_mode"])
            if "current_focus" in arguments:
                kwargs["current_focus"] = str(arguments["current_focus"])
            kwargs["by"] = by
            return overview_manual_update_fn(**kwargs)
        raise MCPError(
            code="invalid_request",
            message="cccc_context_admin action must be 'vision_update' or 'overview_update'",
        )

    if name == "cccc_task":
        gid = resolve_group_id(arguments)
        by = resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "list").strip().lower()
        if action == "list":
            return task_list_fn(
                group_id=gid,
                task_id=arguments.get("task_id"),
                include_archived=coerce_bool(arguments.get("include_archived"), default=False),
            )
        if action == "create":
            steps_raw = arguments.get("steps")
            return task_create_fn(
                group_id=gid,
                name=str(arguments.get("name") or ""),
                goal=str(arguments.get("goal") or ""),
                steps=list(steps_raw) if isinstance(steps_raw, list) else [],
                parent_id=arguments.get("parent_id"),
                assignee=arguments.get("assignee"),
                by=by,
            )
        if action == "update":
            return task_update_fn(
                group_id=gid,
                task_id=str(arguments.get("task_id") or ""),
                name=arguments.get("name"),
                goal=arguments.get("goal"),
                assignee=arguments.get("assignee"),
                step_id=arguments.get("step_id"),
                step_status=arguments.get("step_status"),
                by=by,
            )
        if action == "status":
            return task_status_fn(
                group_id=gid,
                task_id=str(arguments.get("task_id") or ""),
                status=str(arguments.get("status") or ""),
                by=by,
            )
        if action == "move":
            new_parent = arguments.get("new_parent_id")
            return task_move_fn(
                group_id=gid,
                task_id=str(arguments.get("task_id") or ""),
                new_parent_id=str(new_parent) if new_parent is not None else None,
                by=by,
            )
        if action == "restore":
            return task_restore_fn(
                group_id=gid,
                task_id=str(arguments.get("task_id") or ""),
                by=by,
            )
        raise MCPError(
            code="invalid_request",
            message="cccc_task action must be one of: list/create/update/status/move/restore",
        )

    if name == "cccc_context_agent":
        gid = resolve_group_id(arguments)
        self_aid = resolve_self_actor_id(arguments)
        agent_id = str(arguments.get("agent_id") or "").strip() or self_aid
        action = str(arguments.get("action") or "update").strip().lower()
        if action == "clear":
            return context_agent_clear_fn(group_id=gid, agent_id=agent_id, by=self_aid)
        if action != "update":
            raise MCPError(code="invalid_request", message="cccc_context_agent action must be 'update' or 'clear'")
        kwargs_agent: Dict[str, Any] = {"group_id": gid, "agent_id": agent_id}
        for field in (
            "active_task_id",
            "focus",
            "next_action",
            "what_changed",
            "decision_delta",
            "environment",
            "user_profile",
            "notes",
        ):
            if field in arguments:
                kwargs_agent[field] = arguments[field]
        if "blockers" in arguments:
            blockers_raw = arguments["blockers"]
            kwargs_agent["blockers"] = list(blockers_raw) if isinstance(blockers_raw, list) else []
        kwargs_agent["by"] = self_aid
        return context_agent_update_fn(**kwargs_agent)

    return None
