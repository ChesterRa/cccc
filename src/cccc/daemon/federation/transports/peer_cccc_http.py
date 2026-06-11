"""MVP transport: deliver a remote message to a peer CCCC instance over HTTP.

Text-only for the MVP (no attachments/refs/streaming). The HTTP poster is
injectable so the transport is unit-testable without real network I/O.
"""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any, Callable, Dict, Tuple

from .base import (
    RemoteMessageEnvelope,
    RemoteSendResult,
    RemoteSendTransport,
    permanent_result,
    sent_result,
    transient_result,
)

# (url, body, credential) -> (http_status, response_json)
HttpPost = Callable[[str, Dict[str, Any], str], Tuple[int, Dict[str, Any]]]

_TRANSIENT_STATUSES = {408, 425, 429, 500, 502, 503, 504}


def _extract_remote_event_id(parsed: Dict[str, Any]) -> str:
    """Read the remote event id from common CCCC response shapes:
    top-level ``event_id`` / ``id``, or CCCC result envelopes."""
    if not isinstance(parsed, dict):
        return ""
    direct = str(parsed.get("event_id") or parsed.get("id") or "").strip()
    if direct:
        return direct
    result = parsed.get("result")
    if isinstance(result, dict):
        legacy = str(result.get("event_id") or "").strip()
        if legacy:
            return legacy
        event = result.get("event")
        if isinstance(event, dict):
            return str(event.get("id") or "").strip()
    return ""


def _extract_error(parsed: Dict[str, Any]) -> tuple[str, str]:
    error = parsed.get("error") if isinstance(parsed, dict) else None
    if isinstance(error, dict):
        code = str(error.get("code") or "remote_rejected").strip() or "remote_rejected"
        message = str(error.get("message") or code).strip() or code
        return code, message
    return "remote_rejected", "remote returned an unsuccessful CCCC response"


def _is_failed_cccc_envelope(parsed: Dict[str, Any]) -> bool:
    if not isinstance(parsed, dict):
        return False
    if parsed.get("ok") is False:
        return True
    return isinstance(parsed.get("error"), dict) and not _extract_remote_event_id(parsed)


def _default_http_post(url: str, body: Dict[str, Any], credential: str) -> Tuple[int, Dict[str, Any]]:
    data = json.dumps(body).encode("utf-8")
    req = urllib.request.Request(url, data=data, method="POST")
    req.add_header("Content-Type", "application/json")
    if credential:
        req.add_header("Authorization", f"Bearer {credential}")
    with urllib.request.urlopen(req, timeout=15) as resp:  # noqa: S310 - SSRF guard is a separate stage gate
        raw = resp.read().decode("utf-8", errors="replace")
        try:
            parsed = json.loads(raw) if raw else {}
        except Exception:
            parsed = {}
        return int(getattr(resp, "status", 200) or 200), parsed if isinstance(parsed, dict) else {}


class PeerCcccHttpTransport(RemoteSendTransport):
    transport = "peer_cccc_http"
    capabilities = frozenset()  # MVP: text only

    def __init__(self, *, http_post: HttpPost | None = None) -> None:
        self._http_post: HttpPost = http_post or _default_http_post

    def deliver(self, envelope: RemoteMessageEnvelope) -> RemoteSendResult:
        gate = self.unsupported_payload(envelope.payload)
        if gate is not None:
            return gate

        url = f"{envelope.target.url}/api/v1/groups/{envelope.target.remote_group_id}/send"
        body = {
            "text": envelope.payload.text,
            "to": list(envelope.payload.to),
            "priority": envelope.payload.priority,
            "reply_required": envelope.payload.reply_required,
            "idempotency_key": envelope.idempotency_key,
        }

        try:
            status, parsed = self._http_post(url, body, envelope.credential)
        except urllib.error.HTTPError as e:  # pragma: no cover - exercised via injected fakes
            return self._classify(int(getattr(e, "code", 0) or 0), {})
        except (urllib.error.URLError, ConnectionError, TimeoutError, OSError) as e:
            return transient_result("connection_error", str(e), transport=self.transport)
        except Exception as e:  # pragma: no cover - defensive
            return permanent_result("transport_error", str(e), transport=self.transport)

        return self._classify(int(status or 0), parsed if isinstance(parsed, dict) else {})

    def _classify(self, status: int, parsed: Dict[str, Any]) -> RemoteSendResult:
        if 200 <= status < 300:
            if _is_failed_cccc_envelope(parsed):
                code, message = _extract_error(parsed)
                return permanent_result(code, message, transport=self.transport)
            return sent_result(_extract_remote_event_id(parsed), transport=self.transport)
        if status in _TRANSIENT_STATUSES:
            return transient_result("remote_unavailable", f"remote returned {status}", transport=self.transport)
        return permanent_result("remote_rejected", f"remote returned {status}", transport=self.transport)
