"""Install-time slash visibility defaults for capability_ops."""

from __future__ import annotations

from typing import Any, Dict


def clear_hidden_capability_after_install(
    state_doc: Dict[str, Any],
    *,
    group_id: str,
    actor_id: str,
    capability_id: str,
) -> bool:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    cap_id = str(capability_id or "").strip()
    if not gid or not aid or not cap_id:
        return False

    actor_hidden = state_doc.get("actor_hidden") if isinstance(state_doc.get("actor_hidden"), dict) else {}
    per_group = actor_hidden.get(gid) if isinstance(actor_hidden.get(gid), dict) else {}
    items = per_group.get(aid) if isinstance(per_group.get(aid), list) else []
    clean_items = sorted({str(item or "").strip() for item in items if str(item or "").strip()})
    if cap_id not in clean_items:
        return False
    next_items = [item for item in clean_items if item != cap_id]

    if next_items:
        per_group[aid] = next_items
    else:
        per_group.pop(aid, None)
    if per_group:
        actor_hidden[gid] = per_group
    else:
        actor_hidden.pop(gid, None)
    state_doc["actor_hidden"] = actor_hidden
    return True
