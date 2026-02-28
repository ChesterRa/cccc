from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ....util.conv import coerce_bool
from ..common import MCPError, _call_daemon_or_raise


def notify_send(
    *,
    group_id: str,
    actor_id: str,
    kind: str,
    title: str,
    message: str,
    target_actor_id: Optional[str] = None,
    priority: str = "normal",
    requires_ack: bool = False,
) -> Dict[str, Any]:
    """Send system notification."""
    return _call_daemon_or_raise(
        {
            "op": "system_notify",
            "args": {
                "group_id": group_id,
                "by": actor_id,
                "kind": kind,
                "priority": priority,
                "title": title,
                "message": message,
                "target_actor_id": target_actor_id,
                "requires_ack": requires_ack,
            },
        }
    )


def notify_ack(*, group_id: str, actor_id: str, notify_event_id: str) -> Dict[str, Any]:
    """Acknowledge system notification."""
    return _call_daemon_or_raise(
        {
            "op": "notify_ack",
            "args": {"group_id": group_id, "actor_id": actor_id, "notify_event_id": notify_event_id, "by": actor_id},
        }
    )


def _handle_notify_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
    resolve_self_actor_id: Callable[[Dict[str, Any]], str],
    notify_send_fn: Callable[..., Dict[str, Any]],
    notify_ack_fn: Callable[..., Dict[str, Any]],
    coerce_bool_fn: Callable[..., bool] = coerce_bool,
) -> Optional[Dict[str, Any]]:
    if name == "cccc_notify":
        gid = resolve_group_id(arguments)
        aid = resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "send").strip().lower()
        if action == "send":
            return notify_send_fn(
                group_id=gid,
                actor_id=aid,
                kind=str(arguments.get("kind") or "info"),
                title=str(arguments.get("title") or ""),
                message=str(arguments.get("message") or ""),
                target_actor_id=arguments.get("target_actor_id"),
                priority=str(arguments.get("priority") or "normal"),
                requires_ack=coerce_bool_fn(arguments.get("requires_ack"), default=False),
            )
        if action == "ack":
            return notify_ack_fn(
                group_id=gid,
                actor_id=aid,
                notify_event_id=str(arguments.get("notify_event_id") or ""),
            )
        raise MCPError(
            code="invalid_request",
            message="cccc_notify action must be 'send' or 'ack'",
        )

    return None
