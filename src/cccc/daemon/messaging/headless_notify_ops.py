"""Fallback inbox notifications for headless chat recipients."""

from __future__ import annotations

from typing import Any, Callable, Optional

from ...contracts.v1 import SystemNotifyData
from ...kernel.actors import find_actor


def notify_headless_targets(
    *,
    group: Any,
    by: str,
    event_id: str,
    priority: str,
    reply_required: bool,
    event: dict[str, Any],
    emit_notify: Callable[..., Any],
    target_resolver: Callable[..., list[str]],
    skip_actor_ids: Optional[set[str]] = None,
) -> None:
    """Notify headless targets that were not directly delivered the message."""
    try:
        targets = target_resolver(group, event=event, by=by)
        skip_ids = {
            str(item).strip() for item in (skip_actor_ids or set()) if str(item).strip()
        }
        if reply_required:
            notify_title = "Need reply"
            notify_priority = "urgent" if priority == "attention" else "high"
        else:
            notify_title = (
                "Needs acknowledgement" if priority == "attention" else "New message"
            )
            notify_priority = "urgent" if priority == "attention" else "high"
        for actor_id in targets:
            if actor_id in skip_ids:
                continue
            actor = find_actor(group, actor_id)
            if (
                isinstance(actor, dict)
                and str(actor.get("runtime") or "").strip().lower() == "web_model"
            ):
                continue
            emit_notify(
                group,
                by="system",
                notify=SystemNotifyData(
                    kind="info",
                    priority=notify_priority,
                    title=notify_title,
                    message=f"New message from {by}. Check your inbox.",
                    target_actor_id=actor_id,
                    requires_ack=False,
                    context={"event_id": event_id, "from": by},
                ),
            )
    except Exception:
        pass
