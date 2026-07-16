from __future__ import annotations

from typing import Any, Dict
from urllib.parse import quote

from ...kernel.actor_avatar_assets import delete_actor_avatar, resolve_actor_avatar_path, store_actor_avatar


def build_actor_avatar_payload(group_id: str, actor: Dict[str, Any]) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    actor_id = str(actor.get("id") or "").strip()
    rel_path = str(actor.get("avatar_asset_path") or "").strip()
    updated_at = str(actor.get("updated_at") or "").strip() or "default"
    avatar_url = (
        f"/api/v1/groups/{quote(gid, safe='')}/actors/{quote(actor_id, safe='')}/avatar?v={quote(updated_at, safe='')}"
        if gid and actor_id and rel_path
        else ""
    )
    return {
        "avatar_url": avatar_url or None,
        "has_custom_avatar": bool(rel_path),
    }


def build_actor_web_payload(group_id: str, actor: Dict[str, Any]) -> Dict[str, Any]:
    out = dict(actor)
    out.update(build_actor_avatar_payload(group_id, actor))
    return out
