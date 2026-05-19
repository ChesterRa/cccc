"""Idempotency helpers for chat message writes."""

from __future__ import annotations

import json
from typing import Any, Dict, Optional

from .ledger import read_last_lines


def find_existing_reply_result(
    group: Any,
    *,
    client_id: str,
    by: str = "",
    reply_to: str = "",
) -> Optional[Dict[str, Any]]:
    needle = str(client_id or "").strip()
    if not needle:
        return None
    sender = str(by or "").strip()
    target = str(reply_to or "").strip()
    try:
        lines = read_last_lines(group.ledger_path, 800)
    except Exception:
        return None
    for raw_line in reversed(lines):
        try:
            event = json.loads(raw_line)
        except Exception:
            continue
        if not isinstance(event, dict) or str(event.get("kind") or "") != "chat.message":
            continue
        if sender and str(event.get("by") or "").strip() != sender:
            continue
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if str(data.get("client_id") or "").strip() != needle:
            continue
        if target and str(data.get("reply_to") or "").strip() != target:
            continue
        return {
            "event": event,
            "event_id": str(event.get("id") or "").strip(),
            "replayed": True,
            "message_sent": True,
            "partial_failure": False,
        }
    return None
