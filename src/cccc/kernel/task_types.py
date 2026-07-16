from __future__ import annotations

from typing import Any, Dict, Optional, Tuple

TaskTypeId = str

TASK_TYPE_IDS: Tuple[TaskTypeId, ...] = (
    "free",
    "standard",
    "optimization",
)

LEGACY_TASK_TYPE_ALIASES: Dict[str, TaskTypeId] = {
    "lean": "free",
    "root": "standard",
    "planner": "standard",
    "reviewer": "standard",
    "debugger": "standard",
    "release": "standard",
}


def normalize_task_type_id(value: Any) -> Optional[TaskTypeId]:
    normalized = str(value or "").strip().lower()
    if normalized in LEGACY_TASK_TYPE_ALIASES:
        return LEGACY_TASK_TYPE_ALIASES[normalized]
    if normalized in TASK_TYPE_IDS:
        return normalized
    return None


def default_task_type_id(parent_id: Any) -> TaskTypeId:
    return "free" if str(parent_id or "").strip() else "standard"


def resolve_task_type_id(value: Any, parent_id: Any) -> TaskTypeId:
    normalized = normalize_task_type_id(value)
    if normalized:
        return normalized
    return default_task_type_id(parent_id)
