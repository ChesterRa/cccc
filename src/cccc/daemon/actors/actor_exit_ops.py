"""Shared actor runtime-exit persistence helpers."""

from __future__ import annotations

import logging

from ...kernel.actors import find_actor, is_internal_actor
from ...kernel.events import publish_event
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...util.conv import coerce_bool

logger = logging.getLogger(__name__)


def persist_actor_process_exit_stopped(*, group_id: str, actor_id: str, runner: str) -> bool:
    """Record a visible actor's natural runtime exit without changing desired lifecycle.

    Explicit actor_stop and daemon shutdown own their own lifecycle paths. This helper is only
    for a runtime process exiting by itself while the daemon is still the truth owner.
    """
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        return False

    group = load_group(gid)
    if group is None:
        return False
    actor = find_actor(group, aid)
    if not isinstance(actor, dict) or is_internal_actor(actor):
        return False
    if not coerce_bool(actor.get("enabled"), default=True):
        return False

    try:
        event = append_event(
            group.ledger_path,
            kind="actor.stop",
            group_id=group.group_id,
            scope_key="",
            by="system",
            data={
                "actor_id": aid,
                "runner": str(runner or "").strip() or "unknown",
                "reason": "process_exit",
            },
        )
    except Exception as exc:
        logger.debug("failed to record actor process exit for %s/%s: %s", gid, aid, exc)
        return False

    try:
        publish_event(
            "actor.stop",
            {
                "group_id": group.group_id,
                "actor_id": aid,
                "event_id": str(event.get("id") or "").strip(),
                "reason": "process_exit",
            },
        )
    except Exception as exc:
        logger.debug("failed to publish actor process exit for %s/%s: %s", gid, aid, exc)

    return True
