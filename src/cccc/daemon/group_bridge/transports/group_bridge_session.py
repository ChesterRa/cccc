"""Group Bridge WebSocket session transport."""

from __future__ import annotations

import json
import os
from typing import Any, Mapping
import urllib.error
import urllib.parse
import urllib.request

from ....contracts.v1.group_bridge import GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION
from .base import (
    RemoteMessageEnvelope,
    RemoteReplyRequestCancelEnvelope,
    RemoteSendResult,
    RemoteSendTransport,
    permanent_result,
    sent_result,
    transient_result,
)
from ..remote_payloads import build_remote_chat_payload
from ....paths import ensure_home
from ....ports.web.runtime_control import http_url, local_connect_host, read_web_runtime_state


class GroupBridgeSessionTransport(RemoteSendTransport):
    transport = "group_bridge_session"
    capabilities = frozenset({"attachments"})

    def deliver(self, envelope: RemoteMessageEnvelope) -> RemoteSendResult:
        unsupported = self.unsupported_payload(envelope.payload)
        if unsupported is not None:
            return unsupported

        target = envelope.target
        if not target.remote_peer_id:
            return permanent_result("missing_remote_peer_id", "remote_peer_id is required", transport=self.transport)
        if not target.remote_group_id:
            return permanent_result("missing_remote_group_id", "remote_group_id is required", transport=self.transport)
        try:
            payload = build_remote_chat_payload(envelope)
        except ValueError as exc:
            return permanent_result("invalid_attachments", str(exc), transport=self.transport)
        except OSError as exc:
            return permanent_result("attachment_read_failed", str(exc), transport=self.transport)
        parsed = _send_session_request(
            local_group_id=envelope.src_group_id,
            remote_group_id=target.remote_group_id,
            remote_peer_id=target.remote_peer_id,
            request={
                "message_contract_version": GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
                "op": "remote_send",
                "src_group_id": envelope.src_group_id,
                "target_group_id": target.remote_group_id,
                "remote_peer_id": target.remote_peer_id,
                "idempotency_key": envelope.idempotency_key,
                "payload": payload,
            },
        )
        if parsed is None or _session_unavailable(parsed):
            parsed = _send_authenticated_http(
                target.url,
                envelope.credential,
                {**payload, "op": "remote_send"},
            )
        if parsed is None:
            return transient_result("peer_session_unavailable", "no active Group Bridge WebSocket session", transport=self.transport)
        return _result_from_response(parsed, transport=self.transport)

    def cancel_reply_request(self, envelope: RemoteReplyRequestCancelEnvelope) -> RemoteSendResult:
        target = envelope.target
        if not target.remote_peer_id or not target.remote_group_id:
            return permanent_result(
                "missing_remote_route",
                "remote_group_id and remote_peer_id are required",
                transport=self.transport,
            )
        parsed = _send_session_request(
            local_group_id=envelope.src_group_id,
            remote_group_id=target.remote_group_id,
            remote_peer_id=target.remote_peer_id,
            request={
                "message_contract_version": GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
                "op": "reply_request_cancel",
                "src_group_id": envelope.src_group_id,
                "target_group_id": target.remote_group_id,
                "remote_peer_id": target.remote_peer_id,
                "idempotency_key": envelope.idempotency_key,
                "payload": envelope.payload.model_dump(),
            },
        )
        if parsed is None or _session_unavailable(parsed):
            parsed = _send_authenticated_http(
                target.url,
                envelope.credential,
                {
                    "message_contract_version": GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
                    "op": "reply_request_cancel",
                    "source_group_id": envelope.src_group_id,
                    "src_group_id": envelope.src_group_id,
                    "idempotency_key": envelope.idempotency_key,
                    "payload": envelope.payload.model_dump(),
                },
            )
        if parsed is None:
            return transient_result(
                "peer_session_unavailable",
                "no active Group Bridge WebSocket session",
                transport=self.transport,
            )
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
        return _send_session_request_via_web_owner(
            local_group_id=local_group_id,
            remote_group_id=remote_group_id,
            remote_peer_id=remote_peer_id,
            request=request,
        )
    return send_via_session_sync(
        target_group_id=local_group_id,
        src_group_id=remote_group_id,
        remote_peer_id=remote_peer_id,
        request=dict(request),
    )


def _send_session_request_via_web_owner(
    *,
    local_group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
    request: Mapping[str, Any],
    timeout: float = 5.0,
) -> Mapping[str, Any] | None:
    # The supervised web child owns inbound WebSocket objects. Daemon/MCP sends
    # must route to that owner instead of reading their own process-local session map.
    if os.environ.get("CCCC_WEB_SUPERVISED"):
        return None
    runtime = read_web_runtime_state(ensure_home())
    try:
        port = int(runtime.get("port") or 0)
    except Exception:
        port = 0
    if port <= 0:
        return None
    host = local_connect_host(str(runtime.get("host") or "127.0.0.1"))
    url = http_url(host, port, path="/api/group-bridge/session/send")
    body = json.dumps(
        {
            "target_group_id": local_group_id,
            "src_group_id": remote_group_id,
            "remote_peer_id": remote_peer_id,
            "request": dict(request or {}),
            "timeout": float(timeout or 5.0),
        }
    ).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=max(0.1, float(timeout or 5.0)) + 0.5) as resp:
            parsed = json.loads(resp.read().decode("utf-8") or "{}")
    except (OSError, urllib.error.URLError, urllib.error.HTTPError, json.JSONDecodeError):
        return None
    return parsed if isinstance(parsed, Mapping) else None


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):  # type: ignore[no-untyped-def]
        return None


def _send_authenticated_http(
    endpoint: str,
    credential: str,
    body: Mapping[str, Any],
    *,
    timeout: float = 10.0,
) -> Mapping[str, Any] | None:
    raw_endpoint = str(endpoint or "").strip()
    token = str(credential or "").strip()
    parsed_endpoint = urllib.parse.urlsplit(raw_endpoint)
    if (
        not token
        or parsed_endpoint.scheme not in {"http", "https"}
        or not parsed_endpoint.hostname
        or parsed_endpoint.username is not None
        or parsed_endpoint.password is not None
    ):
        return None
    url = urllib.parse.urlunsplit(
        (
            parsed_endpoint.scheme,
            parsed_endpoint.netloc,
            "/api/group-bridge/session/send",
            "",
            "",
        )
    )
    request = urllib.request.Request(
        url,
        data=json.dumps(dict(body or {})).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.build_opener(_NoRedirect).open(
            request,
            timeout=max(0.1, float(timeout or 10.0)),
        ) as response:
            value = json.loads(response.read().decode("utf-8") or "{}")
            return value if isinstance(value, Mapping) else None
    except urllib.error.HTTPError as exc:
        if 300 <= int(exc.code or 0) < 400:
            return {
                "ok": False,
                "error": {
                    "code": "redirect_rejected",
                    "message": "Group Bridge authenticated delivery does not follow redirects",
                },
            }
        try:
            value = json.loads(exc.read().decode("utf-8") or "{}")
        except (OSError, json.JSONDecodeError):
            value = None
        if isinstance(value, Mapping):
            return value
        code = "peer_session_failed" if int(exc.code or 0) >= 500 else "remote_delivery_failed"
        return {
            "ok": False,
            "error": {"code": code, "message": f"remote Group Bridge HTTP status {exc.code}"},
        }
    except (OSError, urllib.error.URLError, json.JSONDecodeError):
        return None


def _session_unavailable(parsed: Mapping[str, Any]) -> bool:
    error = parsed.get("error") if isinstance(parsed.get("error"), Mapping) else {}
    return str(error.get("code") or "") in {
        "peer_session_unavailable",
        "peer_session_timeout",
        "peer_session_failed",
    }


def _result_from_response(parsed: Mapping[str, Any], *, transport: str) -> RemoteSendResult:
    if parsed.get("ok") is False or isinstance(parsed.get("error"), dict):
        err = parsed.get("error") if isinstance(parsed.get("error"), dict) else {}
        code = str(err.get("code") or "remote_error")
        if code in {"peer_session_unavailable", "peer_session_timeout", "peer_session_failed"}:
            return transient_result(
                code,
                str(err.get("message") or parsed.get("error") or "remote Group Bridge WebSocket session is unavailable"),
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
        event = result.get("event")
        if isinstance(event, Mapping) and event.get("id"):
            return str(event.get("id"))
    return ""
