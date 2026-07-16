"""Shared actor runtime-exit persistence helpers."""

from __future__ import annotations

import logging

from ...kernel.actors import find_actor, is_internal_actor, list_actors, update_actor
from ...kernel.events import publish_event
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...util.conv import coerce_bool

logger = logging.getLogger(__name__)


def persist_actor_process_exit_stopped(*, group_id: str, actor_id: str, runner: str) -> bool:
    """Persist a visible actor's natural runtime exit as stopped.

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
        update_actor(group, aid, {"enabled": False})
    except Exception as exc:
        logger.debug("failed to persist actor process exit for %s/%s: %s", gid, aid, exc)
        return False

    try:
        any_enabled = any(
            coerce_bool(item.get("enabled"), default=True)
            for item in list_actors(group)
            if isinstance(item, dict) and str(item.get("id") or "").strip()
        )
        if not any_enabled:
            group.doc["running"] = False
            group.save()
    except Exception:
        pass

    try:
        event = append_event(
            group.ledger_path,
            kind="actor.stop",
            group_id=group.group_id,
            scope_key="",
            by="system",
            data={"actor_id": aid, "runner": str(runner or "").strip() or "unknown"},
        )
        publish_event(
            "actor.stop",
            {
                "group_id": group.group_id,
                "actor_id": aid,
                "event_id": str(event.get("id") or "").strip(),
                "reason": "process_exit",
            },
        )
    except Exception:
        pass

    return True
