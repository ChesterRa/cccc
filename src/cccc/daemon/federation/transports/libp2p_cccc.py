"""libp2p transport boundary for CCCC federation.

Stage 1 deliberately keeps this as a small adapter seam: registration and
dispatch can carry peer metadata, while the actual libp2p client is injected by
the runtime that owns networking.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Mapping, Optional

from .base import (
    RemoteMessageEnvelope,
    RemoteSendResult,
    RemoteSendTransport,
    permanent_result,
    sent_result,
    transient_result,
)
from ..remote_payloads import build_remote_chat_payload


@dataclass(frozen=True)
class Libp2pSendRequest:
    src_group_id: str
    remote_peer_id: str
    multiaddrs: tuple[str, ...]
    remote_group_id: str
    payload: Any
    idempotency_key: str
    credential: str = ""


Libp2pSend = Callable[[Libp2pSendRequest], Mapping[str, Any]]


class Libp2pCcccTransport(RemoteSendTransport):
    transport = "libp2p_cccc"

    def __init__(self, *, client: Optional[Libp2pSend] = None, address_book_home: Optional[Path] = None) -> None:
        self._client = client
        self._address_book_home = Path(address_book_home) if address_book_home is not None else None

    def deliver(self, envelope: RemoteMessageEnvelope) -> RemoteSendResult:
        unsupported = self.unsupported_payload(envelope.payload)
        if unsupported is not None:
            return unsupported

        client = self._client
        if client is None:
            try:
                from ..libp2p.client import default_libp2p_send

                client = lambda request: default_libp2p_send(request, home=self._address_book_home)
            except Exception:
                client = None
        if client is None:
            return transient_result("libp2p_client_unavailable", "libp2p client is not configured for this runtime", transport=self.transport)

        target = envelope.target
        if not target.remote_peer_id:
            return permanent_result("missing_remote_peer_id", "remote_peer_id is required", transport=self.transport)
        if not target.remote_group_id:
            return permanent_result("missing_remote_group_id", "remote_group_id is required", transport=self.transport)
        multiaddrs = tuple(target.multiaddrs or ())
        if not multiaddrs:
            try:
                from ....kernel.federation.peer_addresses import resolve_peer_multiaddrs

                multiaddrs = resolve_peer_multiaddrs(
                    target.remote_peer_id,
                    remote_group_id=target.remote_group_id,
                    home=self._address_book_home,
                )
            except Exception:
                multiaddrs = ()
        if not multiaddrs:
            return transient_result("address_unresolved", "remote peer multiaddr is not known", transport=self.transport)

        request = Libp2pSendRequest(
            src_group_id=envelope.src_group_id,
            remote_peer_id=target.remote_peer_id,
            multiaddrs=multiaddrs,
            remote_group_id=target.remote_group_id,
            payload=build_remote_chat_payload(envelope),
            idempotency_key=envelope.idempotency_key,
            credential=envelope.credential,
        )
        try:
            parsed = dict(client(request) or {})
        except Exception as exc:
            return transient_result("libp2p_delivery_failed", str(exc), transport=self.transport)

        if parsed.get("ok") is False or isinstance(parsed.get("error"), dict):
            err = parsed.get("error") if isinstance(parsed.get("error"), dict) else {}
            code = str(err.get("code") or "remote_error")
            if code in {"libp2p_dial_failed", "libp2p_client_unavailable"}:
                return transient_result(
                    code,
                    str(err.get("message") or parsed.get("error") or "remote libp2p node is unavailable"),
                    transport=self.transport,
                )
            return permanent_result(
                code,
                str(err.get("message") or parsed.get("error") or "remote rejected the message"),
                transport=self.transport,
            )

        remote_event_id = _extract_remote_event_id(parsed)
        return sent_result(remote_event_id, transport=self.transport)


def _extract_remote_event_id(parsed: Mapping[str, Any]) -> str:
    event_id = parsed.get("event_id")
    if event_id:
        return str(event_id)
    result = parsed.get("result")
    if isinstance(result, Mapping):
        legacy = result.get("event_id")
        if legacy:
            return str(legacy)
        event = result.get("event")
        if isinstance(event, Mapping) and event.get("id"):
            return str(event.get("id"))
    return ""
