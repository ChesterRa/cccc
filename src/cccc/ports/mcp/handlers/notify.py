from __future__ import annotations

from typing import Any, Callable, Dict, Optional
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
            },
        }
    )


def _handle_notify_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
    resolve_self_actor_id: Callable[[Dict[str, Any]], str],
    notify_send_fn: Callable[..., Dict[str, Any]],
) -> Optional[Dict[str, Any]]:
    if name == "cccc_notify":
        action = str(arguments.get("action") or "send").strip()
        if action != "send":
            raise MCPError(code="invalid_action", message="cccc_notify action must be send")
        gid = resolve_group_id(arguments)
        aid = resolve_self_actor_id(arguments)
        return notify_send_fn(
            group_id=gid,
            actor_id=aid,
            kind=str(arguments.get("kind") or "info"),
            title=str(arguments.get("title") or ""),
            message=str(arguments.get("message") or ""),
            target_actor_id=arguments.get("target_actor_id"),
            priority=str(arguments.get("priority") or "normal"),
        )

    return None
