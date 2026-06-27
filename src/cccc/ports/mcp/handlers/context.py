from __future__ import annotations

from typing import Any, Callable, Dict, List, Optional

from ....kernel.actors import get_effective_role, list_actors
from ....kernel.group import load_group
from ....kernel.prompt_files import (
    HELP_FILENAME,
    delete_group_prompt_file,
    load_builtin_help_markdown,
    read_group_prompt_file,
    write_group_prompt_file,
)
from ..task_types import default_task_type_id, normalize_task_type_id
from ..common import MCPError, _call_daemon_or_raise
from ..utils.help_markdown import parse_help_markdown, update_actor_help_note


def _resolve_task_create_assignee(
    *,
    group_id: str,
    actor_id: Optional[str],
    arguments: Dict[str, Any],
) -> Optional[str]:
    if "assignee" in arguments:
        return str(arguments.get("assignee") or "").strip() or None
    actor = str(actor_id or "").strip()
    if not actor or actor in {"system", "user"}:
        return None
    group = load_group(group_id)
    if group is None:
        return None
    role = get_effective_role(group, actor)
    if role == "peer":
        return actor
    return None


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
    task_type: Optional[str] = None,
    notes: Optional[str] = None,
    checklist: Optional[List[Dict[str, Any]]] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    resolved_task_type = task_type or default_task_type_id(parent_id)
    op: Dict[str, Any] = {
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
        "task_type": resolved_task_type,
    }
    if notes is not None:
        op["notes"] = notes
    if checklist is not None:
        op["checklist"] = checklist
    return context_sync(
        group_id=group_id,
        by=by,
        ops=[op],
    )


def task_update(
    *,
    group_id: str,
    task_id: str,
    title: Optional[str] = None,
    outcome: Optional[str] = None,
    status: Optional[str] = None,
    parent_id: Optional[str] = None,
    assignee: Optional[str] = None,
    priority: Optional[str] = None,
    blocked_by: Optional[List[str]] = None,
    waiting_on: Optional[str] = None,
    handoff_to: Optional[str] = None,
    task_type: Optional[str] = None,
    notes: Optional[str] = None,
    checklist: Optional[List[Dict[str, Any]]] = None,
    by: Optional[str] = None,
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "task.update", "task_id": task_id}
    has_patch_fields = False
    if title is not None:
        op["title"] = title
        has_patch_fields = True
    if outcome is not None:
        op["outcome"] = outcome
        has_patch_fields = True
    if parent_id is not None:
        op["parent_id"] = parent_id
        has_patch_fields = True
    if assignee is not None:
        op["assignee"] = assignee
        has_patch_fields = True
    if priority is not None:
        op["priority"] = priority
        has_patch_fields = True
    if blocked_by is not None:
        op["blocked_by"] = blocked_by
        has_patch_fields = True
    if waiting_on is not None:
        op["waiting_on"] = waiting_on
        has_patch_fields = True
    if handoff_to is not None:
        op["handoff_to"] = handoff_to
        has_patch_fields = True
    if task_type is not None:
        op["task_type"] = task_type
        has_patch_fields = True
    if notes is not None:
        op["notes"] = notes
        has_patch_fields = True
    if checklist is not None:
        op["checklist"] = checklist
        has_patch_fields = True

    ops: List[Dict[str, Any]] = []
    if has_patch_fields or status is None:
        ops.append(op)
    if status is not None:
        ops.append({"op": "task.move", "task_id": task_id, "status": status})
    return context_sync(group_id=group_id, ops=ops, by=by)


def task_move(*, group_id: str, task_id: str, status: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(
        group_id=group_id,
        ops=[{"op": "task.move", "task_id": task_id, "status": status}],
        by=by,
    )


def task_restore(*, group_id: str, task_id: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "task.restore", "task_id": task_id}], by=by)


def task_delete(*, group_id: str, task_id: str, by: Optional[str] = None) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "task.delete", "task_id": task_id}], by=by)


def _resolve_task_type(arguments: Dict[str, Any]) -> Optional[str]:
    if "type" not in arguments:
        return None
    task_type = normalize_task_type_id(arguments.get("type"))
    raw = str(arguments.get("type") or "").strip()
    if raw and task_type is None:
        raise MCPError(
            code="invalid_request",
            message="cccc_task type must be one of: free|standard|optimization",
        )
    return task_type


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


def _load_actor_notes_help(group: Any) -> Dict[str, Any]:
    prompt_file = read_group_prompt_file(group, HELP_FILENAME)
    if prompt_file.found and isinstance(prompt_file.content, str) and prompt_file.content.strip():
        content = str(prompt_file.content)
        source = "home"
    else:
        content = str(load_builtin_help_markdown() or "")
        source = "builtin"
    return {
        "content": content,
        "source": source,
        "path": prompt_file.path,
    }


def _group_actor_ids(group: Any) -> List[str]:
    return [
        str(item.get("id") or "").strip()
        for item in list_actors(group)
        if isinstance(item, dict) and str(item.get("id") or "").strip()
    ]


def _canonical_actor_id(actor_ids: List[str], target_actor_id: str, extra_actor_ids: Optional[List[str]] = None) -> str:
    target = str(target_actor_id or "").strip()
    if not target:
        return ""
    target_fold = target.casefold()
    for candidate in list(actor_ids or []):
        if str(candidate or "").strip().casefold() == target_fold:
            return str(candidate or "").strip()
    for candidate in list(extra_actor_ids or []):
        if str(candidate or "").strip().casefold() == target_fold:
            return str(candidate or "").strip()
    return target


def _write_actor_notes_help(group: Any, markdown: str) -> Dict[str, Any]:
    builtin = str(load_builtin_help_markdown() or "")
    next_content = str(markdown or "")
    if not next_content.strip() or next_content == builtin:
        prompt_file = delete_group_prompt_file(group, HELP_FILENAME)
        return {
            "content": builtin,
            "source": "builtin",
            "path": prompt_file.path,
        }
    prompt_file = write_group_prompt_file(group, HELP_FILENAME, next_content)
    return {
        "content": str(prompt_file.content or ""),
        "source": "home",
        "path": prompt_file.path,
    }


def _notify_actor_notes_target(*, group_id: str, target_actor_id: str) -> List[str]:
    target = str(target_actor_id or "").strip()
    if not target:
        return []
    try:
        actor_list_resp = _call_daemon_or_raise({"op": "actor_list", "args": {"group_id": group_id}})
        actors = actor_list_resp.get("actors") if isinstance(actor_list_resp.get("actors"), list) else []
        target_view = None
        for item in actors:
            if not isinstance(item, dict):
                continue
            if str(item.get("id") or "").strip().casefold() == target.casefold():
                target_view = item
                break
        if not isinstance(target_view, dict) or not bool(target_view.get("running")):
            return []
        _call_daemon_or_raise(
            {
                "op": "system_notify",
                "args": {
                    "group_id": group_id,
                    "by": "system",
                    "kind": "info",
                    "priority": "normal",
                    "title": "Help updated: your actor note",
                    "message": (
                        "Updated: your actor note. Run `cccc_help` now to refresh your effective playbook; "
                        "then update `cccc_agent_state` if your plan changes."
                    ),
                    "target_actor_id": str(target_view.get("id") or target),
                    "requires_ack": False,
                },
            }
        )
        return [str(target_view.get("id") or target)]
    except Exception:
        return []


def actor_notes_get(*, group_id: str, caller_actor_id: str, target_actor_id: Optional[str] = None) -> Dict[str, Any]:
    group = load_group(group_id)
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {group_id}")
    caller_id = str(caller_actor_id or "").strip()
    role = str(get_effective_role(group, caller_id) or "").strip().lower()
    help_prompt = _load_actor_notes_help(group)
    parsed = parse_help_markdown(str(help_prompt.get("content") or ""))
    actor_ids = _group_actor_ids(group)
    actor_notes = parsed.get("actor_notes") if isinstance(parsed.get("actor_notes"), dict) else {}
    extra_actor_ids = [str(aid or "").strip() for aid in actor_notes.keys() if str(aid or "").strip() not in actor_ids]
    target = _canonical_actor_id(actor_ids, str(target_actor_id or "").strip(), extra_actor_ids=extra_actor_ids)
    if role != "foreman":
        if not target:
            raise MCPError(code="permission_denied", message="listing all actor notes requires foreman access")
        if target.casefold() != caller_id.casefold():
            raise MCPError(code="permission_denied", message="actors can only read their own actor notes")
    if target_actor_id:
        return {
            "target_actor_id": target,
            "content": str(actor_notes.get(target) or ""),
            "source": help_prompt.get("source"),
            "path": help_prompt.get("path"),
        }
    ordered_actor_ids = list(actor_ids)
    for actor_id in extra_actor_ids:
        if actor_id not in ordered_actor_ids:
            ordered_actor_ids.append(actor_id)
    return {
        "actor_notes": [
            {"actor_id": actor_id, "content": str(actor_notes.get(actor_id) or "")}
            for actor_id in ordered_actor_ids
        ],
        "source": help_prompt.get("source"),
        "path": help_prompt.get("path"),
    }


def actor_notes_set(*, group_id: str, target_actor_id: str, content: str, by: Optional[str] = None) -> Dict[str, Any]:
    group = load_group(group_id)
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {group_id}")
    caller_id = str(by or "").strip()
    role = str(get_effective_role(group, caller_id) or "").strip().lower()
    if role != "foreman":
        raise MCPError(code="permission_denied", message="modifying actor notes requires foreman access")
    actor_ids = _group_actor_ids(group)
    target = _canonical_actor_id(actor_ids, target_actor_id)
    if target not in actor_ids:
        raise MCPError(code="actor_not_found", message=f"actor not found: {target_actor_id}")
    current = _load_actor_notes_help(group)
    next_content = update_actor_help_note(
        str(current.get("content") or ""),
        target,
        str(content or ""),
        actor_order=actor_ids,
    )
    if next_content == str(current.get("content") or ""):
        return {
            "target_actor_id": target,
            "content": str(parse_help_markdown(next_content).get("actor_notes", {}).get(target) or ""),
            "source": current.get("source"),
            "path": current.get("path"),
            "notified_actor_ids": [],
        }
    written = _write_actor_notes_help(group, next_content)
    return {
        "target_actor_id": target,
        "content": str(parse_help_markdown(str(written.get("content") or "")).get("actor_notes", {}).get(target) or ""),
        "source": written.get("source"),
        "path": written.get("path"),
        "notified_actor_ids": _notify_actor_notes_target(group_id=group_id, target_actor_id=target),
    }


def actor_notes_clear(*, group_id: str, target_actor_id: str, by: Optional[str] = None) -> Dict[str, Any]:
    return actor_notes_set(group_id=group_id, target_actor_id=target_actor_id, content="", by=by)


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
    task_delete_fn: Callable[..., Dict[str, Any]],
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
            task_type = _resolve_task_type(arguments)
            assignee = _resolve_task_create_assignee(group_id=gid, actor_id=by, arguments=arguments)
            return task_create_fn(
                group_id=gid,
                title=str(arguments.get("title") or ""),
                outcome=str(arguments.get("outcome") or ""),
                status=str(arguments.get("status") or "planned"),
                parent_id=(str(arguments.get("parent_id") or "").strip() or None),
                assignee=assignee,
                priority=(str(arguments.get("priority") or "").strip() or None),
                blocked_by=list(blocked_by_raw) if isinstance(blocked_by_raw, list) else None,
                waiting_on=(str(arguments.get("waiting_on") or "").strip() or None),
                handoff_to=(str(arguments.get("handoff_to") or "").strip() or None),
                task_type=task_type,
                notes=(str(arguments.get("notes") or "") if "notes" in arguments else None),
                checklist=(list(checklist_raw) if isinstance(checklist_raw, list) else None),
                by=by,
            )
        if action == "update":
            checklist_raw = arguments.get("checklist")
            blocked_by_raw = arguments.get("blocked_by")
            task_type = _resolve_task_type(arguments)
            kwargs: Dict[str, Any] = {
                "group_id": gid,
                "task_id": str(arguments.get("task_id") or ""),
                "by": by,
            }
            for field in ("title", "outcome", "status", "parent_id", "assignee", "priority", "waiting_on", "handoff_to", "notes"):
                if field in arguments:
                    value = arguments.get(field)
                    kwargs[field] = str(value) if value is not None else None
            if "blocked_by" in arguments:
                kwargs["blocked_by"] = list(blocked_by_raw) if isinstance(blocked_by_raw, list) else []
            if "type" in arguments:
                kwargs["task_type"] = task_type
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
        if action == "delete":
            return task_delete_fn(group_id=gid, task_id=str(arguments.get("task_id") or ""), by=by)
        raise MCPError(code="invalid_request", message="cccc_task action must be list|create|update|move|restore|delete")

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
