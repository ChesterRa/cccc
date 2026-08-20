"""Account-plane HTTP client. Versioned, no path tokens, no business payloads."""

from __future__ import annotations

import json
import os
import ipaddress
import urllib.error
import urllib.request
from typing import Any, Callable, Dict, Optional
from urllib.parse import urljoin, urlparse

from .membership import normalize_reach_hostname

CLIENT_VERSION = 1
VERSION_HEADER = "CCCC-Membership-Version"
USER_AGENT = "cccc-membership"
DEFAULT_TIMEOUT_S = 15.0
MAX_RESPONSE_BYTES = 1024 * 1024


def _timeout_s(override: float | None = None) -> float:
    if override is not None:
        return override
    raw = os.environ.get("CCCC_ACCOUNT_TIMEOUT_S")
    if raw:
        try:
            return max(0.2, float(raw))
        except ValueError:
            pass
    return DEFAULT_TIMEOUT_S


Transport = Callable[
    [str, str, Dict[str, str], Optional[bytes], float], tuple[int, Dict[str, Any]]
]


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        return None


def _open_no_redirect(request: urllib.request.Request, timeout_s: float):
    return urllib.request.build_opener(_NoRedirect).open(request, timeout=timeout_s)


class AccountError(Exception):
    def __init__(
        self,
        code: str,
        message: str,
        *,
        retryable: bool = False,
        retry_after_delta: int = 0,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.message = message
        self.retryable = retryable
        self.retry_after_delta = max(0, int(retry_after_delta))


def _normalize_origin(origin: str) -> str:
    value = str(origin or "").strip().rstrip("/")
    if not value:
        raise AccountError(
            "membership_unavailable", "membership account service is not configured"
        )
    parsed = urlparse(value)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        raise AccountError(
            "membership_unavailable",
            "CCCC_ACCOUNT_ORIGIN must be an absolute http(s) URL",
        )
    hostname = parsed.hostname or ""
    loopback = hostname.lower() == "localhost"
    if not loopback:
        try:
            loopback = ipaddress.ip_address(hostname).is_loopback
        except ValueError:
            loopback = False
    if parsed.scheme == "http" and not loopback:
        raise AccountError(
            "membership_unavailable",
            "CCCC_ACCOUNT_ORIGIN must use HTTPS except for a loopback development server",
        )
    return value


def _decode_body(raw: bytes) -> Dict[str, Any]:
    if not raw:
        return {}
    try:
        payload = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise AccountError(
            "membership_network", "account service returned a non-JSON body"
        ) from exc
    return payload if isinstance(payload, dict) else {}


def _read_body(response: Any) -> Dict[str, Any]:
    raw = response.read(MAX_RESPONSE_BYTES + 1)
    if len(raw) > MAX_RESPONSE_BYTES:
        raise AccountError(
            "membership_network", "account service response exceeded size limit"
        )
    return _decode_body(raw)


def _error_from_payload(status: int, payload: Dict[str, Any]) -> AccountError:
    nested = payload.get("error")
    code = ""
    message = ""
    if isinstance(nested, dict):
        code = str(nested.get("code") or "").strip()
        message = str(nested.get("message") or "").strip()
    elif isinstance(nested, str):
        code = nested.strip()
        message = str(payload.get("error_description") or nested).strip()
    if code == "authorization_pending":
        return AccountError(
            "membership_authorization_pending", message or code, retryable=True
        )
    if code == "slow_down":
        return AccountError(
            "membership_authorization_pending",
            message or code,
            retryable=True,
            retry_after_delta=5,
        )
    if code in {"expired_token", "expired"}:
        return AccountError("membership_network", message or "device code expired")
    if code in {"access_denied", "denied"}:
        return AccountError("membership_gate", message or "login was denied")
    if code in {"unsupported_version", "version_unsupported"} or status == 426:
        return AccountError(
            "membership_unsupported_version", message or "please upgrade CCCC"
        )
    if code in {"disabled", "device_disabled"} or status == 403:
        return AccountError(
            "membership_disabled", message or "this device has been disabled"
        )
    if status in {401, 404}:
        return AccountError("membership_not_logged_in", message or "not logged in")
    return AccountError(
        "membership_network",
        message or f"account service rejected the request ({status})",
    )


def default_transport(
    method: str,
    url: str,
    headers: Dict[str, str],
    body: Optional[bytes],
    timeout_s: float,
) -> tuple[int, Dict[str, Any]]:
    request = urllib.request.Request(url, data=body, method=method, headers=headers)
    try:
        with _open_no_redirect(request, timeout_s) as response:
            return int(response.status), _read_body(response)
    except urllib.error.HTTPError as exc:
        return int(exc.code), _read_body(exc)
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise AccountError(
            "membership_network", f"account service is not reachable: {exc}"
        ) from exc


def request(
    origin: str,
    method: str,
    path: str,
    *,
    payload: Optional[Dict[str, Any]] = None,
    token: Optional[str] = None,
    transport: Optional[Transport] = None,
    timeout_s: Optional[float] = None,
) -> Dict[str, Any]:
    base = _normalize_origin(origin)
    headers = {
        "Accept": "application/json",
        "User-Agent": USER_AGENT,
        VERSION_HEADER: str(CLIENT_VERSION),
    }
    body: Optional[bytes] = None
    if payload is not None:
        body = json.dumps(payload).encode("utf-8")
        headers["Content-Type"] = "application/json"
    if token:
        headers["Authorization"] = f"Bearer {token}"
    send = transport or default_transport
    status, data = send(
        method,
        urljoin(base + "/", path.lstrip("/")),
        headers,
        body,
        _timeout_s(timeout_s),
    )
    if not 200 <= status < 300:
        raise _error_from_payload(status, data)
    return data


def start_device_login(
    origin: str,
    *,
    transport: Optional[Transport] = None,
) -> Dict[str, Any]:
    data = request(origin, "POST", "/v1/device/code", payload={}, transport=transport)
    device_code = str(data.get("device_code") or "").strip()
    user_code = str(data.get("user_code") or "").strip()
    verification_uri = str(
        data.get("verification_uri") or data.get("verification_uri_complete") or ""
    ).strip()
    if not device_code or not user_code or not verification_uri:
        raise AccountError(
            "membership_network", "account service returned an incomplete device code"
        )
    try:
        expires_in = int(data.get("expires_in") or 900)
    except (TypeError, ValueError):
        expires_in = 900
    try:
        interval = int(data.get("interval") or 5)
    except (TypeError, ValueError):
        interval = 5
    return {
        "device_code": device_code,
        "user_code": user_code,
        "verification_uri": verification_uri,
        "expires_in": max(30, expires_in),
        "interval": max(1, interval),
    }


def poll_device_login(
    origin: str,
    device_code: str,
    *,
    transport: Optional[Transport] = None,
) -> Dict[str, Any]:
    data = request(
        origin,
        "POST",
        "/v1/device/token",
        payload={
            "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
            "device_code": device_code,
        },
        transport=transport,
    )
    token = str(data.get("access_token") or data.get("device_token") or "").strip()
    device_id = str(data.get("device_id") or "").strip()
    raw_hostname = str(data.get("hostname") or "").strip()
    hostname = normalize_reach_hostname(raw_hostname)
    if not token or not device_id:
        raise AccountError(
            "membership_network", "account service returned an incomplete device grant"
        )
    if raw_hostname and hostname is None:
        raise AccountError(
            "membership_network", "account service returned an unsafe reach hostname"
        )
    return {
        "device_token": token,
        "device_id": device_id,
        "hostname": hostname,
    }


def issue_reach(
    origin: str,
    device_token: str,
    *,
    origin_port: int = 8848,
    transport: Optional[Transport] = None,
) -> Dict[str, Any]:
    if (
        isinstance(origin_port, bool)
        or not isinstance(origin_port, int)
        or not 1 <= origin_port <= 65535
    ):
        raise AccountError(
            "membership_network", "origin_port must be an integer between 1 and 65535"
        )
    data = request(
        origin,
        "POST",
        "/v1/reach",
        payload={"origin_port": origin_port},
        token=device_token,
        transport=transport,
    )
    hostname = normalize_reach_hostname(data.get("hostname"))
    tunnel_token = str(data.get("tunnel_token") or "").strip()
    if not hostname or not tunnel_token:
        raise AccountError(
            "membership_network",
            "account service returned incomplete or unsafe reach credentials",
        )
    return {"hostname": hostname, "tunnel_token": tunnel_token}


def fetch_device(
    origin: str,
    device_token: str,
    *,
    transport: Optional[Transport] = None,
) -> Dict[str, Any]:
    data = request(origin, "GET", "/v1/device", token=device_token, transport=transport)
    online = data.get("online")
    return {
        "device_id": str(data.get("device_id") or "").strip() or None,
        "hostname": normalize_reach_hostname(data.get("hostname")),
        "disabled": bool(data.get("disabled")),
        "online": online if isinstance(online, bool) else None,
    }
