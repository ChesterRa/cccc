from __future__ import annotations

from typing import Any, Callable, Dict, List

from ...kernel.context import ContextStorage, Task, TaskStatus
from .task_listing import build_task_listing
from .task_listing_filters import sort_tasks, task_ids, task_status


def task_list_result(
    storage: ContextStorage,
    args: Dict[str, Any],
    *,
    serialize: Callable[[Task], Dict[str, Any]],
) -> Dict[str, Any]:
    version = f"tasksv:{storage.load_version_state()['tasks_rev']}"
    task_id = str(args.get("task_id") or "").strip()
    if task_id:
        task = storage.load_task(task_id)
        if task is None:
            raise LookupError(task_id)
        all_tasks = storage.list_tasks()
        payload = serialize(task)
        payload["children"] = [
            serialize(child)
            for child in sort_tasks(
                storage.get_task_children(task_id, tasks=all_tasks), None
            )
        ]
        return {
            "task": payload,
            "tasks_version": version,
            "delete_info": delete_info(all_tasks, task_id),
        }

    raw_ids = str(args.get("task_ids") or "").strip()
    if raw_ids:
        selected = [storage.load_task(task_id) for task_id in task_ids(raw_ids)]
        return {
            "tasks": [serialize(task) for task in selected if task is not None],
            "tasks_version": version,
        }

    tasks = storage.list_tasks()
    return build_task_listing(tasks, args, version=version, serialize=serialize)


def delete_info(tasks: List[Task], root_id: str) -> Dict[str, Any]:
    by_parent: Dict[str, List[Task]] = {}
    by_id = {task.id: task for task in tasks}
    for task in tasks:
        parent_id = str(task.parent_id or "").strip()
        if parent_id:
            by_parent.setdefault(parent_id, []).append(task)
    subtree: List[Task] = []
    pending = [root_id]
    seen = set()
    while pending:
        task_id = pending.pop()
        if task_id in seen or task_id not in by_id:
            continue
        seen.add(task_id)
        subtree.append(by_id[task_id])
        pending.extend(child.id for child in by_parent.get(task_id, []))
    blocked = next((task for task in subtree if not _unexecuted(task)), None)
    reason = (
        ""
        if blocked is None
        else "self_history"
        if blocked.id == root_id
        else "subtree_history"
    )
    return {"allowed": blocked is None, "total": len(subtree), "reason": reason}


def _unexecuted(task: Task) -> bool:
    status = task_status(task)
    archived_from = str(task.archived_from or "").strip()
    return status == TaskStatus.PLANNED.value or (
        status == TaskStatus.ARCHIVED.value
        and archived_from in {"", TaskStatus.PLANNED.value}
    )
