"""Group state operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.group import get_group_state, load_group, set_group_state
from ...kernel.ledger import append_event
from ...kernel.permissions import require_group_permission


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_group_set_state(
    args: Dict[str, Any],
    *,
    automation_on_resume: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str], None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    state = str(args.get("state") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not state:
        return _error("missing_state", "missing state")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.set_state")
        old_state = get_group_state(group)
        group = set_group_state(group, state=state)
        new_state = get_group_state(group)
        if old_state in ("idle", "paused") and new_state == "active":
            try:
                automation_on_resume(group)
            except Exception:
                pass
            try:
                clear_pending_system_notifies(group.group_id)
            except Exception:
                pass
    except Exception as e:
        return _error("group_set_state_failed", str(e))
    event = append_event(
        group.ledger_path,
        kind="group.set_state",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"old_state": old_state, "new_state": new_state},
    )
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "state": new_state, "event": event})


def try_handle_group_state_op(
    op: str,
    args: Dict[str, Any],
    *,
    automation_on_resume: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str], None],
) -> Optional[DaemonResponse]:
    if op == "group_set_state":
        return handle_group_set_state(
            args,
            automation_on_resume=automation_on_resume,
            clear_pending_system_notifies=clear_pending_system_notifies,
        )
    return None
