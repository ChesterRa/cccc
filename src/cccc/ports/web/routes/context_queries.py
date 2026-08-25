from __future__ import annotations

from typing import Any, Dict, Mapping


TASK_QUERY_NAMES = (
    "task_id",
    "task_ids",
    "status",
    "statuses",
    "query",
    "assignee",
    "attention",
    "offset",
    "limit",
    "include_index",
)


def task_query_args(group_id: str, query: Mapping[str, Any]) -> Dict[str, Any]:
    """Forward only the task-list query contract to the daemon."""
    args: Dict[str, Any] = {"group_id": group_id}
    args.update(
        (name, value)
        for name in TASK_QUERY_NAMES
        if (value := query.get(name)) not in {None, ""}
    )
    return args
