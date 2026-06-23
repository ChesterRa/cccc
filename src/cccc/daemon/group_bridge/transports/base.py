"""Remote-send transport abstraction + registry (Stage 2).

Mirrors the ``ports/im/adapters`` multi-adapter pattern: a small ABC plus a
name-keyed registry. The dispatch layer selects an implementation by the
registration's ``transport`` field — never by sniffing the URL.

A transport only ever receives a ``RemoteMessageEnvelope`` and returns a
standardized ``RemoteSendResult`` (transient vs permanent errors are explicit).
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, FrozenSet, Optional

from ....contracts.v1.group_bridge import RemoteSendPayload


@dataclass(frozen=True)
class RemoteTarget:
    url: str
    remote_group_id: str = ""
    remote_peer_id: str = ""
    multiaddrs: tuple[str, ...] = field(default_factory=tuple)


@dataclass(frozen=True)
class RemoteMessageEnvelope:
    transport: str
    src_group_id: str
    source_peer_id: str
    target: RemoteTarget
    payload: RemoteSendPayload
    idempotency_key: str
    source_multiaddrs: tuple[str, ...] = field(default_factory=tuple)
    source_event_id: str = ""
    reply_to_remote_event_id: str = ""
    group_bridge_thread: str = ""
    # Opaque credential resolved at dispatch time. Never log this field.
    credential: str = ""


@dataclass(frozen=True)
class RemoteSendResult:
    ok: bool
    status: str  # "sent" | "failed"
    remote_event_id: Optional[str] = None
    error_code: str = ""
    error_message: str = ""
    retriable: bool = False
    transport: str = ""


class UnknownTransportError(Exception):
    """Raised when no transport is registered for the requested name."""


def sent_result(remote_event_id: str, *, transport: str) -> RemoteSendResult:
    return RemoteSendResult(ok=True, status="sent", remote_event_id=str(remote_event_id or ""), transport=transport)


def transient_result(code: str, message: str, *, transport: str) -> RemoteSendResult:
    return RemoteSendResult(ok=False, status="failed", error_code=code, error_message=message, retriable=True, transport=transport)


def permanent_result(code: str, message: str, *, transport: str) -> RemoteSendResult:
    return RemoteSendResult(ok=False, status="failed", error_code=code, error_message=message, retriable=False, transport=transport)


class RemoteSendTransport(ABC):
    transport: str = "unknown"
    capabilities: FrozenSet[str] = frozenset()

    def unsupported_payload(self, payload: RemoteSendPayload) -> Optional[RemoteSendResult]:
        """Return a permanent error if the payload needs a capability we lack."""
        if payload.attachments and "attachments" not in self.capabilities:
            return permanent_result("unsupported_attachments", "attachments are not supported by this transport", transport=self.transport)
        if payload.refs and "refs" not in self.capabilities:
            return permanent_result("unsupported_refs", "quoted refs are not supported by this transport", transport=self.transport)
        return None

    @abstractmethod
    def deliver(self, envelope: RemoteMessageEnvelope) -> RemoteSendResult:
        """Deliver the envelope to the remote target. Must not raise for normal
        network failures — classify them into a RemoteSendResult instead."""
        raise NotImplementedError


_REGISTRY: Dict[str, RemoteSendTransport] = {}


def register_transport(transport: RemoteSendTransport) -> None:
    _REGISTRY[str(transport.transport)] = transport


def get_transport(name: str) -> RemoteSendTransport:
    key = str(name or "").strip()
    if key not in _REGISTRY:
        raise UnknownTransportError(f"unknown transport: {key or '(empty)'}")
    return _REGISTRY[key]


def available_transports() -> list[str]:
    return sorted(_REGISTRY.keys())
