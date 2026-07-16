"""HTTP issuer-endpoint pairing helpers for Stage B Group Bridge pairing."""

from __future__ import annotations

import hashlib
import ipaddress
import json
import socket
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, Optional
from urllib.error import HTTPError
from urllib.parse import parse_qs, urlencode, urlparse, urlunparse
from urllib.request import HTTPRedirectHandler, Request, build_opener

from ...util.time import utc_now_iso
from .pairing import get_local_identity, get_pairing_outbound, upsert_pairing_outbound
from .pairing_outbound_sync import approve_outbound_from_remote_request
from ..settings import resolve_remote_access_web_binding

_REMOTE_REQUEST_PATH = "/api/group-bridge/pairing/requests/remote"
_REMOTE_REQUEST_STATUS_PATH = "/api/group-bridge/pairing/requests/remote/status"
_REMOTE_ERROR_DETAIL_LIMIT = 120


@dataclass(frozen=True)
class ConnectionPayload:
    pairing_code: str
    issuer_endpoint: str = ""
    issuer_group_id: str = ""
    issuer_group_title: str = ""
    issuer_peer_id: str = ""
    issuer_node_id: str = ""
    expires_at: str = ""
    nonce: str = ""
    integrity: str = ""
    is_remote: bool = False


class _SafeRemotePairingError(ValueError):
    """Safe, user-facing pairing failure detail that can be persisted."""


def build_connection_payload(
    invite: Dict[str, Any],
    *,
    issuer_endpoint: str = "",
    issuer_group_title: str = "",
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    identity = get_local_identity(home=home)
    endpoint = normalize_issuer_endpoint(issuer_endpoint, allow_localhost=True, allow_private=True) if issuer_endpoint else ""
    payload = {
        "type": "cccc.group_bridge_session.connection_info",
        "version": 2,
        "issuer_endpoint": endpoint,
        "issuer_group_id": str(invite.get("group_id") or "").strip(),
        "issuer_group_title": str(issuer_group_title or "").strip(),
        "issuer_peer_id": str(identity.get("peer_id") or "").strip(),
        "issuer_node_id": str(identity.get("node_id") or "").strip(),
        "code": str(invite.get("pairing_code") or "").strip(),
        "expires_at": str(invite.get("expires_at") or "").strip(),
        "nonce": str(invite.get("invite_id") or "").strip(),
    }
    payload["integrity"] = _integrity(payload)
    return payload


def parse_connection_payload(raw: Any) -> ConnectionPayload:
    payload: Any = raw
    if isinstance(raw, str):
        value = _strip_code_fence(raw.strip())
        try:
            payload = json.loads(value)
        except Exception:
            return ConnectionPayload(pairing_code=_normalize_code(value))
    if isinstance(payload, str):
        return ConnectionPayload(pairing_code=_normalize_code(payload))
    if not isinstance(payload, dict):
        return ConnectionPayload(pairing_code="")
    endpoint = str(payload.get("issuer_endpoint") or payload.get("issuerEndpoint") or "").strip()
    code = str(payload.get("code") or payload.get("pairing_code") or payload.get("pairingCode") or "").strip()
    return ConnectionPayload(
        pairing_code=_normalize_code(code),
        issuer_endpoint=endpoint,
        issuer_group_id=str(payload.get("issuer_group_id") or payload.get("issuerGroupId") or payload.get("group_id") or payload.get("groupId") or "").strip(),
        issuer_group_title=str(payload.get("issuer_group_title") or payload.get("issuerGroupTitle") or "").strip(),
        issuer_peer_id=str(payload.get("issuer_peer_id") or payload.get("issuerPeerId") or payload.get("peer_id") or payload.get("peerId") or "").strip(),
        issuer_node_id=str(payload.get("issuer_node_id") or payload.get("issuerNodeId") or payload.get("node_id") or payload.get("nodeId") or "").strip(),
        expires_at=str(payload.get("expires_at") or payload.get("expiresAt") or "").strip(),
        nonce=str(payload.get("nonce") or payload.get("invite_id") or payload.get("inviteId") or "").strip(),
        integrity=str(payload.get("integrity") or "").strip(),
        is_remote=bool(endpoint),
    )


def normalize_issuer_endpoint(raw: str, *, allow_localhost: bool = True, allow_private: bool = True) -> str:
    value = str(raw or "").strip()
    if not value:
        raise ValueError("issuer_endpoint is required")
    parsed = urlparse(value if "://" in value else f"https://{value}")
    if parsed.scheme not in ("http", "https") or not parsed.netloc:
        raise ValueError("issuer_endpoint must be an http(s) URL")
    host = str(parsed.hostname or "").strip().lower()
    if not host:
        raise ValueError("issuer_endpoint host is required")
    _reject_unsafe_host(host, allow_localhost=allow_localhost, allow_private=allow_private)
    return urlunparse((parsed.scheme, parsed.netloc, "", "", "", "")).rstrip("/")


def issuer_remote_request_url(endpoint: str, *, allow_localhost: bool = True) -> str:
    return normalize_issuer_endpoint(endpoint, allow_localhost=allow_localhost, allow_private=True) + _REMOTE_REQUEST_PATH


def issuer_remote_request_status_url(
    endpoint: str,
    *,
    request_id: str,
    invite_id: str,
    allow_localhost: bool = True,
) -> str:
    query = urlencode({"request_id": str(request_id or "").strip(), "invite_id": str(invite_id or "").strip()})
    return normalize_issuer_endpoint(endpoint, allow_localhost=allow_localhost, allow_private=True) + _REMOTE_REQUEST_STATUS_PATH + "?" + query


def submit_remote_pairing_request(
    payload: Any,
    *,
    local_group_id: str,
    local_group_title: str = "",
    client: Optional[Callable[..., Dict[str, Any]]] = None,
    allow_localhost: bool = True,
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    local_gid = str(local_group_id or "").strip()
    if not local_gid:
        raise ValueError("local_group_id is required")
    parsed = parse_connection_payload(payload)
    if not parsed.is_remote:
        raise ValueError("issuer_endpoint is required for remote pairing")
    if not parsed.pairing_code:
        raise ValueError("pairing_code is required")
    if not parsed.issuer_group_title:
        raise ValueError("issuer_group_title is required")
    _validate_integrity(parsed)
    endpoint = issuer_remote_request_url(parsed.issuer_endpoint, allow_localhost=allow_localhost)
    identity = get_local_identity(home=home)
    body = {
        "pairing_code": parsed.pairing_code,
        "invite_id": parsed.nonce,
        "requester_group_id": local_gid,
        "requester_group_title": str(local_group_title or "").strip(),
        "requester_endpoint": _requester_endpoint(),
        "requester_peer_id": str(identity.get("peer_id") or ""),
        "requester_node_id": str(identity.get("node_id") or ""),
        "requester_multiaddrs": [],
    }
    caller = client or _default_remote_client
    try:
        result = caller(endpoint, body, timeout_seconds=3.0)
        status = "submitted"
        error = ""
        request = result.get("request") if isinstance(result, dict) else None
    except Exception as exc:
        status = "failed"
        error = _safe_error(exc)
        request = None
    outbound = {
        "outbound_id": "pout_" + hashlib.sha256(f"{endpoint}|{local_gid}|{parsed.nonce}|{parsed.pairing_code}".encode("utf-8")).hexdigest()[:16],
        "status": status,
        "local_group_id": local_gid,
        "issuer_endpoint": normalize_issuer_endpoint(parsed.issuer_endpoint, allow_localhost=allow_localhost, allow_private=True),
        "issuer_group_id": parsed.issuer_group_id,
        "issuer_group_title": parsed.issuer_group_title,
        "issuer_peer_id": parsed.issuer_peer_id,
        "invite_id": parsed.nonce,
        "remote_request": request or {},
        "last_error": error,
        "updated_at": utc_now_iso(),
    }
    return upsert_pairing_outbound(outbound, home=home)


def _requester_endpoint() -> str:
    return str(resolve_remote_access_web_binding().get("web_public_url") or "").strip()


def sync_remote_pairing_outbound(
    outbound_id: str,
    *,
    client: Optional[Callable[..., Dict[str, Any]]] = None,
    allow_localhost: bool = True,
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    outbound = get_pairing_outbound(outbound_id, home=home)
    if not outbound:
        raise ValueError("pairing outbound not found")
    remote_request = outbound.get("remote_request") if isinstance(outbound.get("remote_request"), dict) else {}
    request_id = str(remote_request.get("request_id") or "").strip()
    invite_id = str(outbound.get("invite_id") or remote_request.get("invite_id") or "").strip()
    if not request_id or not invite_id:
        raise ValueError("pairing outbound is missing remote request identity")
    endpoint = issuer_remote_request_status_url(
        str(outbound.get("issuer_endpoint") or ""),
        request_id=request_id,
        invite_id=invite_id,
        allow_localhost=allow_localhost,
    )
    caller = client or _default_remote_status_client
    try:
        result = caller(endpoint, timeout_seconds=3.0)
        request = result.get("request") if isinstance(result, dict) else None
        if not isinstance(request, dict):
            raise ValueError("remote pairing status unavailable")
        status = str(request.get("status") or "").strip()
        if status == "approved":
            approved = approve_outbound_from_remote_request(outbound["outbound_id"], request, home=home)
            return approved["outbound"]
        if status == "rejected":
            return upsert_pairing_outbound({**outbound, "status": "rejected", "remote_request": request, "last_error": str(request.get("rejection_reason") or "remote pairing request rejected")}, home=home)
        return upsert_pairing_outbound({**outbound, "status": status or str(outbound.get("status") or "submitted"), "remote_request": request, "last_error": ""}, home=home)
    except Exception as exc:
        return upsert_pairing_outbound({
            **outbound,
            "status": str(outbound.get("status") or "submitted"),
            "last_error": _safe_status_error(exc),
        }, home=home)


def _default_remote_client(endpoint: str, body: Dict[str, Any], *, timeout_seconds: float = 3.0) -> Dict[str, Any]:
    req = Request(
        endpoint,
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    opener = build_opener(_NoRedirectHandler)
    try:
        with opener.open(req, timeout=timeout_seconds) as resp:  # nosec B310 - endpoint is policy-normalized above
            parsed = json.loads(resp.read().decode("utf-8"))
    except HTTPError as exc:
        raise _SafeRemotePairingError(_safe_http_error("remote pairing request", exc, body)) from exc
    except json.JSONDecodeError as exc:
        raise _SafeRemotePairingError("remote pairing request returned invalid JSON") from exc
    if not isinstance(parsed, dict) or not parsed.get("ok"):
        raise _SafeRemotePairingError(_safe_response_error("remote pairing request", parsed, body))
    result = parsed.get("result")
    return dict(result) if isinstance(result, dict) else {}


def _default_remote_status_client(endpoint: str, *, timeout_seconds: float = 3.0) -> Dict[str, Any]:
    req = Request(endpoint, headers={"Accept": "application/json"}, method="GET")
    opener = build_opener(_NoRedirectHandler)
    redactions = _status_endpoint_redactions(endpoint)
    try:
        with opener.open(req, timeout=timeout_seconds) as resp:  # nosec B310 - endpoint is policy-normalized above
            parsed = json.loads(resp.read().decode("utf-8"))
    except HTTPError as exc:
        raise _SafeRemotePairingError(_safe_http_error("remote pairing status", exc, redactions)) from exc
    except json.JSONDecodeError as exc:
        raise _SafeRemotePairingError("remote pairing status returned invalid JSON") from exc
    if not isinstance(parsed, dict) or not parsed.get("ok"):
        raise _SafeRemotePairingError(_safe_response_error("remote pairing status", parsed, redactions))
    result = parsed.get("result")
    return dict(result) if isinstance(result, dict) else {}


def _strip_code_fence(value: str) -> str:
    if value.startswith("```") and value.endswith("```"):
        lines = value.splitlines()
        if len(lines) >= 3:
            return "\n".join(lines[1:-1]).strip()
    return value


def _normalize_code(value: str) -> str:
    return str(value or "").strip().strip("\"'").strip().upper()


def _integrity(payload: Dict[str, Any]) -> str:
    material = "|".join(str(payload.get(k) or "") for k in ("issuer_endpoint", "issuer_group_id", "issuer_group_title", "issuer_peer_id", "code", "expires_at", "nonce"))
    return "sha256:" + hashlib.sha256(material.encode("utf-8")).hexdigest()


def _validate_integrity(payload: ConnectionPayload) -> None:
    if not payload.integrity:
        raise ValueError("connection info integrity is required")
    if not payload.nonce:
        raise ValueError("connection info nonce is required")
    expected = _integrity(
        {
            "issuer_endpoint": normalize_issuer_endpoint(payload.issuer_endpoint, allow_localhost=True, allow_private=True),
            "issuer_group_id": payload.issuer_group_id,
            "issuer_group_title": payload.issuer_group_title,
            "issuer_peer_id": payload.issuer_peer_id,
            "code": payload.pairing_code,
            "expires_at": payload.expires_at,
            "nonce": payload.nonce,
        }
    )
    if payload.integrity and not _constant_time_equal(payload.integrity, expected):
        raise ValueError("connection info integrity mismatch")


def _constant_time_equal(a: str, b: str) -> bool:
    return hashlib.sha256(str(a or "").encode("utf-8")).digest() == hashlib.sha256(str(b or "").encode("utf-8")).digest()


def _reject_unsafe_host(host: str, *, allow_localhost: bool, allow_private: bool) -> None:
    if host in ("localhost",):
        if allow_localhost:
            return
        raise ValueError("localhost issuer_endpoint is not allowed")
    try:
        addrs = [ipaddress.ip_address(host)]
    except ValueError:
        try:
            addrs = [ipaddress.ip_address(item[4][0]) for item in socket.getaddrinfo(host, None)]
        except Exception:
            addrs = []
    for addr in addrs:
        if addr.is_loopback:
            if allow_localhost:
                continue
            raise ValueError("loopback issuer_endpoint is not allowed")
        if addr.is_link_local or addr.is_multicast or addr.is_unspecified or addr.is_reserved:
            raise ValueError("unsafe issuer_endpoint is not allowed")
        if addr.is_private and not allow_private:
            raise ValueError("private issuer_endpoint is not allowed")


def _safe_http_error(prefix: str, exc: HTTPError, redactions: Dict[str, Any]) -> str:
    detail = ""
    try:
        raw = exc.read().decode("utf-8", errors="replace")
        parsed = json.loads(raw) if raw else {}
        detail = _safe_remote_detail(parsed, redactions)
    except Exception:
        detail = ""
    message = f"{prefix} HTTP {int(exc.code)}"
    if detail:
        message = f"{message}: {detail}"
    return message[:240]


def _safe_response_error(prefix: str, parsed: Any, redactions: Dict[str, Any]) -> str:
    detail = _safe_remote_detail(parsed, redactions)
    if detail:
        return f"{prefix} failed: {detail}"[:240]
    return f"{prefix} failed"


def _status_endpoint_redactions(endpoint: str) -> Dict[str, Any]:
    try:
        query = parse_qs(urlparse(str(endpoint or "")).query)
    except Exception:
        return {}
    return {
        "invite_id": (query.get("invite_id") or [""])[0],
    }


def _safe_remote_detail(parsed: Any, redactions: Dict[str, Any]) -> str:
    if not isinstance(parsed, dict):
        return ""
    detail = parsed.get("detail")
    error = parsed.get("error")
    source = detail if isinstance(detail, dict) else error if isinstance(error, dict) else parsed
    if not isinstance(source, dict):
        return ""
    code = str(source.get("code") or "").strip()
    message = str(source.get("message") or "").strip()
    if code and message:
        text = f"{code}: {message}"
    else:
        text = message or code
    return _redact_remote_detail(text, redactions)


def _redact_remote_detail(text: str, redactions: Dict[str, Any]) -> str:
    out = str(text or "").strip()
    for key in ("pairing_code", "invite_id"):
        secret = str(redactions.get(key) or "").strip()
        if secret:
            out = out.replace(secret, "[redacted]")
    if len(out) > _REMOTE_ERROR_DETAIL_LIMIT:
        out = out[: _REMOTE_ERROR_DETAIL_LIMIT - 1].rstrip() + "…"
    return out


def _safe_error(exc: Exception) -> str:
    if isinstance(exc, _SafeRemotePairingError):
        return str(exc)[:240]
    if isinstance(exc, TimeoutError):
        return "remote pairing request timed out"
    if isinstance(exc, ValueError):
        msg = str(exc or "").lower()
        if "issuer_endpoint" in msg or "redirect" in msg:
            return str(exc)[:120]
    return "remote pairing request failed"


def _safe_status_error(exc: Exception) -> str:
    if isinstance(exc, _SafeRemotePairingError):
        return str(exc)[:240]
    if isinstance(exc, TimeoutError):
        return "remote pairing status timed out"
    if isinstance(exc, ValueError):
        msg = str(exc or "").lower()
        if "issuer_endpoint" in msg or "redirect" in msg:
            return str(exc)[:120]
    return "remote pairing status unavailable"


class _NoRedirectHandler(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):  # type: ignore[no-untyped-def]
        raise ValueError("remote pairing redirect is not allowed")
