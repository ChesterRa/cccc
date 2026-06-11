"""MCP handlers for federation remote-send tools.

These are thin delegations to daemon ops — the handler never speaks a remote
transport itself; all delivery happens daemon-side. Web/Settings UI is out of
scope for this stage.
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ..common import MCPError, _call_daemon_or_raise


def remote_send(
    *,
    group_id: str,
    actor_id: str,
    registration_id: str,
    text: str,
    idempotency_key: str,
    to: Optional[List[str]] = None,
    priority: str = "normal",
    reply_required: bool = False,
) -> Dict[str, Any]:
    """Enqueue an outbound remote send through the daemon."""
    rid = str(registration_id or "").strip()
    if not rid:
        raise MCPError(code="missing_registration_id", message="registration_id is required")
    key = str(idempotency_key or "").strip()
    if not key:
        raise MCPError(code="missing_idempotency_key", message="idempotency_key is required for remote send")
    prio = str(priority or "normal").strip() or "normal"
    if prio not in ("normal", "attention"):
        raise MCPError(code="invalid_priority", message="priority must be 'normal' or 'attention'")

    payload = {
        "text": str(text or ""),
        "to": list(to) if isinstance(to, list) else [],
        "priority": prio,
        "reply_required": bool(reply_required),
    }
    return _call_daemon_or_raise(
        {
            "op": "remote_send",
            "args": {
                "group_id": group_id,
                "by": actor_id,
                "registration_id": rid,
                "idempotency_key": key,
                "payload": payload,
            },
        }
    )


def remote_delivery_status(
    *,
    group_id: str,
    registration_id: str,
    idempotency_key: str,
) -> Dict[str, Any]:
    """Read back the receipt for a prior remote send."""
    rid = str(registration_id or "").strip()
    key = str(idempotency_key or "").strip()
    if not rid:
        raise MCPError(code="missing_registration_id", message="registration_id is required")
    if not key:
        raise MCPError(code="missing_idempotency_key", message="idempotency_key is required")
    return _call_daemon_or_raise(
        {
            "op": "remote_delivery_status",
            "args": {
                "group_id": group_id,
                "registration_id": rid,
                "idempotency_key": key,
            },
        }
    )
