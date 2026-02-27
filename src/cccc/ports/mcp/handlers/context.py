from __future__ import annotations

from typing import Any, Callable, Dict, List, Optional

from ..common import MCPError, _call_daemon_or_raise


def context_get(*, group_id: str, include_archived: bool = False) -> Dict[str, Any]:
    """Get full context.

    By default, archived milestones are hidden to reduce cognitive load.
    """
    result = _call_daemon_or_raise({"op": "context_get", "args": {"group_id": group_id}})
    if include_archived:
        return result

    milestones = result.get("milestones")
    if isinstance(milestones, list):
        result["milestones"] = [
            m
            for m in milestones
            if isinstance(m, dict) and str(m.get("status") or "").strip().lower() != "archived"
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


def context_sync(*, group_id: str, ops: List[Dict[str, Any]], dry_run: bool = False) -> Dict[str, Any]:
    """Batch sync context operations."""
    return _call_daemon_or_raise(
        {"op": "context_sync", "args": {"group_id": group_id, "ops": ops, "dry_run": dry_run}}
    )


def task_list(
    *, group_id: str, task_id: Optional[str] = None, include_archived: bool = False
) -> Dict[str, Any]:
    """List tasks.

    By default, archived tasks are hidden to reduce cognitive load.
    """
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


def presence_get(*, group_id: str) -> Dict[str, Any]:
    """Get presence status."""
    return _call_daemon_or_raise({"op": "presence_get", "args": {"group_id": group_id}})


def vision_update(*, group_id: str, vision: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "vision.update", "vision": vision}])


def sketch_update(*, group_id: str, sketch: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "sketch.update", "sketch": sketch}])


def milestone_create(*, group_id: str, name: str, description: str, status: str = "planned") -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        ops=[{"op": "milestone.create", "name": name, "description": description, "status": status}],
    )


def milestone_update(
    *,
    group_id: str,
    milestone_id: str,
    name: Optional[str] = None,
    description: Optional[str] = None,
    status: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "milestone.update", "milestone_id": milestone_id}
    if name is not None:
        op["name"] = name
    if description is not None:
        op["description"] = description
    if status is not None:
        op["status"] = status
    return context_sync(group_id=group_id, ops=[op])


def milestone_complete(*, group_id: str, milestone_id: str, outcomes: str) -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        ops=[{"op": "milestone.complete", "milestone_id": milestone_id, "outcomes": outcomes}],
    )


def task_create(
    *,
    group_id: str,
    name: str,
    goal: str,
    steps: List[Dict[str, str]],
    milestone_id: Optional[str] = None,
    assignee: Optional[str] = None,
) -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        ops=[
            {
                "op": "task.create",
                "name": name,
                "goal": goal,
                "steps": steps,
                "milestone_id": milestone_id,
                "assignee": assignee,
            }
        ],
    )


def task_update(
    *,
    group_id: str,
    task_id: str,
    status: Optional[str] = None,
    name: Optional[str] = None,
    goal: Optional[str] = None,
    assignee: Optional[str] = None,
    milestone_id: Optional[str] = None,
    step_id: Optional[str] = None,
    step_status: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "task.update", "task_id": task_id}
    if status is not None:
        op["status"] = status
    if name is not None:
        op["name"] = name
    if goal is not None:
        op["goal"] = goal
    if assignee is not None:
        op["assignee"] = assignee
    if milestone_id is not None:
        op["milestone_id"] = milestone_id
    if step_id is not None and step_status is not None:
        op["step_id"] = step_id
        op["step_status"] = step_status
    return context_sync(group_id=group_id, ops=[op])


def note_add(*, group_id: str, content: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "note.add", "content": content}])


def note_update(*, group_id: str, note_id: str, content: Optional[str] = None) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "note.update", "note_id": note_id}
    if content is not None:
        op["content"] = content
    return context_sync(group_id=group_id, ops=[op])


def note_remove(*, group_id: str, note_id: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "note.remove", "note_id": note_id}])


def reference_add(*, group_id: str, url: str, note: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "reference.add", "url": url, "note": note}])


def reference_update(
    *, group_id: str, reference_id: str, url: Optional[str] = None, note: Optional[str] = None
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "reference.update", "reference_id": reference_id}
    if url is not None:
        op["url"] = url
    if note is not None:
        op["note"] = note
    return context_sync(group_id=group_id, ops=[op])


def reference_remove(*, group_id: str, reference_id: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "reference.remove", "reference_id": reference_id}])


def presence_update(*, group_id: str, agent_id: str, status: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "presence.update", "agent_id": agent_id, "status": status}])


def presence_clear(*, group_id: str, agent_id: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "presence.clear", "agent_id": agent_id}])


def _handle_context_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
    resolve_self_actor_id: Callable[[Dict[str, Any]], str],
    coerce_bool: Callable[[Any], bool],
    context_get_fn: Callable[..., Dict[str, Any]],
    context_sync_fn: Callable[..., Dict[str, Any]],
    vision_update_fn: Callable[..., Dict[str, Any]],
    sketch_update_fn: Callable[..., Dict[str, Any]],
    milestone_create_fn: Callable[..., Dict[str, Any]],
    milestone_update_fn: Callable[..., Dict[str, Any]],
    milestone_complete_fn: Callable[..., Dict[str, Any]],
    task_list_fn: Callable[..., Dict[str, Any]],
    task_create_fn: Callable[..., Dict[str, Any]],
    task_update_fn: Callable[..., Dict[str, Any]],
    note_add_fn: Callable[..., Dict[str, Any]],
    note_update_fn: Callable[..., Dict[str, Any]],
    note_remove_fn: Callable[..., Dict[str, Any]],
    reference_add_fn: Callable[..., Dict[str, Any]],
    reference_update_fn: Callable[..., Dict[str, Any]],
    reference_remove_fn: Callable[..., Dict[str, Any]],
    presence_get_fn: Callable[..., Dict[str, Any]],
    presence_update_fn: Callable[..., Dict[str, Any]],
    presence_clear_fn: Callable[..., Dict[str, Any]],
) -> Optional[Dict[str, Any]]:
    if name == "cccc_context_get":
        gid = resolve_group_id(arguments)
        return context_get_fn(group_id=gid, include_archived=coerce_bool(arguments.get("include_archived"), default=False))

    if name == "cccc_context_sync":
        gid = resolve_group_id(arguments)
        ops_raw = arguments.get("ops")
        return context_sync_fn(
            group_id=gid,
            ops=list(ops_raw) if isinstance(ops_raw, list) else [],
            dry_run=coerce_bool(arguments.get("dry_run"), default=False),
        )

    if name == "cccc_vision_update":
        gid = resolve_group_id(arguments)
        return vision_update_fn(group_id=gid, vision=str(arguments.get("vision") or ""))

    if name == "cccc_sketch_update":
        gid = resolve_group_id(arguments)
        return sketch_update_fn(group_id=gid, sketch=str(arguments.get("sketch") or ""))

    if name == "cccc_milestone_create":
        gid = resolve_group_id(arguments)
        return milestone_create_fn(
            group_id=gid,
            name=str(arguments.get("name") or ""),
            description=str(arguments.get("description") or ""),
            status=str(arguments.get("status") or "planned"),
        )

    if name == "cccc_milestone_update":
        gid = resolve_group_id(arguments)
        return milestone_update_fn(
            group_id=gid,
            milestone_id=str(arguments.get("milestone_id") or ""),
            name=arguments.get("name"),
            description=arguments.get("description"),
            status=arguments.get("status"),
        )

    if name == "cccc_milestone_complete":
        gid = resolve_group_id(arguments)
        return milestone_complete_fn(
            group_id=gid,
            milestone_id=str(arguments.get("milestone_id") or ""),
            outcomes=str(arguments.get("outcomes") or ""),
        )

    if name == "cccc_task_list":
        gid = resolve_group_id(arguments)
        return task_list_fn(
            group_id=gid,
            task_id=arguments.get("task_id"),
            include_archived=coerce_bool(arguments.get("include_archived"), default=False),
        )

    if name == "cccc_task_create":
        gid = resolve_group_id(arguments)
        steps_raw = arguments.get("steps")
        return task_create_fn(
            group_id=gid,
            name=str(arguments.get("name") or ""),
            goal=str(arguments.get("goal") or ""),
            steps=list(steps_raw) if isinstance(steps_raw, list) else [],
            milestone_id=arguments.get("milestone_id"),
            assignee=arguments.get("assignee"),
        )

    if name == "cccc_task_update":
        gid = resolve_group_id(arguments)
        return task_update_fn(
            group_id=gid,
            task_id=str(arguments.get("task_id") or ""),
            status=arguments.get("status"),
            name=arguments.get("name"),
            goal=arguments.get("goal"),
            assignee=arguments.get("assignee"),
            milestone_id=arguments.get("milestone_id"),
            step_id=arguments.get("step_id"),
            step_status=arguments.get("step_status"),
        )

    if name == "cccc_note_add":
        gid = resolve_group_id(arguments)
        return note_add_fn(group_id=gid, content=str(arguments.get("content") or ""))

    if name == "cccc_note_update":
        gid = resolve_group_id(arguments)
        return note_update_fn(group_id=gid, note_id=str(arguments.get("note_id") or ""), content=arguments.get("content"))

    if name == "cccc_note_remove":
        gid = resolve_group_id(arguments)
        return note_remove_fn(group_id=gid, note_id=str(arguments.get("note_id") or ""))

    if name == "cccc_reference_add":
        gid = resolve_group_id(arguments)
        return reference_add_fn(group_id=gid, url=str(arguments.get("url") or ""), note=str(arguments.get("note") or ""))

    if name == "cccc_reference_update":
        gid = resolve_group_id(arguments)
        return reference_update_fn(
            group_id=gid,
            reference_id=str(arguments.get("reference_id") or ""),
            url=arguments.get("url"),
            note=arguments.get("note"),
        )

    if name == "cccc_reference_remove":
        gid = resolve_group_id(arguments)
        return reference_remove_fn(group_id=gid, reference_id=str(arguments.get("reference_id") or ""))

    if name == "cccc_presence_get":
        gid = resolve_group_id(arguments)
        return presence_get_fn(group_id=gid)

    if name == "cccc_presence_update":
        gid = resolve_group_id(arguments)
        self_aid = resolve_self_actor_id(arguments)
        agent_id = str(arguments.get("agent_id") or "").strip() or self_aid
        return presence_update_fn(group_id=gid, agent_id=agent_id, status=str(arguments.get("status") or ""))

    if name == "cccc_presence_clear":
        gid = resolve_group_id(arguments)
        self_aid = resolve_self_actor_id(arguments)
        agent_id = str(arguments.get("agent_id") or "").strip() or self_aid
        return presence_clear_fn(group_id=gid, agent_id=agent_id)

    return None

