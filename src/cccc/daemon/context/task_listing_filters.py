from __future__ import annotations

from typing import Any, Dict, List, Optional

from ...kernel.context import Task, TaskStatus

STATUSES = ("planned", "active", "done", "archived")
ATTENTION_FILTERS = ("blocked", "waiting_user", "handoff", "unassigned")


def task_status(task: Task) -> str:
    value = (
        task.status.value
        if isinstance(task.status, TaskStatus)
        else str(task.status or "")
    )
    return value.strip() or TaskStatus.PLANNED.value


def task_ids(value: Any) -> List[str]:
    ids = comma_values(value, "task_ids")
    if len(ids) > 100:
        raise ValueError("task_ids accepts at most 100 ids")
    return ids


def status_list(value: Any) -> List[str]:
    values = comma_values(value, "statuses")
    invalid = next((item for item in values if item not in STATUSES), None)
    if invalid:
        raise ValueError(f"invalid statuses: {invalid}")
    return values


def filter_value(value: Any, name: str, allowed: tuple[str, ...]) -> Optional[str]:
    text = str(value or "").strip()
    if not text:
        return None
    if text not in allowed:
        raise ValueError(f"invalid {name}: {text}")
    return text


def pagination(args: Dict[str, Any]) -> Optional[tuple[int, int]]:
    limit_value = args.get("limit")
    if limit_value is None or limit_value == "":
        offset_value = args.get("offset")
        if offset_value is not None and offset_value != "":
            raise ValueError("offset requires limit")
        return None
    limit = integer(limit_value, "limit")
    if limit < 1 or limit > 100:
        raise ValueError("limit must be between 1 and 100")
    return integer(args.get("offset") or 0, "offset"), limit


def bool_arg(value: Any, name: str) -> bool:
    if value is None or value is False or value == 0 or value in ("", "false", "0"):
        return False
    if value is True or value == 1 or value in ("true", "1"):
        return True
    raise ValueError(f"{name} must be a boolean")


def matches(
    task: Task,
    status: Optional[str],
    query: str,
    assignee: str,
    attention: Optional[str],
) -> bool:
    current_status = task_status(task)
    task_assignee = str(task.assignee or "").strip()
    if status and current_status != status:
        return False
    if assignee == "__unassigned__" and task_assignee:
        return False
    if assignee and assignee != "__unassigned__" and task_assignee != assignee:
        return False
    if attention and not matches_attention(task, attention):
        return False
    values = (
        task.id,
        task.title,
        task.outcome,
        task.notes,
        task_assignee,
        task.priority,
        task.handoff_to,
    )
    return not query or any(query in str(value or "").casefold() for value in values)


def sort_tasks(tasks: List[Task], status: Optional[str]) -> List[Task]:
    date_field = "created_at" if status == "planned" else "updated_at"
    return sorted(
        tasks,
        key=lambda task: (
            str(getattr(task, date_field, "") or ""),
            task_number(task.id),
        ),
        reverse=True,
    )


def blocked(task: Task) -> bool:
    return bool(task.blocked_by) or enum_value(task.waiting_on) in {"actor", "external"}


def enum_value(value: Any) -> str:
    return str(getattr(value, "value", value) or "").strip()


def task_number(task_id: str) -> int:
    try:
        return int(task_id.removeprefix("T"))
    except ValueError:
        return 0


def matches_attention(task: Task, wanted: str) -> bool:
    status = task_status(task)
    if wanted == "unassigned":
        return status != "archived" and not str(task.assignee or "").strip()
    if status in {"done", "archived"}:
        return False
    if wanted == "blocked":
        return blocked(task)
    if wanted == "waiting_user":
        return enum_value(task.waiting_on) == "user"
    if wanted == "handoff":
        return bool(str(task.handoff_to or "").strip())
    return True


def comma_values(value: Any, name: str) -> List[str]:
    if value is None or value == "":
        return []
    values = list(
        dict.fromkeys(item.strip() for item in str(value).split(",") if item.strip())
    )
    if not values:
        raise ValueError(f"{name} must not be empty")
    return values


def integer(value: Any, name: str) -> int:
    if isinstance(value, bool):
        raise ValueError(f"{name} must be a non-negative integer")
    try:
        number = int(str(value).strip())
    except (TypeError, ValueError) as exc:
        raise ValueError(f"{name} must be a non-negative integer") from exc
    if number < 0:
        raise ValueError(f"{name} must be a non-negative integer")
    return number
