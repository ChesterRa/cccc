"""Daemon ops for outbound remote send (Stage 2).

- ``remote_send``: validate, enqueue idempotently, then perform one synchronous
  delivery attempt. The background outbox worker reuses the same dispatch seam.
- ``remote_delivery_status``: read back a receipt by (registration_id, key).
"""

from __future__ import annotations

import hashlib
from typing import Any, Callable, Dict, Optional, Tuple

from ...contracts.v1.group_bridge import RemoteSendPayload
from ...contracts.v1.ipc import DaemonError, DaemonResponse
from ...kernel.group import load_group
from ...kernel.group_bridge.registration import get_registration
from ...kernel.group_bridge.receipts import get_receipt, update_receipt
from ...kernel.group_bridge.credentials import resolve_group_bridge_credential
from ...kernel.inbox import find_event
from ...kernel.peer_insight import (
    append_peer_perspective,
    normalized_insight_or_error,
    peer_insight_required_details,
    remote_recipients_include_peer,
)
from ...util.conv import coerce_bool
from .receiver import receive_remote_send
from .remote_dispatch import deliver_enqueued, enqueue_remote_send
from .transports.base import RemoteSendTransport, get_transport

CredentialResolver = Callable[[str], Optional[str]]
DispatchSend = Callable[[str, Dict[str, Any]], Tuple[DaemonResponse, bool]]


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _default_credential_resolver(credential_ref: str) -> Optional[str]:
    return resolve_group_bridge_credential(credential_ref)


def _explicit_remote_recipients(to: Any) -> list[str]:
    if not isinstance(to, list):
        return []
    return [str(item or "").strip() for item in to if str(item or "").strip()]


def _source_client_id(registration_id: str, idempotency_key: str) -> str:
    digest = hashlib.sha256(f"{registration_id}\0{idempotency_key}".encode()).hexdigest()[:32]
    return f"group-bridge-source:{digest}"


def _source_event_or_error(
    *,
    src_group_id: str,
    remote_group_id: str,
    source_event_id: str,
) -> tuple[Optional[Dict[str, Any]], Optional[DaemonResponse]]:
    group = load_group(src_group_id)
    if group is None:
        return None, _error("group_not_found", f"group not found: {src_group_id}")
    event = find_event(group, source_event_id)
    if not isinstance(event, dict) or str(event.get("id") or "").strip() != source_event_id:
        return None, _error(
            "source_event_not_found",
            "Group Bridge source event was not found in the source group ledger",
            details={"group_id": src_group_id, "source_event_id": source_event_id},
        )
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    if (
        str(event.get("kind") or "").strip() != "chat.message"
        or str(data.get("dst_group_id") or "").strip() != remote_group_id
    ):
        return None, _error(
            "source_event_mismatch",
            "Group Bridge source event does not match this remote destination",
            details={
                "group_id": src_group_id,
                "source_event_id": source_event_id,
                "dst_group_id": remote_group_id,
            },
        )
    return event, None


def _ensure_source_event(
    *,
    src_group_id: str,
    remote_group_id: str,
    registration_id: str,
    idempotency_key: str,
    source_event_id: str,
    source_record_payload: Dict[str, Any],
    dispatch_send: Optional[DispatchSend],
) -> tuple[Optional[Dict[str, Any]], Optional[DaemonResponse]]:
    if source_event_id:
        return _source_event_or_error(
            src_group_id=src_group_id,
            remote_group_id=remote_group_id,
            source_event_id=source_event_id,
        )
    if dispatch_send is None:
        return None, _error(
            "source_event_persistence_unavailable",
            "Group Bridge remote_send requires the daemon send dispatcher",
        )

    recipients = _explicit_remote_recipients(source_record_payload.get("to"))
    source_by = str(source_record_payload.get("source_by") or "user").strip() or "user"
    source_response, _ = dispatch_send(
        "send",
        {
            "group_id": src_group_id,
            "text": str(source_record_payload.get("text") or ""),
            "by": source_by,
            "to": ["user"],
            "attachments": list(source_record_payload.get("attachments") or []),
            "refs": list(source_record_payload.get("refs") or []),
            "priority": str(source_record_payload.get("priority") or "normal"),
            "reply_required": bool(source_record_payload.get("reply_required")),
            "client_id": _source_client_id(registration_id, idempotency_key),
            "dst_group_id": remote_group_id,
            "dst_to": recipients,
            "insight": source_record_payload.get("insight"),
        },
    )
    if not source_response.ok:
        return None, source_response
    event = source_response.result.get("event") if isinstance(source_response.result, dict) else None
    event_id = str((event or {}).get("id") or "").strip() if isinstance(event, dict) else ""
    if not event_id:
        return None, _error("source_event_missing", "Group Bridge source message was not persisted")
    return _source_event_or_error(
        src_group_id=src_group_id,
        remote_group_id=remote_group_id,
        source_event_id=event_id,
    )


def _deliver_remote_receipt(
    *,
    registration_id: str,
    idempotency_key: str,
    reg: Dict[str, Any],
    initial_status: str,
    transport_factory: Callable[[str], RemoteSendTransport],
    credential_resolver: CredentialResolver,
) -> DaemonResponse:
    credential_ref = str(reg.get("credential_ref") or "").strip()
    credential = ""
    if credential_ref:
        credential = str(credential_resolver(credential_ref) or "").strip()
        if not credential:
            receipt = deliver_enqueued(
                registration_id=registration_id,
                idempotency_key=idempotency_key,
                transport_factory=lambda _name: _CredentialUnresolvedTransport(),
            )
            return DaemonResponse(ok=True, result={"queued": False, "receipt": receipt})
    receipt = deliver_enqueued(
        registration_id=registration_id,
        idempotency_key=idempotency_key,
        transport_factory=transport_factory,
        credential=credential,
    )
    return DaemonResponse(
        ok=True,
        result={"queued": receipt.get("status") == initial_status == "queued", "receipt": receipt},
    )


def handle_remote_send(
    args: Dict[str, Any],
    *,
    transport_factory: Callable[[str], RemoteSendTransport] = get_transport,
    credential_resolver: CredentialResolver = _default_credential_resolver,
    dispatch_send: Optional[DispatchSend] = None,
) -> DaemonResponse:
    src_group_id = str(args.get("group_id") or "").strip()
    registration_id = str(args.get("registration_id") or "").strip()
    idempotency_key = str(args.get("idempotency_key") or "").strip()
    source_event_id = str(args.get("source_event_id") or "").strip()
    reply_to_remote_event_id = str(args.get("reply_to_remote_event_id") or "").strip()
    group_bridge_thread = str(args.get("group_bridge_thread") or "").strip()
    payload_raw = args.get("payload") if isinstance(args.get("payload"), dict) else {}

    if not registration_id:
        return _error("missing_registration_id", "missing registration_id")
    if not idempotency_key:
        return _error("missing_idempotency_key", "idempotency_key is required for remote send")

    reg = get_registration(registration_id)
    if not reg:
        return _error("registration_not_found", f"registration not found: {registration_id}")
    if src_group_id != str(reg.get("group_id") or ""):
        return _error(
            "group_mismatch",
            "request group_id does not match the registration's group",
            details={"request_group_id": src_group_id, "registration_group_id": reg.get("group_id")},
        )
    if str(reg.get("status") or "") != "active":
        return _error(
            "registration_not_active",
            f"registration is not active (status={reg.get('status')})",
            details={"registration_id": registration_id, "status": reg.get("status")},
        )

    remote_group_id = str(reg.get("remote_group_id") or "").strip()

    existing = get_receipt(registration_id, idempotency_key)
    if existing is not None:
        existing_source_event_id = str(existing.get("source_event_id") or source_event_id).strip()
        source_record_raw = existing.get("source_record_payload")
        if not isinstance(source_record_raw, dict):
            source_record_raw = existing.get("payload")
        source_record_payload = dict(source_record_raw) if isinstance(source_record_raw, dict) else {}
        if not str(source_record_payload.get("source_by") or "").strip():
            source_record_payload["source_by"] = str(args.get("by") or "user").strip() or "user"
        _source_event, source_error = _ensure_source_event(
            src_group_id=src_group_id,
            remote_group_id=remote_group_id,
            registration_id=registration_id,
            idempotency_key=idempotency_key,
            source_event_id=existing_source_event_id,
            source_record_payload=source_record_payload,
            dispatch_send=dispatch_send,
        )
        if source_error is not None:
            return source_error
        persisted_source_event_id = str((_source_event or {}).get("id") or "").strip()
        if persisted_source_event_id != str(existing.get("source_event_id") or "").strip():
            update_receipt(
                registration_id,
                idempotency_key,
                source_event_id=persisted_source_event_id,
                source_record_payload=source_record_payload,
            )
        return _deliver_remote_receipt(
            registration_id=registration_id,
            idempotency_key=idempotency_key,
            reg=reg,
            initial_status=str(existing.get("status") or ""),
            transport_factory=transport_factory,
            credential_resolver=credential_resolver,
        )

    # New requests validate and flatten Insight before creating an outbox receipt.
    try:
        payload = RemoteSendPayload(**payload_raw)
    except Exception as e:
        return _error("invalid_payload", str(e))
    source_by = str(args.get("by") or "").strip()
    if not str(payload.source_by or "").strip():
        payload = payload.model_copy(update={"source_by": source_by or "user"})
    recipients = _explicit_remote_recipients(payload.to)
    if not recipients:
        return _error(
            "missing_remote_recipient",
            "remote_send requires explicit to across Group Bridge; use '@foreman', '@all', or a target actor",
        )
    payload = payload.model_copy(update={"to": recipients})
    try:
        insight = normalized_insight_or_error(args.get("insight"))
    except ValueError as exc:
        return _error("invalid_insight", str(exc))
    if coerce_bool(args.get("require_peer_insight")) and remote_recipients_include_peer(recipients) and insight is None:
        return _error(
            "peer_insight_required",
            "Not sent: this peer-facing message is missing `insight`.",
            details=peer_insight_required_details(),
        )
    source_record_payload = payload.model_dump()
    if insight is not None:
        source_record_payload["insight"] = insight
    source_event, source_error = _ensure_source_event(
        src_group_id=src_group_id,
        remote_group_id=remote_group_id,
        registration_id=registration_id,
        idempotency_key=idempotency_key,
        source_event_id=source_event_id,
        source_record_payload=source_record_payload,
        dispatch_send=dispatch_send,
    )
    if source_error is not None:
        return source_error
    source_event_id = str((source_event or {}).get("id") or "").strip()
    if insight is not None:
        payload = payload.model_copy(update={"text": append_peer_perspective(payload.text, insight)})

    queued = enqueue_remote_send(
        src_group_id=src_group_id,
        registration_id=registration_id,
        idempotency_key=idempotency_key,
        payload=payload.model_dump(),
        source_event_id=source_event_id,
        source_record_payload=source_record_payload,
        reply_to_remote_event_id=reply_to_remote_event_id,
        group_bridge_thread=group_bridge_thread,
    )
    return _deliver_remote_receipt(
        registration_id=registration_id,
        idempotency_key=idempotency_key,
        reg=reg,
        initial_status=str(queued.get("status") or ""),
        transport_factory=transport_factory,
        credential_resolver=credential_resolver,
    )


class _CredentialUnresolvedTransport(RemoteSendTransport):
    transport = "credential_resolver"
    capabilities = frozenset()

    def deliver(self, envelope):  # type: ignore[no-untyped-def]
        from .transports.base import permanent_result

        _ = envelope
        return permanent_result(
            "credential_unresolved",
            "credential reference could not be resolved",
            transport=self.transport,
        )


def handle_remote_delivery_status(args: Dict[str, Any]) -> DaemonResponse:
    src_group_id = str(args.get("group_id") or "").strip()
    registration_id = str(args.get("registration_id") or "").strip()
    idempotency_key = str(args.get("idempotency_key") or "").strip()
    if not registration_id:
        return _error("missing_registration_id", "missing registration_id")
    if not idempotency_key:
        return _error("missing_idempotency_key", "missing idempotency_key")
    reg = get_registration(registration_id)
    if not reg:
        return _error("registration_not_found", f"registration not found: {registration_id}")
    if src_group_id != str(reg.get("group_id") or ""):
        return _error(
            "group_mismatch",
            "request group_id does not match the registration's group",
            details={"request_group_id": src_group_id, "registration_group_id": reg.get("group_id")},
        )
    receipt = get_receipt(registration_id, idempotency_key)
    return DaemonResponse(ok=True, result={"receipt": receipt})


def handle_receive_remote_send(args: Dict[str, Any]) -> DaemonResponse:
    result = receive_remote_send(
        target_group_id=str(args.get("target_group_id") or ""),
        src_group_id=str(args.get("src_group_id") or ""),
        remote_peer_id=str(args.get("remote_peer_id") or ""),
        payload=dict(args.get("payload") or {}) if isinstance(args.get("payload"), dict) else {},
        idempotency_key=str(args.get("idempotency_key") or ""),
    )
    if result.get("ok"):
        return DaemonResponse(ok=True, result=result)
    error = result.get("error") if isinstance(result.get("error"), dict) else {}
    return DaemonResponse(
        ok=False,
        error=DaemonError(
            code=str(error.get("code") or "remote_receive_failed"),
            message=str(error.get("message") or "remote receive failed"),
            details={},
        ),
    )


def try_handle_remote_send_op(
    op: str,
    args: Dict[str, Any],
    *,
    dispatch_send: Optional[DispatchSend] = None,
) -> Optional[DaemonResponse]:
    if op == "remote_send":
        return handle_remote_send(args, dispatch_send=dispatch_send)
    if op == "remote_delivery_status":
        return handle_remote_delivery_status(args)
    if op == "group_bridge_receive_remote_send":
        return handle_receive_remote_send(args)
    return None
