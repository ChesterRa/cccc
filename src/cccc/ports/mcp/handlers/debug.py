from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ....util.conv import coerce_bool
from ..common import MCPError, _call_daemon_or_raise


def debug_snapshot(*, group_id: str, actor_id: str) -> Dict[str, Any]:
    """Get a structured debug snapshot (developer mode only; user+foreman only)."""
    return _call_daemon_or_raise({"op": "debug_snapshot", "args": {"group_id": group_id, "by": actor_id}})


def terminal_tail(
    *,
    group_id: str,
    actor_id: str,
    target_actor_id: str,
    max_chars: int = 8000,
    strip_ansi: bool = True,
) -> Dict[str, Any]:
    """Tail an actor terminal transcript (subject to group policy; may include sensitive stdout/stderr)."""
    return _call_daemon_or_raise(
        {
            "op": "terminal_tail",
            "args": {
                "group_id": group_id,
                "actor_id": str(target_actor_id or ""),
                "by": actor_id,
                "max_chars": int(max_chars or 8000),
                "strip_ansi": bool(strip_ansi),
            },
        }
    )


def debug_tail_logs(*, group_id: str, actor_id: str, component: str, lines: int = 200) -> Dict[str, Any]:
    """Tail CCCC local logs (developer mode only; user+foreman only)."""
    return _call_daemon_or_raise(
        {
            "op": "debug_tail_logs",
            "args": {
                "group_id": group_id,
                "by": actor_id,
                "component": str(component or ""),
                "lines": int(lines or 200),
            },
        }
    )


def _handle_terminal_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
    resolve_self_actor_id: Callable[[Dict[str, Any]], str],
    terminal_tail_fn: Callable[..., Dict[str, Any]],
    coerce_bool_fn: Callable[..., bool] = coerce_bool,
) -> Optional[Dict[str, Any]]:
    if name == "cccc_terminal":
        gid = resolve_group_id(arguments)
        aid = resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "tail").strip().lower()
        if action != "tail":
            raise MCPError(code="invalid_request", message="cccc_terminal action must be 'tail'")
        return terminal_tail_fn(
            group_id=gid,
            actor_id=aid,
            target_actor_id=str(arguments.get("target_actor_id") or ""),
            max_chars=min(max(int(arguments.get("max_chars") or 8000), 1), 100000),
            strip_ansi=coerce_bool_fn(arguments.get("strip_ansi"), default=True),
        )
    return None


def _handle_debug_namespace(
    name: str,
    arguments: Dict[str, Any],
    *,
    resolve_group_id: Callable[[Dict[str, Any]], str],
    resolve_self_actor_id: Callable[[Dict[str, Any]], str],
    debug_snapshot_fn: Callable[..., Dict[str, Any]],
    debug_tail_logs_fn: Callable[..., Dict[str, Any]],
) -> Optional[Dict[str, Any]]:
    if name == "cccc_debug":
        gid = resolve_group_id(arguments)
        aid = resolve_self_actor_id(arguments)
        action = str(arguments.get("action") or "snapshot").strip().lower()
        if action == "snapshot":
            return debug_snapshot_fn(group_id=gid, actor_id=aid)
        if action == "tail_logs":
            return debug_tail_logs_fn(
                group_id=gid,
                actor_id=aid,
                component=str(arguments.get("component") or ""),
                lines=min(max(int(arguments.get("lines") or 200), 1), 10000),
            )
        raise MCPError(
            code="invalid_request",
            message="cccc_debug action must be 'snapshot' or 'tail_logs'",
        )
    return None
