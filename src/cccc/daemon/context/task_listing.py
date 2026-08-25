from __future__ import annotations

from typing import Any, Callable, Dict, Iterable, List, Optional

from ...kernel.context import Task, TaskStatus, WaitingOn
from .task_listing_filters import (
    ATTENTION_FILTERS,
    STATUSES,
    blocked,
    bool_arg,
    enum_value,
    filter_value,
    matches,
    pagination,
    sort_tasks,
    status_list,
    task_number,
    task_status,
)


def build_task_listing(
    tasks: List[Task],
    args: Dict[str, Any],
    *,
    version: str,
    serialize: Callable[[Task], Dict[str, Any]],
) -> Dict[str, Any]:
    status = filter_value(args.get("status"), "status", STATUSES)
    statuses = status_list(args.get("statuses"))
    if status and statuses:
        raise ValueError("status and statuses cannot be combined")
    attention = filter_value(args.get("attention"), "attention", ATTENTION_FILTERS)
    query = str(args.get("query") or "").strip().casefold()
    assignee = str(args.get("assignee") or "").strip()
    page_range = pagination(args)
    facet_value = facets(tasks)

    if statuses:
        offset, limit = page_range or (0, 30)
        result: Dict[str, Any] = {
            "pages": {
                item: _page(
                    tasks, item, offset, limit, query, assignee, attention, serialize
                )
                for item in statuses
            },
            "tasks_version": version,
            "facets": facet_value,
        }
        if bool_arg(args.get("include_index"), "include_index"):
            result["task_index"] = task_index(tasks)
        return result

    matching = [
        task for task in tasks if matches(task, status, query, assignee, attention)
    ]
    matching = sort_tasks(matching, status)
    if page_range is None:
        return {"tasks": [serialize(task) for task in matching]}
    offset, limit = page_range
    result = _page(tasks, status, offset, limit, query, assignee, attention, serialize)
    result.update({"tasks_version": version, "facets": facet_value})
    if bool_arg(args.get("include_index"), "include_index"):
        result["task_index"] = task_index(tasks)
    return result


def facets(tasks: Iterable[Task]) -> Dict[str, Any]:
    status_counts: Dict[str, int] = {}
    assignees = set()
    blocked_count = waiting_user = handoffs = unassigned = 0
    for task in tasks:
        status = task_status(task)
        status_counts[status] = status_counts.get(status, 0) + 1
        if status == TaskStatus.ARCHIVED.value:
            continue
        assignee = str(task.assignee or "").strip()
        if assignee:
            assignees.add(assignee)
        else:
            unassigned += 1
        if status != TaskStatus.DONE.value:
            blocked_count += int(blocked(task))
            waiting_user += int(enum_value(task.waiting_on) == WaitingOn.USER.value)
            handoffs += int(bool(str(task.handoff_to or "").strip()))
    return {
        "status_counts": status_counts,
        "blocked": blocked_count,
        "waiting_user": waiting_user,
        "pending_handoffs": handoffs,
        "unassigned": unassigned,
        "assignees": sorted(assignees),
    }


def task_index(tasks: Iterable[Task]) -> List[Dict[str, Any]]:
    result = [
        {
            "id": task.id,
            "title": task.title,
            "status": task_status(task),
            "assignee": task.assignee,
            "parent_id": task.parent_id,
        }
        for task in tasks
        if task_status(task) != TaskStatus.ARCHIVED.value
    ]
    return sorted(
        result, key=lambda item: task_number(str(item.get("id") or "")), reverse=True
    )


def _page(
    tasks: List[Task],
    status: Optional[str],
    offset: int,
    limit: int,
    query: str,
    assignee: str,
    attention: Optional[str],
    serialize: Callable[[Task], Dict[str, Any]],
) -> Dict[str, Any]:
    matching = sort_tasks(
        [task for task in tasks if matches(task, status, query, assignee, attention)],
        status,
    )
    selected = matching[offset : offset + limit]
    return {
        "tasks": [serialize(task) for task in selected],
        "count": len(selected),
        "total_count": len(matching),
        "offset": offset,
        "limit": limit,
        "has_more": offset + len(selected) < len(matching),
    }
