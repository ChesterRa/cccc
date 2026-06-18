"""Background worker for federation remote-send outbox retries."""

from __future__ import annotations

import logging
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...kernel.federation.credentials import resolve_federation_credential
from .remote_dispatch import deliver_enqueued, iter_due_receipts
from .transports.base import RemoteSendTransport, get_transport, permanent_result

logger = logging.getLogger("cccc.federation.outbox")


@dataclass(frozen=True)
class OutboxSweepResult:
    attempted: int = 0
    sent: int = 0
    retrying: int = 0
    failed: int = 0

    def as_dict(self) -> Dict[str, int]:
        return {
            "attempted": self.attempted,
            "sent": self.sent,
            "retrying": self.retrying,
            "failed": self.failed,
        }


CredentialResolver = Callable[[str], Optional[str]]


def sweep_remote_outbox(
    *,
    home: Optional[Path] = None,
    transport_factory: Callable[[str], RemoteSendTransport] = get_transport,
    credential_resolver: CredentialResolver = resolve_federation_credential,
) -> Dict[str, int]:
    """Attempt all due non-terminal remote-send receipts once."""
    attempted = 0
    sent = 0
    retrying = 0
    failed = 0
    for receipt in iter_due_receipts(home=home):
        registration_id = str(receipt.get("registration_id") or "").strip()
        key = str(receipt.get("idempotency_key") or "").strip()
        if not registration_id or not key:
            continue
        attempted += 1
        try:
            credential = _resolve_credential_for_receipt(
                receipt,
                home=home,
                credential_resolver=credential_resolver,
            )
            if credential is None:
                updated = deliver_enqueued(
                    registration_id=registration_id,
                    idempotency_key=key,
                    home=home,
                    transport_factory=lambda _name: _CredentialUnresolvedTransport(),
                )
                status = str(updated.get("status") or "").strip()
                if status == "failed":
                    failed += 1
                elif status == "retrying":
                    retrying += 1
                elif status == "sent":
                    sent += 1
                continue
            updated = deliver_enqueued(
                registration_id=registration_id,
                idempotency_key=key,
                home=home,
                transport_factory=transport_factory,
                credential=credential,
            )
        except Exception:
            logger.exception("remote outbox sweep failed for registration_id=%s", registration_id)
            failed += 1
            continue
        status = str(updated.get("status") or "").strip()
        if status == "sent":
            sent += 1
        elif status == "retrying":
            retrying += 1
        elif status == "failed":
            failed += 1
    return OutboxSweepResult(attempted=attempted, sent=sent, retrying=retrying, failed=failed).as_dict()


class RemoteOutboxWorker:
    def __init__(
        self,
        *,
        home: Optional[Path] = None,
        interval_seconds: float = 2.0,
        transport_factory: Callable[[str], RemoteSendTransport] = get_transport,
        credential_resolver: CredentialResolver = resolve_federation_credential,
    ) -> None:
        self._home = Path(home) if home is not None else None
        self._interval_seconds = max(0.5, float(interval_seconds or 2.0))
        self._transport_factory = transport_factory
        self._credential_resolver = credential_resolver
        self._stop = threading.Event()
        self._thread: Optional[threading.Thread] = None

    def start(self) -> None:
        if self._thread is not None and self._thread.is_alive():
            return
        self._stop.clear()
        self._thread = threading.Thread(target=self._run, name="cccc-remote-outbox-worker", daemon=True)
        self._thread.start()

    def stop(self, timeout: float = 1.0) -> None:
        self._stop.set()
        thread = self._thread
        if thread is not None:
            thread.join(timeout=max(0.0, float(timeout or 0.0)))

    def _run(self) -> None:
        while not self._stop.is_set():
            try:
                sweep_remote_outbox(
                    home=self._home,
                    transport_factory=self._transport_factory,
                    credential_resolver=self._credential_resolver,
                )
            except Exception:
                logger.exception("remote outbox sweep crashed")
            self._stop.wait(self._interval_seconds)


def _resolve_credential_for_receipt(
    receipt: Dict[str, Any],
    *,
    home: Optional[Path],
    credential_resolver: CredentialResolver,
) -> Optional[str]:
    from ...kernel.federation.registration import get_registration

    reg = get_registration(str(receipt.get("registration_id") or ""), home=home)
    if not isinstance(reg, dict):
        return ""
    credential_ref = str(reg.get("credential_ref") or "").strip()
    if not credential_ref:
        return ""
    credential = str(credential_resolver(credential_ref) or "").strip()
    return credential or None


class _CredentialUnresolvedTransport(RemoteSendTransport):
    transport = "credential_resolver"
    capabilities = frozenset()

    def deliver(self, envelope):  # type: ignore[no-untyped-def]
        _ = envelope
        return permanent_result(
            "credential_unresolved",
            "credential reference could not be resolved",
            transport=self.transport,
        )
