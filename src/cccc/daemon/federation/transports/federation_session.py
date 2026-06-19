"""Federation WebSocket session transport."""

from __future__ import annotations

from typing import Any, Mapping

from .base import (
    RemoteMessageEnvelope,
    RemoteSendResult,
    RemoteSendTransport,
    permanent_result,
    sent_result,
    transient_result,
)
from ..remote_payloads import build_remote_chat_payload


class FederationSessionTransport(RemoteSendTransport):
    transport = "federation_session"

    def deliver(self, envelope: RemoteMessageEnvelope) -> RemoteSendResult:
        unsupported = self.unsupported_payload(envelope.payload)
        if unsupported is not None:
            return unsupported

        target = envelope.target
        if not target.remote_peer_id:
            return permanent_result("missing_remote_peer_id", "remote_peer_id is required", transport=self.transport)
        if not target.remote_group_id:
            return permanent_result("missing_remote_group_id", "remote_group_id is required", transport=self.transport)
        parsed = _send_session_request(
            local_group_id=envelope.src_group_id,
            remote_group_id=target.remote_group_id,
            remote_peer_id=target.remote_peer_id,
            request={
                "op": "remote_send",
                "src_group_id": envelope.src_group_id,
                "target_group_id": target.remote_group_id,
                "remote_peer_id": target.remote_peer_id,
                "idempotency_key": envelope.idempotency_key,
                "payload": build_remote_chat_payload(envelope),
            },
        )
        if parsed is None:
            return transient_result("peer_session_unavailable", "no active federation WebSocket session", transport=self.transport)
        return _result_from_response(parsed, transport=self.transport)


def _send_session_request(
    *,
    local_group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
    request: Mapping[str, Any],
) -> Mapping[str, Any] | None:
    try:
        from ..ws_session import get_session, send_via_session_sync
    except Exception:
        return None
    if (
        get_session(
            target_group_id=local_group_id,
            src_group_id=remote_group_id,
            remote_peer_id=remote_peer_id,
        )
        is None
    ):
        return None
    return send_via_session_sync(
        target_group_id=local_group_id,
        src_group_id=remote_group_id,
        remote_peer_id=remote_peer_id,
        request=dict(request),
    )


def _result_from_response(parsed: Mapping[str, Any], *, transport: str) -> RemoteSendResult:
    if parsed.get("ok") is False or isinstance(parsed.get("error"), dict):
        err = parsed.get("error") if isinstance(parsed.get("error"), dict) else {}
        code = str(err.get("code") or "remote_error")
        if code in {"peer_session_unavailable", "peer_session_timeout", "peer_session_failed"}:
            return transient_result(
                code,
                str(err.get("message") or parsed.get("error") or "remote federation WebSocket session is unavailable"),
                transport=transport,
            )
        return permanent_result(
            code,
            str(err.get("message") or parsed.get("error") or "remote rejected the message"),
            transport=transport,
        )
    return sent_result(_extract_remote_event_id(parsed), transport=transport)


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
