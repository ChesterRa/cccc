"""Maintenance and relay operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import resolve_recipient_tokens
from ...kernel.group import load_group
from ...kernel.ledger_retention import compact as compact_ledger
from ...kernel.ledger_retention import snapshot as snapshot_ledger
from ...kernel.permissions import require_group_permission
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_term_resize(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    cols_raw = args.get("cols")
    rows_raw = args.get("rows")
    try:
        cols = int(cols_raw) if isinstance(cols_raw, int) else int(str(cols_raw or "0"))
    except Exception:
        cols = 0
    try:
        rows = int(rows_raw) if isinstance(rows_raw, int) else int(str(rows_raw or "0"))
    except Exception:
        rows = 0
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    pty_runner.SUPERVISOR.resize(group_id=group_id, actor_id=actor_id, cols=cols, rows=rows)
    return DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id, "cols": cols, "rows": rows})


def handle_ledger_snapshot(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    reason = str(args.get("reason") or "manual").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.update")
        snapshot = snapshot_ledger(group, reason=reason)
    except Exception as e:
        return _error("ledger_snapshot_failed", str(e))
    return DaemonResponse(ok=True, result={"snapshot": snapshot})


def handle_ledger_compact(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    reason = str(args.get("reason") or "auto").strip()
    force = coerce_bool(args.get("force"), default=False)
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.update")
        result = compact_ledger(group, reason=reason, force=force)
    except Exception as e:
        return _error("ledger_compact_failed", str(e))
    return DaemonResponse(ok=True, result=result)


def handle_send_cross_group(
    args: Dict[str, Any],
    *,
    dispatch_send: Callable[[str, Dict[str, Any]], Tuple[DaemonResponse, bool]],
) -> DaemonResponse:
    src_group_id = str(args.get("group_id") or "").strip()
    dst_group_id = str(args.get("dst_group_id") or "").strip()
    text = str(args.get("text") or "")
    by = str(args.get("by") or "user").strip() or "user"
    priority = str(args.get("priority") or "normal").strip() or "normal"
    reply_required = coerce_bool(args.get("reply_required"), default=False)
    to_raw = args.get("to")
    dst_to_tokens: list[str] = []
    if isinstance(to_raw, list):
        dst_to_tokens = [str(item).strip() for item in to_raw if isinstance(item, str) and str(item).strip()]

    attachments_raw = args.get("attachments")
    if attachments_raw:
        return _error("attachments_not_supported", "attachments are not supported for cross-group messages yet")
    if priority not in ("normal", "attention"):
        return _error("invalid_priority", "priority must be 'normal' or 'attention'")
    if not src_group_id:
        return _error("missing_group_id", "missing group_id")
    if not dst_group_id:
        return _error("missing_dst_group_id", "missing dst_group_id")
    if src_group_id == dst_group_id:
        return _error("invalid_dst_group_id", "dst_group_id must be different from group_id")

    src_group = load_group(src_group_id)
    if src_group is None:
        return _error("group_not_found", f"group not found: {src_group_id}")
    dst_group = load_group(dst_group_id)
    if dst_group is None:
        return _error("group_not_found", f"group not found: {dst_group_id}")

    dst_to_canon: list[str] = []
    if dst_to_tokens:
        try:
            dst_to_canon = resolve_recipient_tokens(dst_group, dst_to_tokens)
        except Exception as e:
            return _error("invalid_recipient", str(e))

    src_resp, _ = dispatch_send(
        "send",
        {
            "group_id": src_group_id,
            "text": text,
            "by": by,
            "to": ["user"],
            "priority": priority,
            "reply_required": reply_required,
            "dst_group_id": dst_group_id,
            "dst_to": dst_to_canon,
        },
    )
    if not src_resp.ok:
        return src_resp

    src_event = src_resp.result.get("event")
    src_event_id = str((src_event or {}).get("id") or "").strip() if isinstance(src_event, dict) else ""
    if not src_event_id:
        return _error("send_failed", "missing source event id")

    dst_resp, _ = dispatch_send(
        "send",
        {
            "group_id": dst_group_id,
            "text": text,
            "by": by,
            "to": dst_to_canon,
            "priority": priority,
            "reply_required": reply_required,
            "src_group_id": src_group_id,
            "src_event_id": src_event_id,
        },
    )
    if not dst_resp.ok:
        return dst_resp

    return DaemonResponse(ok=True, result={"src_event": src_event, "dst_event": dst_resp.result.get("event")})


def try_handle_maintenance_op(
    op: str,
    args: Dict[str, Any],
    *,
    dispatch_send: Optional[Callable[[str, Dict[str, Any]], Tuple[DaemonResponse, bool]]] = None,
) -> Optional[DaemonResponse]:
    if op == "term_resize":
        return handle_term_resize(args)
    if op == "ledger_snapshot":
        return handle_ledger_snapshot(args)
    if op == "ledger_compact":
        return handle_ledger_compact(args)
    if op == "send_cross_group":
        if dispatch_send is None:
            return _error("internal_error", "dispatch_send callback not configured")
        return handle_send_cross_group(args, dispatch_send=dispatch_send)
    return None
