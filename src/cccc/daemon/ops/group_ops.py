"""Core group operation handlers for daemon."""

from __future__ import annotations

import copy
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.active import load_active, set_active_group_id
from ...kernel.group import delete_group, detach_scope_from_group, load_group, set_active_scope, update_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_group_permission
from ...kernel.registry import load_registry
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...kernel.scope import detect_scope


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _redact_group_doc(doc: Dict[str, Any]) -> Dict[str, Any]:
    """Redact secrets from group doc before returning to clients."""
    try:
        out = copy.deepcopy(doc)
    except Exception:
        out = dict(doc or {})

    im = out.get("im")
    if isinstance(im, dict):
        im.pop("token", None)
        im.pop("bot_token", None)
        im.pop("app_token", None)
    return out


def handle_group_show(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    return DaemonResponse(ok=True, result={"group": _redact_group_doc(group.doc)})


def handle_group_update(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    allowed = {"title", "topic"}
    unknown = set(patch.keys()) - allowed
    if unknown:
        return _error("invalid_patch", "invalid patch keys", details={"unknown_keys": sorted(unknown)})
    if not patch:
        return _error("invalid_patch", "empty patch")
    try:
        require_group_permission(group, by=by, action="group.update")
        reg = load_registry()
        group = update_group(reg, group, patch=dict(patch))
    except Exception as e:
        return _error("group_update_failed", str(e))
    event = append_event(
        group.ledger_path,
        kind="group.update",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"patch": dict(patch)},
    )
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "group": _redact_group_doc(group.doc), "event": event})


def handle_group_detach_scope(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    scope_key = str(args.get("scope_key") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not scope_key:
        return _error("missing_scope_key", "missing scope_key")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.detach_scope")
        reg = load_registry()
        group = detach_scope_from_group(reg, group, scope_key=scope_key)
    except Exception as e:
        return _error("group_detach_scope_failed", str(e))
    event = append_event(
        group.ledger_path,
        kind="group.detach_scope",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data={"scope_key": scope_key},
    )
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "event": event})


def handle_group_delete(
    args: Dict[str, Any],
    *,
    stop_im_bridges_for_group: Callable[[str], None],
    delete_group_private_env: Callable[[str], None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.delete")
        stop_im_bridges_for_group(group_id)
        pty_runner.SUPERVISOR.stop_group(group_id=group_id)
        headless_runner.SUPERVISOR.stop_group(group_id=group_id)
        delete_group_private_env(group_id)
        reg = load_registry()
        delete_group(reg, group_id=group_id)
        active = load_active()
        if str(active.get("active_group_id") or "") == group_id:
            set_active_group_id("")
    except Exception as e:
        return _error("group_delete_failed", str(e))
    return DaemonResponse(ok=True, result={"group_id": group_id})


def handle_group_use(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    path = Path(str(args.get("path") or "."))
    scope = detect_scope(path)
    reg = load_registry()
    try:
        group = set_active_scope(reg, group, scope_key=scope.scope_key)
    except ValueError as e:
        return _error(
            "scope_not_attached",
            str(e),
            details={"hint": "attach scope first (cccc attach <path> --group <id>)"},
        )
    event = append_event(
        group.ledger_path,
        kind="group.set_active_scope",
        group_id=group.group_id,
        scope_key=scope.scope_key,
        by=str(args.get("by") or "cli"),
        data={"path": scope.url},
    )
    return DaemonResponse(
        ok=True,
        result={"group_id": group.group_id, "active_scope_key": scope.scope_key, "event": event},
    )


def try_handle_group_core_op(
    op: str,
    args: Dict[str, Any],
    *,
    stop_im_bridges_for_group: Optional[Callable[[str], None]] = None,
    delete_group_private_env: Optional[Callable[[str], None]] = None,
) -> Optional[DaemonResponse]:
    if op == "group_show":
        return handle_group_show(args)
    if op == "group_update":
        return handle_group_update(args)
    if op == "group_detach_scope":
        return handle_group_detach_scope(args)
    if op == "group_delete":
        if stop_im_bridges_for_group is None or delete_group_private_env is None:
            return _error("internal_error", "group_delete callbacks not configured")
        return handle_group_delete(
            args,
            stop_im_bridges_for_group=stop_im_bridges_for_group,
            delete_group_private_env=delete_group_private_env,
        )
    if op == "group_use":
        return handle_group_use(args)
    return None
