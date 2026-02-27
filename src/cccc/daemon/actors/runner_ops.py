"""Runner operations for daemon - handles both PTY and headless runners."""
from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Optional, Tuple

from ...contracts.v1 import DaemonResponse, DaemonError
from ...kernel.group import load_group
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.time import utc_now_iso


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _pty_supported() -> bool:
    return bool(getattr(pty_runner, "PTY_SUPPORTED", True))


def _effective_runner_kind(runner_kind: str) -> str:
    rk = str(runner_kind or "").strip() or "pty"
    if rk == "headless":
        return "headless"
    return "pty" if _pty_supported() else "headless"


def handle_headless_status(args: Dict[str, Any]) -> DaemonResponse:
    """Get headless session status."""
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    state = headless_runner.SUPERVISOR.get_state(group_id=group_id, actor_id=actor_id)
    if state is None:
        return _error("session_not_found", f"headless session not found: {actor_id}")

    return DaemonResponse(ok=True, result={"state": state.model_dump()})


def handle_headless_set_status(args: Dict[str, Any]) -> DaemonResponse:
    """Update headless session status (called by agent via MCP)."""
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    status = str(args.get("status") or "").strip()
    task_id = args.get("task_id")
    if task_id is not None:
        task_id = str(task_id).strip() or None

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    if status not in ("idle", "working", "waiting", "stopped"):
        return _error("invalid_status", f"invalid status: {status}")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    ok = headless_runner.SUPERVISOR.set_status(
        group_id=group_id,
        actor_id=actor_id,
        status=status,  # type: ignore
        task_id=task_id,
    )
    if not ok:
        return _error("session_not_found", f"headless session not found: {actor_id}")

    state = headless_runner.SUPERVISOR.get_state(group_id=group_id, actor_id=actor_id)
    return DaemonResponse(ok=True, result={"state": state.model_dump() if state else None})


def handle_headless_ack_message(args: Dict[str, Any]) -> DaemonResponse:
    """Acknowledge processing of a message (called by agent via MCP)."""
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    message_id = str(args.get("message_id") or "").strip()

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    if not message_id:
        return _error("missing_message_id", "missing message_id")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    ok = headless_runner.SUPERVISOR.set_last_message(
        group_id=group_id,
        actor_id=actor_id,
        message_id=message_id,
    )
    if not ok:
        return _error("session_not_found", f"headless session not found: {actor_id}")

    return DaemonResponse(ok=True, result={"message_id": message_id, "acked_at": utc_now_iso()})


def is_actor_running(group_id: str, actor_id: str, runner_kind: str) -> bool:
    """Check if an actor is running (works for both PTY and headless)."""
    if _effective_runner_kind(runner_kind) == "headless":
        return headless_runner.SUPERVISOR.actor_running(group_id, actor_id)
    else:
        return pty_runner.SUPERVISOR.actor_running(group_id, actor_id)


def is_group_running(group_id: str) -> bool:
    """Check if any actor in a group is running (either PTY or headless)."""
    return (
        pty_runner.SUPERVISOR.group_running(group_id)
        or headless_runner.SUPERVISOR.group_running(group_id)
    )


def stop_actor(group_id: str, actor_id: str, runner_kind: str) -> None:
    """Stop an actor (works for both PTY and headless)."""
    if _effective_runner_kind(runner_kind) == "headless":
        headless_runner.SUPERVISOR.stop_actor(group_id=group_id, actor_id=actor_id)
    else:
        pty_runner.SUPERVISOR.stop_actor(group_id=group_id, actor_id=actor_id)


def stop_group(group_id: str) -> None:
    """Stop all actors in a group (both PTY and headless)."""
    pty_runner.SUPERVISOR.stop_group(group_id=group_id)
    headless_runner.SUPERVISOR.stop_group(group_id=group_id)


def stop_all() -> None:
    """Stop all actors (both PTY and headless)."""
    pty_runner.SUPERVISOR.stop_all()
    headless_runner.SUPERVISOR.stop_all()


def try_handle_headless_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "headless_status":
        return handle_headless_status(args)
    if op == "headless_set_status":
        return handle_headless_set_status(args)
    if op == "headless_ack_message":
        return handle_headless_ack_message(args)
    return None
