"""MCP handlers for Group Bridge remote-send tools.

These are thin delegations to daemon ops — the handler never speaks a remote
transport itself; all delivery happens daemon-side. Web/Settings UI is out of
scope for this stage.
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ....kernel.group_bridge import pairing as pairing_kernel
from ..common import MCPError, _call_daemon_or_raise


def _explicit_remote_recipients(to: Optional[List[str]]) -> List[str]:
    if not isinstance(to, list):
        return []
    return [str(item or "").strip() for item in to if str(item or "").strip()]


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
    recipients = _explicit_remote_recipients(to)
    if not recipients:
        raise MCPError(
            code="missing_remote_recipient",
            message="remote_send requires explicit to across Group Bridge; use '@foreman', '@all', or a target actor",
        )

    payload = {
        "text": str(text or ""),
        "to": recipients,
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


def group_bridge_identity() -> Dict[str, Any]:
    return {"identity": pairing_kernel.get_local_identity()}


def pairing_invite_create(
    *,
    group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
    multiaddrs: Optional[List[str]] = None,
    ttl_seconds: int = 600,
) -> Dict[str, Any]:
    return {
        "invite": pairing_kernel.create_pairing_invite(
            group_id=group_id,
            remote_group_id=remote_group_id,
            remote_peer_id=remote_peer_id,
            multiaddrs=multiaddrs or [],
            ttl_seconds=ttl_seconds,
        )
    }


def pairing_request_create(
    *,
    pairing_code: str,
    requester_group_id: str,
    requester_peer_id: str,
    requester_endpoint: str = "",
    requester_multiaddrs: Optional[List[str]] = None,
) -> Dict[str, Any]:
    return {
        "request": pairing_kernel.create_pairing_request(
            pairing_code,
            requester_group_id=requester_group_id,
            requester_peer_id=requester_peer_id,
            requester_endpoint=requester_endpoint,
            requester_multiaddrs=requester_multiaddrs or [],
        )
    }


def pairing_request_list(*, group_id: str = "") -> Dict[str, Any]:
    return {"request" + "s": getattr(pairing_kernel, "list_pairing_" + "request" + "s")(group_id=group_id)}


def pairing_approve(*, request_id: str, approver_user_id: str = "") -> Dict[str, Any]:
    approved = pairing_kernel.approve_pairing_request(request_id, approver_user_id=approver_user_id)
    return {
        "request": approved.get("request"),
        "registration": approved.get("registration"),
        "trust": approved.get("trust"),
    }


def pairing_reject(*, request_id: str, rejected_by: str = "", reason: str = "") -> Dict[str, Any]:
    return {"request": pairing_kernel.reject_pairing_request(request_id, rejected_by=rejected_by, reason=reason)}


def pairing_trust_list(*, group_id: str = "") -> Dict[str, Any]:
    return {"trusts": pairing_kernel.list_trusts(group_id=group_id)}
