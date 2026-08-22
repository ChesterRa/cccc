"""System notification operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse, SystemNotifyData
from ...kernel.group import load_group
from .delivery import emit_system_notify


def _error(
    code: str, message: str, *, details: Optional[Dict[str, Any]] = None
) -> DaemonResponse:
    return DaemonResponse(
        ok=False, error=DaemonError(code=code, message=message, details=(details or {}))
    )


def handle_system_notify(
    args: Dict[str, Any],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "system").strip()
    kind = str(args.get("kind") or "info").strip()
    priority = str(args.get("priority") or "normal").strip()
    title = str(args.get("title") or "").strip()
    message = str(args.get("message") or "").strip()
    target_actor_id = str(args.get("target_actor_id") or "").strip() or None
    im_visibility = str(args.get("im_visibility") or "internal").strip().lower()
    context = args.get("context") if isinstance(args.get("context"), dict) else {}

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if "requires_ack" in args:
        return _error(
            "unsupported_notify_field",
            "system notifications do not support generic acknowledgement",
        )
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    valid_kinds = {
        "nudge",
        "keepalive",
        "help_nudge",
        "actor_idle",
        "silence_check",
        "auto_idle",
        "automation",
        "status_change",
        "error",
        "info",
    }
    valid_priorities = {"low", "normal", "high", "urgent"}
    if kind not in valid_kinds:
        kind = "info"
    if priority not in valid_priorities:
        priority = "normal"
    if im_visibility not in {"internal", "public"}:
        im_visibility = "internal"

    notify = SystemNotifyData(
        kind=kind,
        priority=priority,
        title=title,
        message=message,
        target_actor_id=target_actor_id,
        im_visibility=im_visibility,
        context=context,
    )
    event = emit_system_notify(group, by=by, notify=notify)
    return DaemonResponse(ok=True, result={"event": event})


def try_handle_system_notify_op(
    op: str,
    args: Dict[str, Any],
) -> Optional[DaemonResponse]:
    if op == "system_notify":
        return handle_system_notify(args)
    return None
