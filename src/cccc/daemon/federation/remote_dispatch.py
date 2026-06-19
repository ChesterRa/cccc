"""Outbound remote-send dispatch (Stage 2).

Two seams, both idempotent:

- ``enqueue_remote_send`` records a ``queued`` receipt and returns immediately.
  It never touches the network. The daemon ``remote_send`` op uses it first,
  then synchronously calls ``deliver_enqueued`` once for the MVP send path.
- ``deliver_enqueued`` resolves the transport + credential and performs one
  delivery attempt. Permanent terminal receipts (``sent``/``failed``) replay
  without touching the adapter; transient delivery failures become ``retrying``
  and can be attempted again with the same idempotency key.
"""

from __future__ import annotations

from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1.federation import RemoteSendPayload
from ...kernel.federation.pairing import get_local_identity
from ...kernel.federation import receipts, registration
from ...util.time import parse_utc_iso, utc_now_iso
from .transports.base import (
    RemoteMessageEnvelope,
    RemoteSendTransport,
    RemoteTarget,
    UnknownTransportError,
    get_transport,
)

_TERMINAL = {"sent", "failed"}
_DEFAULT_MAX_ATTEMPTS = 5
_BACKOFF_SECONDS = (2, 5, 15, 30, 60)
_SENDING_STALE_SECONDS = 120


def enqueue_remote_send(
    *,
    src_group_id: str,
    registration_id: str,
    idempotency_key: str,
    payload: Dict[str, Any],
    source_event_id: str = "",
    reply_to_remote_event_id: str = "",
    federation_thread: str = "",
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    """Record a queued receipt idempotently. Returns the stored receipt."""
    # Normalize + persist the payload so the delivery seam reconstructs the
    # exact message (text/to/priority/reply_required/refs) the caller queued.
    normalized_payload = RemoteSendPayload(**(payload or {})).model_dump()
    now = utc_now_iso()
    receipt = {
        "ok": False,
        "status": "queued",
        "registration_id": str(registration_id or "").strip(),
        "idempotency_key": str(idempotency_key or "").strip(),
        "src_group_id": str(src_group_id or "").strip(),
        "source_event_id": str(source_event_id or "").strip(),
        "reply_to_remote_event_id": str(reply_to_remote_event_id or "").strip(),
        "federation_thread": str(federation_thread or "").strip(),
        "payload": normalized_payload,
        "transport": "",
        "remote_event_id": None,
        "attempt": 0,
        "max_attempts": _DEFAULT_MAX_ATTEMPTS,
        "first_queued_at": now,
        "last_attempt_at": "",
        "next_attempt_at": now,
        "accepted_at": now,
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
        transport = _resolve_transport(transport_name, home=home, transport_factory=transport_factory)
    except UnknownTransportError as e:
        return _finalize(registration_id, idempotency_key, home, ok=False, status="failed",
                         error={"code": "unknown_transport", "message": str(e), "retriable": False})

    envelope = RemoteMessageEnvelope(
        transport=transport_name,
        src_group_id=str(reg.get("group_id") or ""),
        source_peer_id=_local_peer_id(home=home),
        source_multiaddrs=_local_multiaddrs(home=home),
        target=RemoteTarget(
            url=str(reg.get("url") or ""),
            remote_group_id=str(reg.get("remote_group_id") or ""),
            remote_peer_id=str(reg.get("remote_peer_id") or ""),
            multiaddrs=tuple(str(addr or "").strip() for addr in (reg.get("multiaddrs") or []) if str(addr or "").strip()),
        ),
        payload=RemoteSendPayload(**(payload_from_receipt(existing) or {"text": ""})),
        idempotency_key=str(idempotency_key or "").strip(),
        source_event_id=str((existing or {}).get("source_event_id") or ""),
        reply_to_remote_event_id=str((existing or {}).get("reply_to_remote_event_id") or ""),
        federation_thread=str((existing or {}).get("federation_thread") or ""),
        credential=str(credential or ""),
    )

    started_at = utc_now_iso()
    receipts.update_receipt(
        registration_id,
        idempotency_key,
        home,
        status="sending",
        last_attempt_at=started_at,
        accepted_at=started_at,
    )

    result = transport.deliver(envelope)
    error = None
    status = result.status
    if not result.ok:
        error = {"code": result.error_code, "message": result.error_message, "retriable": result.retriable, "transport": result.transport}
        if result.retriable:
            status = "retrying"
    return _finalize(
        registration_id,
        idempotency_key,
        home,
        ok=result.ok,
        status=status,
        remote_event_id=result.remote_event_id,
        transport=result.transport,
        error=error,
        attempted_at=started_at,
    )


def retry_remote_send_for_peer(
    *,
    group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
    transport: str = "libp2p_cccc",
    home: Optional[Path] = None,
    transport_factory: Callable[[str], RemoteSendTransport] = get_transport,
    credential: str = "",
) -> Dict[str, Any]:
    """Retry non-terminal remote-send receipts for one trusted peer route."""
    attempted = 0
    sent = 0
    still_retrying = 0
    for reg in registration.list_registrations(home=home):
        if str(reg.get("group_id") or "").strip() != str(group_id or "").strip():
            continue
        if str(reg.get("remote_group_id") or "").strip() != str(remote_group_id or "").strip():
            continue
        if str(reg.get("remote_peer_id") or "").strip() != str(remote_peer_id or "").strip():
            continue
        if str(reg.get("transport") or "").strip() != str(transport or "").strip():
            continue
        if str(reg.get("status") or "").strip() != "active":
            continue
        registration_id = str(reg.get("registration_id") or "").strip()
        if not registration_id:
            continue
        for receipt in receipts.load_receipts(home=home).values():
            if str(receipt.get("registration_id") or "").strip() != registration_id:
                continue
            if str(receipt.get("status") or "").strip() != "retrying":
                continue
            key = str(receipt.get("idempotency_key") or "").strip()
            if not key:
                continue
            attempted += 1
            updated = deliver_enqueued(
                registration_id=registration_id,
                idempotency_key=key,
                home=home,
                transport_factory=transport_factory,
                credential=credential,
            )
            if str(updated.get("status") or "") == "sent":
                sent += 1
            elif str(updated.get("status") or "") == "retrying":
                still_retrying += 1
    return {"attempted": attempted, "sent": sent, "retrying": still_retrying}


def iter_due_receipts(*, home: Optional[Path] = None, now: Optional[datetime] = None) -> list[Dict[str, Any]]:
    """Return queued/retrying receipts whose next attempt time is due."""
    now_utc = now.astimezone(timezone.utc) if isinstance(now, datetime) else datetime.now(timezone.utc)
    due: list[Dict[str, Any]] = []
    for receipt in receipts.load_receipts(home=home).values():
        status = str(receipt.get("status") or "").strip()
        if status == "sending":
            last_dt = parse_utc_iso(str(receipt.get("last_attempt_at") or ""))
            if last_dt is not None and (now_utc - last_dt).total_seconds() < _SENDING_STALE_SECONDS:
                continue
        elif status not in {"queued", "retrying"}:
            continue
        attempt = _safe_int(receipt.get("attempt"), 0)
        max_attempts = max(1, _safe_int(receipt.get("max_attempts"), _DEFAULT_MAX_ATTEMPTS))
        if attempt >= max_attempts:
            continue
        next_dt = parse_utc_iso(str(receipt.get("next_attempt_at") or ""))
        if next_dt is not None and next_dt > now_utc:
            continue
        due.append(dict(receipt))
    return due


def _resolve_transport(
    transport_name: str,
    *,
    home: Optional[Path],
    transport_factory: Callable[[str], RemoteSendTransport],
) -> RemoteSendTransport:
    if home is not None and transport_factory is get_transport and str(transport_name or "").strip() == "libp2p_cccc":
        from .transports.libp2p_cccc import Libp2pCcccTransport

        return Libp2pCcccTransport(address_book_home=home)
    return transport_factory(transport_name)


def _local_peer_id(*, home: Optional[Path]) -> str:
    try:
        return str(get_local_identity(home=home).get("peer_id") or "").strip()
    except Exception:
        return ""


def _local_multiaddrs(*, home: Optional[Path]) -> tuple[str, ...]:
    try:
        from .libp2p.advertise import local_advertised_multiaddrs

        return local_advertised_multiaddrs(home=home)
    except Exception:
        return ()


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
    attempted_at: str = "",
) -> Dict[str, Any]:
    prior = receipts.get_receipt(registration_id, idempotency_key, home=home) or {}
    try:
        attempt = int(prior.get("attempt") or 0) + 1
    except Exception:
        attempt = 1
    max_attempts = max(1, _safe_int(prior.get("max_attempts"), _DEFAULT_MAX_ATTEMPTS))
    status_out = str(status)
    error_out = error
    if status_out == "retrying" and attempt >= max_attempts:
        status_out = "failed"
        if isinstance(error_out, dict):
            error_out = dict(error_out)
            error_out["retriable"] = False
            error_out.setdefault("code", "max_attempts_exhausted")
            error_out["message"] = str(error_out.get("message") or "remote delivery retry attempts exhausted")
        else:
            error_out = {
                "code": "max_attempts_exhausted",
                "message": "remote delivery retry attempts exhausted",
                "retriable": False,
                "transport": str(transport or ""),
            }
    next_attempt_at = ""
    if status_out == "retrying":
        next_attempt_at = _next_attempt_iso(attempt, attempted_at=attempted_at)
    last_attempt_at = str(attempted_at or utc_now_iso())
    accepted_at = utc_now_iso()
    updated = receipts.update_receipt(
        registration_id,
        idempotency_key,
        home,
        ok=bool(ok),
        status=status_out,
        remote_event_id=remote_event_id,
        transport=str(transport or ""),
        error=error_out,
        attempt=attempt,
        max_attempts=max_attempts,
        last_attempt_at=last_attempt_at,
        next_attempt_at=next_attempt_at,
        accepted_at=accepted_at,
    )
    if updated is not None:
        return updated
    # No prior queued receipt — record terminal directly (still idempotent).
    receipt = {
        "ok": bool(ok),
        "status": status_out,
        "registration_id": str(registration_id or "").strip(),
        "idempotency_key": str(idempotency_key or "").strip(),
        "remote_event_id": remote_event_id,
        "transport": str(transport or ""),
        "error": error_out,
        "attempt": attempt,
        "max_attempts": max_attempts,
        "first_queued_at": accepted_at,
        "last_attempt_at": last_attempt_at,
        "next_attempt_at": next_attempt_at,
        "accepted_at": accepted_at,
    }
    stored, _ = receipts.record_receipt(registration_id, idempotency_key, receipt, home=home)
    return stored


def _safe_int(value: Any, default: int) -> int:
    try:
        return int(value)
    except Exception:
        return int(default)


def _next_attempt_iso(attempt: int, *, attempted_at: str) -> str:
    base = parse_utc_iso(attempted_at) or datetime.now(timezone.utc)
    idx = min(max(0, int(attempt) - 1), len(_BACKOFF_SECONDS) - 1)
    return (base + timedelta(seconds=_BACKOFF_SECONDS[idx])).isoformat().replace("+00:00", "Z")
