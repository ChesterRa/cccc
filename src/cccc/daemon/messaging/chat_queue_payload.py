"""Build canonical queued-chat payloads for runtime delivery."""

from __future__ import annotations

from typing import Any


def build_chat_queue_payload(
    *,
    actor_id: str,
    event_id: str,
    by: str,
    effective_to: list[str],
    delivery_text: str,
    event_ts: str,
    reply_to: str,
    quote_text: str,
    source_platform: str,
    source_user_name: str,
    source_user_id: str,
    deduplicate_by_event_id: bool,
) -> dict[str, Any]:
    """Return the canonical queue fields shared by PTY and DeepSeek delivery."""
    payload: dict[str, Any] = {
        "actor_id": actor_id,
        "event_id": event_id,
        "by": by,
        "to": effective_to,
        "text": delivery_text,
        "ts": event_ts,
    }
    if reply_to:
        payload.update(reply_to=reply_to, quote_text=quote_text)
    else:
        payload.update(
            source_platform=source_platform or None,
            source_user_name=source_user_name or None,
            source_user_id=source_user_id or None,
        )
    if deduplicate_by_event_id:
        payload["deduplicate_by_event_id"] = True
    return payload
