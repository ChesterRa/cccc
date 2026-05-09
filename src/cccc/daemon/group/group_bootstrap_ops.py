"""Group bootstrap operation handlers for daemon."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.group import attach_scope_to_group, create_group, ensure_group_for_scope, load_group
from ...kernel.ledger import append_event
from ...kernel.registry import load_registry
from ...kernel.scope import detect_scope


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_attach(args: Dict[str, Any]) -> DaemonResponse:
    path = Path(str(args.get("path") or "."))
    if not path.exists():
        try:
            path.mkdir(parents=True, exist_ok=True)
        except Exception as exc:
            return _error("scope_path_create_failed", f"cannot create directory: {exc}")
    scope = detect_scope(path)
    reg = load_registry()
    requested_group_id = str(args.get("group_id") or "").strip()
    if requested_group_id:
        group = load_group(requested_group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {requested_group_id}")
        group = attach_scope_to_group(reg, group, scope, set_active=True)
    else:
        group = ensure_group_for_scope(reg, scope)
    append_event(
        group.ledger_path,
        kind="group.attach",
        group_id=group.group_id,
        scope_key=scope.scope_key,
        by=str(args.get("by") or "cli"),
        data={"url": scope.url, "label": scope.label, "git_remote": scope.git_remote},
    )
    return DaemonResponse(
        ok=True,
        result={"group_id": group.group_id, "scope_key": scope.scope_key, "title": group.doc.get("title")},
    )


def handle_group_create(args: Dict[str, Any]) -> DaemonResponse:
    reg = load_registry()
    title = str(args.get("title") or "working-group")
    topic = str(args.get("topic") or "")
    group = create_group(reg, title=title, topic=topic)
    event = append_event(
        group.ledger_path,
        kind="group.create",
        group_id=group.group_id,
        scope_key="",
        by=str(args.get("by") or "cli"),
        data={"title": group.doc.get("title", ""), "topic": group.doc.get("topic", "")},
    )
    return DaemonResponse(
        ok=True,
        result={"group_id": group.group_id, "title": group.doc.get("title"), "event": event},
    )


def try_handle_group_bootstrap_op(
    op: str,
    args: Dict[str, Any],
) -> Optional[DaemonResponse]:
    if op == "attach":
        return handle_attach(args)
    if op == "group_create":
        return handle_group_create(args)
    return None
