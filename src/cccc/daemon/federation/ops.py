"""Daemon ops for outbound remote send (Stage 2).

- ``remote_send``: validate, enqueue idempotently, then perform one synchronous
  delivery attempt. There is still no retry worker in this stage.
- ``remote_delivery_status``: read back a receipt by (registration_id, key).
"""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ...contracts.v1.federation import RemoteSendPayload
from ...contracts.v1.ipc import DaemonError, DaemonResponse
from ...kernel.federation.registration import get_registration
from ...kernel.federation.receipts import get_receipt
from .remote_dispatch import deliver_enqueued, enqueue_remote_send
from .transports.base import RemoteSendTransport, get_transport

CredentialResolver = Callable[[str], Optional[str]]


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _default_credential_resolver(credential_ref: str) -> Optional[str]:
    _ = credential_ref
    return None


def handle_remote_send(
    args: Dict[str, Any],
    *,
    transport_factory: Callable[[str], RemoteSendTransport] = get_transport,
    credential_resolver: CredentialResolver = _default_credential_resolver,
) -> DaemonResponse:
    src_group_id = str(args.get("group_id") or "").strip()
    registration_id = str(args.get("registration_id") or "").strip()
    idempotency_key = str(args.get("idempotency_key") or "").strip()
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


def try_handle_remote_send_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "remote_send":
        return handle_remote_send(args)
    if op == "remote_delivery_status":
        return handle_remote_delivery_status(args)
    return None
