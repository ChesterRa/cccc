"""Federation reply relay helpers.

This module keeps remote reply routing out of chat operation code. Chat reply
handlers pass local event metadata here; federation owns trust lookup and
remote-send dispatch.
"""

from __future__ import annotations

import logging
from typing import Any, Dict, Optional

from ...contracts.v1.ipc import DaemonError, DaemonResponse
from ...kernel.federation.pairing import list_trusts
from .ops import handle_remote_send

logger = logging.getLogger("cccc.daemon.server")

_RELAY_SOURCE_PLATFORMS = frozenset({"federation_session", "peer_cccc_http"})


def relay_federation_reply(
    *,
    group_id: str,
    original_data: Dict[str, Any],
    reply_event_id: str,
    text: str,
    to: list[str],
    priority: str,
    reply_required: bool,
    refs: list[dict[str, Any]],
) -> Optional[DaemonResponse]:
    """Relay a local reply back to a trusted federation source."""
    registration_id = federation_reply_registration_id(group_id=group_id, original_data=original_data)
    remote_group_id = str(original_data.get("src_group_id") or "").strip()
    remote_peer_id = str(original_data.get("source_user_id") or "").strip()
    if not registration_id:
        return DaemonResponse(
            ok=False,
            error=DaemonError(
                code="federation_reply_route_not_found",
                message="no active federation route found for reply source",
                details={
                    "group_id": group_id,
                    "remote_group_id": remote_group_id,
                    "remote_peer_id": remote_peer_id,
                    "source_platform": str(original_data.get("source_platform") or "").strip(),
                },
            ),
        )
    try:
        return handle_remote_send(
            {
                "group_id": group_id,
                "registration_id": registration_id,
                "idempotency_key": f"reply:{reply_event_id}:{registration_id}",
                "source_event_id": reply_event_id,
                "reply_to_remote_event_id": str(original_data.get("src_event_id") or "").strip(),
                "payload": {
                    "text": text,
                    "to": _reply_return_recipients(original_data=original_data, fallback=to),
                    "priority": priority,
                    "reply_required": reply_required,
                    "refs": list(refs or []),
                },
            }
        )
    except Exception as exc:
        logger.exception(
            "[federation-reply] relay failed group=%s remote_group=%s remote_peer=%s",
            group_id,
            remote_group_id,
            remote_peer_id,
        )
        return DaemonResponse(
            ok=False,
            error=DaemonError(
                code="federation_reply_failed",
                message=str(exc) or "federation reply failed",
                details={"registration_id": registration_id},
            ),
        )


def can_relay_federation_reply(*, group_id: str, original_data: Dict[str, Any]) -> bool:
    return bool(federation_reply_registration_id(group_id=group_id, original_data=original_data))


def _reply_return_recipients(*, original_data: Dict[str, Any], fallback: list[str]) -> list[str]:
    raw_to = original_data.get("to")
    if isinstance(raw_to, list):
        original_to = [str(item).strip() for item in raw_to if isinstance(item, str) and str(item).strip()]
        if original_to and all(item in ("@all", "@peers", "@foreman", "user", "@user") for item in original_to):
            return original_to
    return list(fallback or [])


def federation_reply_registration_id(*, group_id: str, original_data: Dict[str, Any]) -> str:
    source_platform = str(original_data.get("source_platform") or "").strip()
    if source_platform not in _RELAY_SOURCE_PLATFORMS:
        return ""
    remote_group_id = str(original_data.get("src_group_id") or "").strip()
    remote_peer_id = str(original_data.get("source_user_id") or "").strip()
    if not remote_group_id or not remote_peer_id:
        return ""
    return _registration_for_remote_peer(
        group_id=group_id,
        remote_group_id=remote_group_id,
        remote_peer_id=remote_peer_id,
        transport=source_platform,
    )


def _registration_for_remote_peer(*, group_id: str, remote_group_id: str, remote_peer_id: str, transport: str = "") -> str:
    gid = str(group_id or "").strip()
    remote_gid = str(remote_group_id or "").strip()
    peer_id = str(remote_peer_id or "").strip()
    transport_name = str(transport or "").strip()
    if not gid or not remote_gid or not peer_id:
        return ""
    fallback_registration_id = ""
    session_fallback_registration_id = ""
    for trust in list_trusts(group_id=gid):
        if str(trust.get("status") or "") != "active":
            continue
        if str(trust.get("remote_group_id") or "").strip() != remote_gid:
            continue
        if str(trust.get("remote_peer_id") or "").strip() != peer_id:
            continue
        registration_id = str(trust.get("registration_id") or "").strip()
        if not registration_id:
            continue
        trust_transport = str(trust.get("transport") or "").strip()
        if trust_transport == transport_name:
            return registration_id
        if not _trust_route_is_sendable(trust):
            continue
        if trust_transport == "federation_session":
            if not session_fallback_registration_id:
                session_fallback_registration_id = registration_id
            continue
        if not fallback_registration_id:
            fallback_registration_id = registration_id
    return fallback_registration_id or session_fallback_registration_id


def _trust_route_is_sendable(trust: Dict[str, Any]) -> bool:
    transport = str(trust.get("transport") or "").strip()
    if transport == "peer_cccc_http":
        return True
    if transport == "federation_session":
        return True
    return True
