"""Daemon ops for outbound remote send (Stage 2).

- ``remote_send``: validate, enqueue idempotently, then perform one synchronous
  delivery attempt. The background outbox worker reuses the same dispatch seam.
- ``remote_delivery_status``: read back a receipt by (registration_id, key).
"""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ...contracts.v1.federation import RemoteSendPayload
from ...contracts.v1.ipc import DaemonError, DaemonResponse
from ...kernel.federation.registration import get_registration
from ...kernel.federation.receipts import get_receipt
from ...kernel.federation.credentials import resolve_federation_credential
from .receiver import receive_remote_send
from .remote_dispatch import deliver_enqueued, enqueue_remote_send
from .transports.base import RemoteSendTransport, get_transport

CredentialResolver = Callable[[str], Optional[str]]


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _default_credential_resolver(credential_ref: str) -> Optional[str]:
    return resolve_federation_credential(credential_ref)


def handle_remote_send(
    args: Dict[str, Any],
    *,
    transport_factory: Callable[[str], RemoteSendTransport] = get_transport,
    credential_resolver: CredentialResolver = _default_credential_resolver,
) -> DaemonResponse:
    src_group_id = str(args.get("group_id") or "").strip()
    registration_id = str(args.get("registration_id") or "").strip()
    idempotency_key = str(args.get("idempotency_key") or "").strip()
    source_event_id = str(args.get("source_event_id") or "").strip()
    reply_to_remote_event_id = str(args.get("reply_to_remote_event_id") or "").strip()
    federation_thread = str(args.get("federation_thread") or "").strip()
    payload_raw = args.get("payload") if isinstance(args.get("payload"), dict) else {}

    if not registration_id:
        return _error("missing_registration_id", "missing registration_id")
    if not idempotency_key:
        return _error("missing_idempotency_key", "idempotency_key is required for remote send")

    # Validate payload shape up-front (rejects unknown fields, normalizes defaults).
    try:
        payload = RemoteSendPayload(**payload_raw)
    except Exception as e:
        return _error("invalid_payload", str(e))

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

    queued = enqueue_remote_send(
        src_group_id=src_group_id,
        registration_id=registration_id,
        idempotency_key=idempotency_key,
        payload=payload.model_dump(),
        source_event_id=source_event_id,
        reply_to_remote_event_id=reply_to_remote_event_id,
        federation_thread=federation_thread,
    )
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
    return DaemonResponse(ok=True, result={"queued": receipt.get("status") == queued.get("status") == "queued", "receipt": receipt})


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


def try_handle_remote_send_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "remote_send":
        return handle_remote_send(args)
    if op == "remote_delivery_status":
        return handle_remote_delivery_status(args)
    if op == "federation_receive_remote_send":
        return handle_receive_remote_send(args)
    return None
