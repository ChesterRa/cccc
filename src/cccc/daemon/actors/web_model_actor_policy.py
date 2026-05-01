"""Policy helpers for ChatGPT Web Model actors."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ...kernel.actors import list_actors
from ...kernel.group import load_group
from ...kernel.registry import load_registry
from ...paths import ensure_home


def _group_ids_from_home() -> list[str]:
    group_ids = {str(group_id or "").strip() for group_id in load_registry().groups.keys()}
    groups_dir = ensure_home() / "groups"
    try:
        if groups_dir.exists():
            for child in groups_dir.iterdir():
                if child.is_dir() and not child.name.startswith("."):
                    group_ids.add(child.name)
    except Exception:
        pass
    return sorted(group_id for group_id in group_ids if group_id)


def find_existing_chatgpt_web_model_actor(
    *,
    exclude_group_id: str = "",
    exclude_actor_id: str = "",
) -> Optional[Dict[str, str]]:
    """Return the existing ChatGPT Web Model actor, if one is already configured."""

    excluded_group = str(exclude_group_id or "").strip()
    excluded_actor = str(exclude_actor_id or "").strip()
    for group_id in _group_ids_from_home():
        group = load_group(group_id)
        if group is None:
            continue
        for actor in list_actors(group):
            if not isinstance(actor, dict):
                continue
            if str(actor.get("runtime") or "").strip().lower() != "web_model":
                continue
            actor_id = str(actor.get("id") or "").strip()
            if group_id == excluded_group and actor_id == excluded_actor:
                continue
            return {
                "group_id": group_id,
                "actor_id": actor_id,
                "title": str(actor.get("title") or "").strip(),
            }
    return None


def require_no_other_chatgpt_web_model_actor(
    *,
    group_id: str,
    actor_id: str = "",
) -> None:
    existing = find_existing_chatgpt_web_model_actor(
        exclude_group_id=group_id,
        exclude_actor_id=actor_id,
    )
    if existing is None:
        return
    label = existing.get("title") or existing.get("actor_id") or "ChatGPT Web Model"
    raise ValueError(
        "ChatGPT Web Model is limited to one actor per CCCC instance "
        f"(existing actor: {label} in group {existing.get('group_id')}). "
        "Remove the existing ChatGPT Web Model actor before creating another."
    )
