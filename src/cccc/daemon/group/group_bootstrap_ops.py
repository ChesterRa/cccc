"""Group bootstrap operation handlers for daemon."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.group import attach_scope_to_group, create_group, ensure_group_for_scope, load_group
from ...kernel.ledger import append_event
from ...kernel.registry import load_registry
from ...kernel.scope import detect_scope


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_attach(args: Dict[str, Any]) -> DaemonResponse:
    path = Path(str(args.get("path") or "."))
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


def handle_group_template_import_replace(
    args: Dict[str, Any],
    *,
    group_template_import_replace: Callable[[Dict[str, Any]], DaemonResponse],
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[[Any, str], None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    before_foreman_id = ""
    if group_id:
        before_group = load_group(group_id)
        if before_group is not None:
            before_foreman_id = foreman_id(before_group)
    resp = group_template_import_replace(args)
    if resp.ok and group_id:
        after_group = load_group(group_id)
        if after_group is not None:
            maybe_reset_automation_on_foreman_change(after_group, before_foreman_id)
    return resp


def try_handle_group_bootstrap_op(
    op: str,
    args: Dict[str, Any],
    *,
    group_create_from_template: Optional[Callable[[Dict[str, Any]], DaemonResponse]] = None,
    group_template_export: Optional[Callable[[Dict[str, Any]], DaemonResponse]] = None,
    group_template_preview: Optional[Callable[[Dict[str, Any]], DaemonResponse]] = None,
    group_template_import_replace: Optional[Callable[[Dict[str, Any]], DaemonResponse]] = None,
    foreman_id: Optional[Callable[[Any], str]] = None,
    maybe_reset_automation_on_foreman_change: Optional[Callable[[Any, str], None]] = None,
) -> Optional[DaemonResponse]:
    if op == "attach":
        return handle_attach(args)
    if op == "group_create":
        return handle_group_create(args)
    if op == "group_create_from_template":
        if group_create_from_template is None:
            return _error("internal_error", "group_create_from_template callback not configured")
        return group_create_from_template(args)
    if op == "group_template_export":
        if group_template_export is None:
            return _error("internal_error", "group_template_export callback not configured")
        return group_template_export(args)
    if op == "group_template_preview":
        if group_template_preview is None:
            return _error("internal_error", "group_template_preview callback not configured")
        return group_template_preview(args)
    if op == "group_template_import_replace":
        if (
            group_template_import_replace is None
            or foreman_id is None
            or maybe_reset_automation_on_foreman_change is None
        ):
            return _error("internal_error", "group_template_import_replace callbacks not configured")
        return handle_group_template_import_replace(
            args,
            group_template_import_replace=group_template_import_replace,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
        )
    return None
