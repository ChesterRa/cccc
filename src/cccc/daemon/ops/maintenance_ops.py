"""Maintenance and relay operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional, Tuple
from uuid import uuid4

from ...contracts.v1 import DaemonError, DaemonResponse
from ..federation.ops import handle_remote_send
from ..federation.route_lookup import resolve_remote_group_route
from ...kernel.actors import resolve_recipient_tokens
from ...kernel.group import load_group
from ...kernel.ledger_retention import compact as compact_ledger
from ...kernel.ledger_retention import snapshot as snapshot_ledger
from ...kernel.permissions import require_group_permission
from ...kernel.recipient_syntax import (
    CROSS_GROUP_HASH_RECIPIENT_MESSAGE,
    cross_group_recipient_tokens_or_default,
    group_not_found_with_resolution_hint,
    has_hash_recipient_token,
)
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
    if cols < 10 or rows < 2:
        return _error("invalid_size", f"cols={cols} rows={rows} too small")
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
    if attachments_raw is not None and not isinstance(attachments_raw, list):
        return _error("invalid_attachments", "attachments must be a list")
    attachments = attachments_raw if isinstance(attachments_raw, list) else []
    refs_raw = args.get("refs")
    if isinstance(refs_raw, list) and any(isinstance(item, dict) for item in refs_raw):
        return _error("refs_not_supported", "quoted refs are not supported for cross-group messages yet")
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

    remote_route = resolve_remote_group_route(group_id=src_group_id, remote_group_id=dst_group_id)
    if remote_route is not None:
        dst_to_tokens = cross_group_recipient_tokens_or_default(dst_to_tokens)
        if has_hash_recipient_token(dst_to_tokens):
            return _error("invalid_recipient_syntax", CROSS_GROUP_HASH_RECIPIENT_MESSAGE)
        src_resp, _ = dispatch_send(
            "send",
            {
                "group_id": src_group_id,
                "text": text,
                "by": by,
                "to": ["user"],
                "attachments": attachments,
                "priority": priority,
                "reply_required": reply_required,
                "dst_group_id": dst_group_id,
                "dst_to": dst_to_tokens,
            },
        )
        if not src_resp.ok:
            return src_resp

        src_event = src_resp.result.get("event")
        src_event_id = str((src_event or {}).get("id") or "").strip() if isinstance(src_event, dict) else ""
        idempotency_key = f"webmsg_{src_event_id}" if src_event_id else f"webmsg_{uuid4().hex}"
        remote_resp = handle_remote_send(
            {
                "group_id": src_group_id,
                "registration_id": remote_route.registration_id,
                "idempotency_key": idempotency_key,
                "source_event_id": src_event_id,
                "payload": {
                    "text": text,
                    "to": dst_to_tokens,
                    "priority": priority,
                    "reply_required": reply_required,
                    "refs": [],
                    "attachments": attachments,
                },
            }
        )
        if not remote_resp.ok:
            return remote_resp
        receipt = remote_resp.result.get("receipt") if isinstance(remote_resp.result, dict) else {}
        if isinstance(receipt, dict) and str(receipt.get("status") or "").strip() == "failed":
            error = receipt.get("error") if isinstance(receipt.get("error"), dict) else {}
            return _error(
                str(error.get("code") or "remote_send_failed"),
                str(error.get("message") or "remote send failed"),
                details={
                    "remote_group_id": remote_route.remote_group_id,
                    "registration_id": remote_route.registration_id,
                    "idempotency_key": idempotency_key,
                    "receipt_status": "failed",
                },
            )
        return DaemonResponse(
            ok=True,
            result={
                "src_event": src_event,
                "remote_group_id": remote_route.remote_group_id,
                "remote_send": remote_resp.result,
            },
        )

    dst_group = load_group(dst_group_id)
    if dst_group is None:
        return _error("group_not_found", group_not_found_with_resolution_hint(dst_group_id))
    if attachments:
        return _error("attachments_not_supported", "attachments are only supported for remote Group Bridge messages")

    dst_to_tokens = cross_group_recipient_tokens_or_default(dst_to_tokens)
    if has_hash_recipient_token(dst_to_tokens):
        return _error("invalid_recipient_syntax", CROSS_GROUP_HASH_RECIPIENT_MESSAGE)
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
