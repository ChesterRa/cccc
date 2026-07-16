"""MCP handler functions for the group Presentation surface."""

from __future__ import annotations

from typing import Any, Dict

from ..common import _call_daemon_or_raise


def presentation_get(*, group_id: str) -> Dict[str, Any]:
    return _call_daemon_or_raise({"op": "presentation_get", "args": {"group_id": group_id}})


def presentation_publish(
    *,
    group_id: str,
    actor_id: str,
    slot: str = "auto",
    card_type: str = "",
    title: str = "",
    summary: str = "",
    source_label: str = "",
    source_ref: str = "",
    content: str = "",
    table: Any = None,
    path: str = "",
    url: str = "",
    blob_rel_path: str = "",
) -> Dict[str, Any]:
    return _call_daemon_or_raise(
        {
            "op": "presentation_publish",
            "args": {
                "group_id": group_id,
                "by": actor_id,
                "slot": slot,
                "card_type": card_type,
                "title": title,
                "summary": summary,
                "source_label": source_label,
                "source_ref": source_ref,
                "content": content,
                "table": table if isinstance(table, (dict, list)) else None,
                "path": path,
                "url": url,
                "blob_rel_path": blob_rel_path,
            },
        }
    )


def presentation_clear(*, group_id: str, actor_id: str, slot: str = "", clear_all: bool = False) -> Dict[str, Any]:
    return _call_daemon_or_raise(
        {
            "op": "presentation_clear",
            "args": {
                "group_id": group_id,
                "by": actor_id,
                "slot": slot,
                "all": bool(clear_all),
            },
        }
    )

