"""Cross-group propagation for request-reply cancellation facts."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any, Dict, Optional

from ...contracts.v1.group_bridge import (
    GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
    RemoteReplyRequestCancelPayload,
)
from ...kernel.group import Group, load_group
from ...kernel.group_bridge.credentials import lookup_pairing_remote_send_credential
from ...kernel.group_bridge.pairing import list_trusts
from ...kernel.group_bridge.pairing import active_trust_for_remote_send_credential
from ...kernel.group_bridge.receipts import load_receipts
from ...kernel.inbox import find_event, iter_events_reverse
from ...kernel.ledger import append_event
from .remote_dispatch import deliver_enqueued, enqueue_reply_request_cancel

LOGGER = logging.getLogger("cccc.group_bridge.cancellation")


def propagate_reply_request_cancel(
    *,
    source_group: Group,
    source_message: Dict[str, Any],
    cancel_event: Dict[str, Any],
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    """Propagate a local cancellation to its relayed destination, if any."""
    try:
        return _propagate_reply_request_cancel(
            source_group=source_group,
            source_message=source_message,
            cancel_event=cancel_event,
            home=home,
        )
    except Exception as exc:
        LOGGER.exception(
            "reply-request cancellation propagation failed group=%s source_event=%s",
            source_group.group_id,
            source_message.get("id"),
        )
        return {
            "state": "failed",
            "error": {"code": "propagation_failed", "message": str(exc)},
        }


def _propagate_reply_request_cancel(
    *,
    source_group: Group,
    source_message: Dict[str, Any],
    cancel_event: Dict[str, Any],
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    source_message_id = str(source_message.get("id") or "").strip()
    cancel_event_id = str(cancel_event.get("id") or "").strip()
    if not source_message_id or not cancel_event_id:
        return {"state": "not_applicable"}

    local_receipt = _local_cross_group_receipt(source_group, source_message_id)
    if local_receipt is not None:
        data = local_receipt.get("data") if isinstance(local_receipt.get("data"), dict) else {}
        dst_group_id = str(data.get("dst_group_id") or "").strip()
        dst_event_id = str(data.get("dst_event_id") or "").strip()
        if dst_group_id and dst_event_id:
            result = _append_destination_cancel(
                target_group_id=dst_group_id,
                remote_source_event_id=dst_event_id,
                src_group_id=source_group.group_id,
                source_message_event_id=source_message_id,
                source_cancel_event_id=cancel_event_id,
                by="system",
                home=home,
            )
            return {"state": "sent", "transport": "local", **result}

    original_receipt = _remote_source_receipt(
        source_message_event_id=source_message_id,
        home=home,
    )
    if original_receipt is None:
        return {"state": "not_applicable"}
    registration_id = str(original_receipt.get("registration_id") or "").strip()
    if not registration_id:
        return {"state": "failed", "error": {"code": "missing_registration_id"}}
    idempotency_key = f"reply-request-cancel:{cancel_event_id}"
    queued = enqueue_reply_request_cancel(
        src_group_id=source_group.group_id,
        registration_id=registration_id,
        idempotency_key=idempotency_key,
        source_cancel_event_id=cancel_event_id,
        source_message_event_id=source_message_id,
        remote_source_event_id=str(original_receipt.get("remote_event_id") or "").strip(),
        home=home,
    )
    delivered = deliver_enqueued(
        registration_id=registration_id,
        idempotency_key=idempotency_key,
        home=home,
    )
    return {
        "state": str(delivered.get("status") or queued.get("status") or "queued"),
        "transport": str(delivered.get("transport") or "group_bridge_session"),
        "receipt": delivered,
    }


def receive_remote_reply_request_cancel(
    *,
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
    payload: Dict[str, Any],
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    target_gid = str(target_group_id or "").strip()
    src_gid = str(src_group_id or "").strip()
    peer_id = str(remote_peer_id or "").strip()
    try:
        control = RemoteReplyRequestCancelPayload(**(payload or {}))
    except Exception:
        return _error("invalid_payload", "reply-request cancellation payload is invalid")
    if control.source_group_id != src_gid:
        return _error("source_group_mismatch", "cancellation source group does not match the session")
    if not _has_active_trust(target_gid, src_gid, peer_id, home=home):
        return _error("unauthorized_peer", "remote peer is not trusted for this group")
    try:
        result = _append_destination_cancel(
            target_group_id=target_gid,
            remote_source_event_id=control.remote_source_event_id,
            src_group_id=src_gid,
            source_message_event_id=control.source_message_event_id,
            source_cancel_event_id=control.source_cancel_event_id,
            by=f"group_bridge:{peer_id}",
            home=home,
        )
    except ValueError as exc:
        return _error("source_event_mismatch", str(exc))
    return {"ok": True, **result}


def receive_authenticated_reply_request_cancel(
    token: str,
    body: Dict[str, Any],
    *,
    home: Optional[Path] = None,
) -> Optional[Dict[str, Any]]:
    if body.get("message_contract_version") != GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION:
        return _error(
            "contract_version_mismatch",
            "Group Bridge message contract version does not match",
        )
    credential = lookup_pairing_remote_send_credential(token, home=home)
    trust = active_trust_for_remote_send_credential(credential or {}, home=home)
    if not isinstance(trust, dict):
        return None
    source_group_id = str(body.get("source_group_id") or body.get("src_group_id") or "").strip()
    if source_group_id != str(trust.get("remote_group_id") or "").strip():
        return None
    payload = body.get("payload") if isinstance(body.get("payload"), dict) else {}
    return receive_remote_reply_request_cancel(
        target_group_id=str(trust.get("group_id") or ""),
        src_group_id=source_group_id,
        remote_peer_id=str(trust.get("remote_peer_id") or ""),
        payload=payload,
        home=home,
    )


def _append_destination_cancel(
    *,
    target_group_id: str,
    remote_source_event_id: str,
    src_group_id: str,
    source_message_event_id: str,
    source_cancel_event_id: str,
    by: str,
    home: Optional[Path],
) -> Dict[str, Any]:
    group = _load_group(target_group_id, home=home)
    if group is None:
        raise ValueError("target group was not found")
    source = find_event(group, remote_source_event_id)
    data = source.get("data") if isinstance(source, dict) and isinstance(source.get("data"), dict) else {}
    if (
        not isinstance(source, dict)
        or str(source.get("kind") or "") != "chat.message"
        or str(data.get("message_mode") or "") != "request_reply"
        or str(data.get("src_group_id") or "").strip() != src_group_id
        or str(data.get("src_event_id") or "").strip() != source_message_event_id
    ):
        raise ValueError("relayed request-reply source does not match the cancellation provenance")
    for event in iter_events_reverse(group.ledger_path):
        event_data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if (
            str(event.get("kind") or "") == "chat.reply_request.cancelled"
            and str(event_data.get("source_event_id") or "").strip() == remote_source_event_id
        ):
            return {"event": event, "event_id": str(event.get("id") or ""), "already": True}
    event = append_event(
        group.ledger_path,
        kind="chat.reply_request.cancelled",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={
            "source_event_id": remote_source_event_id,
            "src_group_id": src_group_id,
            "src_event_id": source_cancel_event_id,
            "src_message_event_id": source_message_event_id,
        },
    )
    return {"event": event, "event_id": str(event.get("id") or ""), "already": False}


def _local_cross_group_receipt(group: Group, source_event_id: str) -> Optional[Dict[str, Any]]:
    for event in iter_events_reverse(group.ledger_path):
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if (
            str(event.get("kind") or "") == "chat.cross_group_receipt"
            and str(data.get("operation") or "") == "remote_send"
            and str(data.get("source_event_id") or "").strip() == source_event_id
            and str(data.get("status") or "").strip() == "sent"
        ):
            return event
    return None


def _remote_source_receipt(
    *,
    source_message_event_id: str,
    home: Optional[Path],
) -> Optional[Dict[str, Any]]:
    for receipt in load_receipts(home=home).values():
        if str(receipt.get("operation") or "") != "remote_send":
            continue
        if str(receipt.get("source_event_id") or "").strip() == source_message_event_id:
            return dict(receipt)
    return None


def _has_active_trust(
    target_group_id: str,
    src_group_id: str,
    remote_peer_id: str,
    *,
    home: Optional[Path],
) -> bool:
    return any(
        str(trust.get("status") or "") == "active"
        and str(trust.get("remote_group_id") or "").strip() == src_group_id
        and str(trust.get("remote_peer_id") or "").strip() == remote_peer_id
        for trust in list_trusts(group_id=target_group_id, home=home)
    )


def _load_group(group_id: str, *, home: Optional[Path]) -> Optional[Group]:
    if home is None:
        return load_group(group_id)
    from .receiver import _load_group as load_group_from_home

    return load_group_from_home(group_id, home=home)


def _error(code: str, message: str) -> Dict[str, Any]:
    return {"ok": False, "error": {"code": code, "message": message}}
