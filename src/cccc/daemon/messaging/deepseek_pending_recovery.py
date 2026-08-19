"""Restart recovery for DeepSeek's in-memory delivery queue."""

from __future__ import annotations

from typing import Any


def recover_pending_messages(group: Any, *, actor_id: str, limit: int = 256) -> int:
    """Recover durable completions, then enqueue the remaining unread prefix."""
    from .deepseek_delivery import recover_durable_terminals
    from .delivery import (
        THROTTLE,
        refill_unread_runtime_messages,
        request_flush_pending_messages,
    )

    aid = str(actor_id or "").strip()
    if not aid:
        return 0
    completed = recover_durable_terminals(group, actor_id=aid, limit=limit)
    recovered = refill_unread_runtime_messages(group, actor_id=aid, limit=limit)
    if recovered or THROTTLE.has_pending(str(group.group_id), aid):
        request_flush_pending_messages(group, actor_id=aid)
    return completed + recovered
