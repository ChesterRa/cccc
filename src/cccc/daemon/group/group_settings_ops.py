"""Group settings operations for daemon."""

from __future__ import annotations

from typing import Any, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.messaging import get_default_send_to
from ...kernel.permissions import require_group_permission
from ...kernel.terminal_transcript import apply_terminal_transcript_patch, get_terminal_transcript_settings
from ...util.conv import coerce_bool


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _safe_int(value: Any, *, default: int, min_value: int = 0, max_value: Optional[int] = None) -> int:
    try:
        out = int(value)
    except Exception:
        out = int(default)
    if out < int(min_value):
        out = int(min_value)
    if max_value is not None and out > int(max_value):
        out = int(max_value)
    return out


def handle_group_settings_update(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    messaging_keys = {"default_send_to"}
    delivery_keys = {"min_interval_seconds", "auto_mark_on_delivery"}
    automation_keys = {
        "nudge_after_seconds",
        "reply_required_nudge_after_seconds",
        "attention_ack_nudge_after_seconds",
        "unread_nudge_after_seconds",
        "nudge_digest_min_interval_seconds",
        "nudge_max_repeats_per_obligation",
        "nudge_escalate_after_repeats",
        "actor_idle_timeout_seconds",
        "keepalive_delay_seconds",
        "keepalive_max_per_actor",
        "silence_timeout_seconds",
        "help_nudge_interval_seconds",
        "help_nudge_min_messages",
    }
    terminal_transcript_keys = {
        "terminal_transcript_visibility",
        "terminal_transcript_notify_tail",
        "terminal_transcript_notify_lines",
    }
    allowed = messaging_keys | delivery_keys | automation_keys | terminal_transcript_keys

    unknown = set(patch.keys()) - allowed
    if unknown:
        return _error("invalid_patch", "invalid patch keys", details={"unknown_keys": sorted(unknown)})
    if not patch:
        return _error("invalid_patch", "empty patch")
    if "default_send_to" in patch:
        value = str(patch.get("default_send_to") or "").strip()
        if value not in ("foreman", "broadcast"):
            return _error(
                "invalid_patch",
                "default_send_to must be 'foreman' or 'broadcast'",
                details={"default_send_to": value},
            )
    try:
        require_group_permission(group, by=by, action="group.settings_update")

        messaging_patch = {k: v for k, v in patch.items() if k in messaging_keys}
        if messaging_patch:
            messaging = group.doc.get("messaging") if isinstance(group.doc.get("messaging"), dict) else {}
            messaging["default_send_to"] = str(messaging_patch.get("default_send_to") or "foreman").strip()
            group.doc["messaging"] = messaging

        delivery_patch = {k: v for k, v in patch.items() if k in delivery_keys}
        if delivery_patch:
            delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
            for key, value in delivery_patch.items():
                if key == "auto_mark_on_delivery":
                    delivery[key] = coerce_bool(value, default=False)
                else:
                    delivery[key] = int(value)
            group.doc["delivery"] = delivery

        automation_patch = {k: v for k, v in patch.items() if k in automation_keys}
        if automation_patch:
            automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
            for key, value in automation_patch.items():
                automation[key] = int(value)
            group.doc["automation"] = automation

        tt_patch: Dict[str, Any] = {}
        if "terminal_transcript_visibility" in patch:
            tt_patch["visibility"] = patch.get("terminal_transcript_visibility")
        if "terminal_transcript_notify_tail" in patch:
            tt_patch["notify_tail"] = patch.get("terminal_transcript_notify_tail")
        if "terminal_transcript_notify_lines" in patch:
            tt_patch["notify_lines"] = patch.get("terminal_transcript_notify_lines")
        if tt_patch:
            apply_terminal_transcript_patch(group.doc, tt_patch)

        group.save()
    except Exception as e:
        return _error("group_settings_update_failed", str(e))

    automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
    delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
    tt = get_terminal_transcript_settings(group.doc)
    settings = {
        "default_send_to": get_default_send_to(group.doc),
        "nudge_after_seconds": _safe_int(automation.get("nudge_after_seconds", 300), default=300, min_value=0),
        "reply_required_nudge_after_seconds": _safe_int(
            automation.get("reply_required_nudge_after_seconds", 300),
            default=300,
            min_value=0,
        ),
        "attention_ack_nudge_after_seconds": _safe_int(
            automation.get("attention_ack_nudge_after_seconds", 600),
            default=600,
            min_value=0,
        ),
        "unread_nudge_after_seconds": _safe_int(automation.get("unread_nudge_after_seconds", 900), default=900, min_value=0),
        "nudge_digest_min_interval_seconds": _safe_int(
            automation.get("nudge_digest_min_interval_seconds", 120),
            default=120,
            min_value=0,
        ),
        "nudge_max_repeats_per_obligation": _safe_int(
            automation.get("nudge_max_repeats_per_obligation", 3),
            default=3,
            min_value=0,
        ),
        "nudge_escalate_after_repeats": _safe_int(
            automation.get("nudge_escalate_after_repeats", 2),
            default=2,
            min_value=0,
        ),
        "actor_idle_timeout_seconds": _safe_int(
            automation.get("actor_idle_timeout_seconds", 600),
            default=600,
            min_value=0,
        ),
        "keepalive_delay_seconds": _safe_int(automation.get("keepalive_delay_seconds", 120), default=120, min_value=0),
        "keepalive_max_per_actor": _safe_int(automation.get("keepalive_max_per_actor", 3), default=3, min_value=0),
        "silence_timeout_seconds": _safe_int(automation.get("silence_timeout_seconds", 600), default=600, min_value=0),
        "help_nudge_interval_seconds": _safe_int(
            automation.get("help_nudge_interval_seconds", 600),
            default=600,
            min_value=0,
        ),
        "help_nudge_min_messages": _safe_int(automation.get("help_nudge_min_messages", 10), default=10, min_value=0),
        "min_interval_seconds": _safe_int(delivery.get("min_interval_seconds", 0), default=0, min_value=0),
        "auto_mark_on_delivery": coerce_bool(delivery.get("auto_mark_on_delivery"), default=False),
        "terminal_transcript_visibility": str(tt.get("visibility") or "foreman"),
        "terminal_transcript_notify_tail": coerce_bool(tt.get("notify_tail"), default=False),
        "terminal_transcript_notify_lines": _safe_int(
            tt.get("notify_lines", 20),
            default=20,
            min_value=1,
            max_value=80,
        ),
    }

    event = append_event(
        group.ledger_path,
        kind="group.settings_update",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"patch": dict(patch)},
    )
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "settings": settings, "event": event})


def try_handle_group_settings_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "group_settings_update":
        return handle_group_settings_update(args)
    return None
