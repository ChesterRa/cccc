"""Compact capability-state projections for narrow UI surfaces."""

from __future__ import annotations

from typing import Any, Dict, List


_SLASH_TOOL_FIELDS = {"name", "description", "inputSchema", "capability_id", "real_tool_name"}
_SLASH_SKILL_FIELDS = {"capability_id", "name", "description_short", "capsule_preview"}


def _project_rows(rows: Any, allowed_fields: set[str]) -> List[Dict[str, Any]]:
    if not isinstance(rows, list):
        return []
    out: List[Dict[str, Any]] = []
    for item in rows:
        if not isinstance(item, dict):
            continue
        projected = {key: item.get(key) for key in allowed_fields if key in item}
        if projected:
            out.append(projected)
    return out


def capability_state_view(result: Dict[str, Any], view: str) -> Dict[str, Any]:
    normalized = str(view or "").strip().lower()
    if not is_slash_commands_view(normalized):
        return result
    return {
        "group_id": str(result.get("group_id") or ""),
        "actor_id": str(result.get("actor_id") or ""),
        "view": "slash_commands",
        "dynamic_tools": _project_rows(result.get("dynamic_tools"), _SLASH_TOOL_FIELDS),
        "active_capsule_skills": _project_rows(result.get("active_capsule_skills"), _SLASH_SKILL_FIELDS),
        "actor_hidden_capabilities": [
            str(item or "").strip()
            for item in (result.get("actor_hidden_capabilities") if isinstance(result.get("actor_hidden_capabilities"), list) else [])
            if str(item or "").strip()
        ],
    }


def is_slash_commands_view(view: str) -> bool:
    return str(view or "").strip().lower() in {"slash", "slash_commands"}
