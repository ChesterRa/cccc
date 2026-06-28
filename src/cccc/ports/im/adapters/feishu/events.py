"""Feishu SDK event normalization helpers."""

from __future__ import annotations

from typing import Any, Dict, List, Optional


def _getattr_text(obj: Any, name: str, default: str = "") -> str:
    return str(getattr(obj, name, "") or default)


def _normalize_mentions(raw_mentions: Any) -> Optional[List[Dict[str, Any]]]:
    if not raw_mentions:
        return None

    mentions: List[Dict[str, Any]] = []
    for mention in raw_mentions:
        item: Dict[str, Any] = {
            "key": _getattr_text(mention, "key"),
            "name": _getattr_text(mention, "name"),
        }
        mention_id = getattr(mention, "id", None)
        if mention_id:
            item["id"] = {
                "open_id": _getattr_text(mention_id, "open_id"),
                "user_id": _getattr_text(mention_id, "user_id"),
            }
        mentions.append(item)
    return mentions


def normalize_message_event(data: Any) -> Dict[str, Any]:
    """Convert a Feishu SDK message event object to the adapter event shape."""
    event_data: Dict[str, Any] = {
        "message": {},
        "sender": {"sender_id": {}},
    }

    event_obj = getattr(data, "event", None)
    if not event_obj:
        return event_data

    msg = getattr(event_obj, "message", None)
    if msg:
        event_data["message"] = {
            "message_id": _getattr_text(msg, "message_id"),
            "chat_id": _getattr_text(msg, "chat_id"),
            "chat_type": _getattr_text(msg, "chat_type"),
            "message_type": _getattr_text(msg, "message_type"),
            "content": _getattr_text(msg, "content", "{}") or "{}",
            "root_id": _getattr_text(msg, "root_id"),
            "create_time": _getattr_text(msg, "create_time"),
            "mentions": _normalize_mentions(getattr(msg, "mentions", None)),
        }

    sender = getattr(event_obj, "sender", None)
    if sender:
        event_data["sender"]["sender_type"] = _getattr_text(sender, "sender_type")
        sender_id = getattr(sender, "sender_id", None)
        if sender_id:
            event_data["sender"]["sender_id"] = {
                "open_id": _getattr_text(sender_id, "open_id"),
                "user_id": _getattr_text(sender_id, "user_id"),
            }

    return event_data
