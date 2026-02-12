"""Actor membership operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import list_actors, remove_actor
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_actor_permission
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_actor_remove(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    throttle_clear_actor: Callable[[str, str], None],
    delete_actor_private_env: Callable[[str, str], None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    before_foreman = foreman_id(group)
    try:
        require_actor_permission(group, by=by, action="actor.remove", target_actor_id=actor_id)
        remove_actor(group, actor_id)
        pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
        remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
        headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
        remove_headless_state(group.group_id, actor_id)
        throttle_clear_actor(group.group_id, actor_id)
        delete_actor_private_env(group.group_id, actor_id)
    except Exception as e:
        return _error("actor_remove_failed", str(e))

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

    event = append_event(
        group.ledger_path,
        kind="actor.remove",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"actor_id": actor_id},
    )
    maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    return DaemonResponse(ok=True, result={"actor_id": actor_id, "event": event})


def try_handle_actor_membership_op(
    op: str,
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    throttle_clear_actor: Callable[[str, str], None],
    delete_actor_private_env: Callable[[str, str], None],
) -> Optional[DaemonResponse]:
    if op == "actor_remove":
        return handle_actor_remove(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
            throttle_clear_actor=throttle_clear_actor,
            delete_actor_private_env=delete_actor_private_env,
        )
    return None
