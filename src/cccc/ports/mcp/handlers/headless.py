from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ..common import _call_daemon_or_raise


def headless_status(*, group_id: str, actor_id: str) -> Dict[str, Any]:
    """Get headless session status."""
    return _call_daemon_or_raise(
        {"op": "headless_status", "args": {"group_id": group_id, "actor_id": actor_id}}
    )


def headless_set_status(
    *, group_id: str, actor_id: str, status: str, task_id: Optional[str] = None
) -> Dict[str, Any]:
    """Update headless session status."""
    return _call_daemon_or_raise(
        {
            "op": "headless_set_status",
            "args": {"group_id": group_id, "actor_id": actor_id, "status": status, "task_id": task_id},
        }
    )


def headless_ack_message(*, group_id: str, actor_id: str, message_id: str) -> Dict[str, Any]:
    """Acknowledge processed message."""
    return _call_daemon_or_raise(
        {
            "op": "headless_ack_message",
            "args": {"group_id": group_id, "actor_id": actor_id, "message_id": message_id},
        }
    )


def _handle_headless_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
    resolve_self_actor_id: Callable[[Dict[str, Any]], str],
    headless_status_fn: Callable[..., Dict[str, Any]],
    headless_set_status_fn: Callable[..., Dict[str, Any]],
    headless_ack_message_fn: Callable[..., Dict[str, Any]],
) -> Optional[Dict[str, Any]]:
    if name == "cccc_headless_status":
        gid = resolve_group_id(arguments)
        aid = resolve_self_actor_id(arguments)
        return headless_status_fn(group_id=gid, actor_id=aid)

    if name == "cccc_headless_set_status":
        gid = resolve_group_id(arguments)
        aid = resolve_self_actor_id(arguments)
        return headless_set_status_fn(
            group_id=gid,
            actor_id=aid,
            status=str(arguments.get("status") or ""),
            task_id=arguments.get("task_id"),
        )

    if name == "cccc_headless_ack_message":
        gid = resolve_group_id(arguments)
        aid = resolve_self_actor_id(arguments)
        return headless_ack_message_fn(
            group_id=gid, actor_id=aid, message_id=str(arguments.get("message_id") or "")
        )

    return None

