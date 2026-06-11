"""Outbound remote-send dispatch (Stage 2).

Two seams, both idempotent:

- ``enqueue_remote_send`` records a ``queued`` receipt and returns immediately.
  It never touches the network. The daemon ``remote_send`` op uses it first,
  then synchronously calls ``deliver_enqueued`` once for the MVP send path.
- ``deliver_enqueued`` resolves the transport + credential and performs one
  delivery attempt, then records the terminal receipt. If the receipt is
  already terminal (``sent``/``failed``), it replays the stored receipt and does
  NOT call the adapter again.

There is intentionally no automatic retry worker in this stage; ``deliver_enqueued``
remains the idempotent seam a future worker can reuse.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1.federation import RemoteSendPayload
from ...kernel.federation import receipts, registration
from ...util.time import utc_now_iso
from .transports.base import (
    RemoteMessageEnvelope,
    RemoteSendTransport,
    RemoteTarget,
    UnknownTransportError,
    get_transport,
)

_TERMINAL = {"sent", "failed"}


def enqueue_remote_send(
    *,
    src_group_id: str,
    registration_id: str,
    idempotency_key: str,
    payload: Dict[str, Any],
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    """Record a queued receipt idempotently. Returns the stored receipt."""
    # Normalize + persist the payload so the delivery seam reconstructs the
    # exact message (text/to/priority/reply_required/refs) the caller queued.
    normalized_payload = RemoteSendPayload(**(payload or {})).model_dump()
    receipt = {
        "ok": False,
        "status": "queued",
        "registration_id": str(registration_id or "").strip(),
        "idempotency_key": str(idempotency_key or "").strip(),
        "src_group_id": str(src_group_id or "").strip(),
        "payload": normalized_payload,
        "transport": "",
        "remote_event_id": None,
        "attempt": 0,
        "accepted_at": utc_now_iso(),
        "error": None,
    }
    stored, _created = receipts.record_receipt(registration_id, idempotency_key, receipt, home=home)
    return stored


def deliver_enqueued(
    *,
    registration_id: str,
    idempotency_key: str,
    home: Optional[Path] = None,
    transport_factory: Callable[[str], RemoteSendTransport] = get_transport,
    credential: str = "",
) -> Dict[str, Any]:
    """Attempt one delivery for an enqueued receipt.

    Replays (no adapter call) when the receipt is already terminal.
    """
    existing = receipts.get_receipt(registration_id, idempotency_key, home=home)
    if existing and str(existing.get("status") or "") in _TERMINAL:
        return existing

    reg = registration.get_registration(registration_id, home=home)
    if not reg:
        return _finalize(registration_id, idempotency_key, home, ok=False, status="failed",
                         error={"code": "registration_not_found", "message": "registration not found", "retriable": False})

    transport_name = str(reg.get("transport") or "").strip()
    try:
        transport = transport_factory(transport_name)
    except UnknownTransportError as e:
        return _finalize(registration_id, idempotency_key, home, ok=False, status="failed",
                         error={"code": "unknown_transport", "message": str(e), "retriable": False})

    envelope = RemoteMessageEnvelope(
        transport=transport_name,
        target=RemoteTarget(url=str(reg.get("url") or ""), remote_group_id=str(reg.get("remote_group_id") or "")),
        payload=RemoteSendPayload(**(payload_from_receipt(existing) or {"text": ""})),
        idempotency_key=str(idempotency_key or "").strip(),
        credential=str(credential or ""),
    )

    result = transport.deliver(envelope)
    error = None
    if not result.ok:
        error = {"code": result.error_code, "message": result.error_message, "retriable": result.retriable, "transport": result.transport}
    return _finalize(
        registration_id,
        idempotency_key,
        home,
        ok=result.ok,
        status=result.status,
        remote_event_id=result.remote_event_id,
        transport=result.transport,
        error=error,
    )


def payload_from_receipt(receipt: Optional[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    """Recover the queued payload persisted on the receipt at enqueue time."""
    if not isinstance(receipt, dict):
        return None
    payload = receipt.get("payload")
    return dict(payload) if isinstance(payload, dict) else None


def _finalize(
    registration_id: str,
    idempotency_key: str,
    home: Optional[Path],
    *,
    ok: bool,
    status: str,
    remote_event_id: Optional[str] = None,
    transport: str = "",
    error: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    updated = receipts.update_receipt(
        registration_id,
        idempotency_key,
        home,
        ok=bool(ok),
        status=str(status),
        remote_event_id=remote_event_id,
        transport=str(transport or ""),
        error=error,
        accepted_at=utc_now_iso(),
    )
    if updated is not None:
        return updated
    # No prior queued receipt — record terminal directly (still idempotent).
    receipt = {
        "ok": bool(ok),
        "status": str(status),
        "registration_id": str(registration_id or "").strip(),
        "idempotency_key": str(idempotency_key or "").strip(),
        "remote_event_id": remote_event_id,
        "transport": str(transport or ""),
        "error": error,
        "accepted_at": utc_now_iso(),
    }
    stored, _ = receipts.record_receipt(registration_id, idempotency_key, receipt, home=home)
    return stored
