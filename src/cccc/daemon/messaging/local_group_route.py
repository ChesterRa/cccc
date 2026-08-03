"""Render local-group route context for actor delivery."""

from __future__ import annotations

import re
from typing import Any


def _compact(value: Any, *, limit: int) -> str:
    text = re.sub(r"\s+", " ", str(value or "").strip())
    if len(text) <= limit:
        return text
    return text[: max(1, limit - 1)].rstrip() + "…"


def render_local_group_route_ref(ref: dict[str, Any]) -> list[str]:
    group_id = _compact(ref.get("group_id"), limit=48)
    if not group_id:
        return []
    group_title = _compact(ref.get("group_title"), limit=72)
    token = _compact(ref.get("token"), limit=72)
    label = group_title or token or group_id
    return [
        f"- Local group route {label} (group_id={group_id}); this is context, not an automatic send. "
        f"If the user asks you to contact it, decide first, then use cccc_message_send with "
        f'dst_group_id="{group_id}", to="@foreman" or a target actor, and your own natural message. '
        "Do not forward the user's text or a template."
    ]
