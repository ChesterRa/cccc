"""Transport policy and safe diagnostics for remote Group Bridge pairing."""

from __future__ import annotations

import socket
import ssl
from typing import Optional, Tuple
from urllib.error import URLError

REMOTE_PAIRING_TIMEOUT_SECONDS = 15.0
_TRANSPORT_DETAIL_LIMIT = 120


def format_transport_error(
    prefix: str,
    exc: Exception,
    *,
    timeout_seconds: float = REMOTE_PAIRING_TIMEOUT_SECONDS,
) -> Optional[str]:
    """Return a safe, actionable transport error or ``None`` for non-I/O failures."""

    classified = _classify_transport_error(exc)
    if classified is None:
        return None
    category, detail = classified
    if category == "timeout":
        summary = f"{prefix} failed (timeout after {_duration_label(timeout_seconds)})"
    else:
        summary = f"{prefix} failed ({category})"
    return f"{summary}: {detail}"[:240] if detail else summary


def _classify_transport_error(exc: Exception) -> Optional[Tuple[str, str]]:
    reason = exc.reason if isinstance(exc, URLError) else exc
    detail = _safe_detail(reason)
    lowered = detail.lower()

    if isinstance(reason, (TimeoutError, socket.timeout)) or "timed out" in lowered:
        return "timeout", detail
    if isinstance(reason, socket.gaierror) or _contains_any(
        lowered,
        (
            "dns",
            "name or service not known",
            "nodename nor servname",
            "no such host",
            "temporary failure in name resolution",
        ),
    ):
        return "dns", detail
    if isinstance(reason, ssl.SSLError) or _contains_any(
        lowered,
        ("tls", "ssl", "certificate", "handshake"),
    ):
        return "tls", detail
    if _contains_any(lowered, ("proxy", "tunnel connection")):
        return "proxy", detail
    if isinstance(reason, (ConnectionError, OSError)) or isinstance(exc, URLError):
        return "connect", detail
    return None


def _contains_any(value: str, needles: Tuple[str, ...]) -> bool:
    return any(needle in value for needle in needles)


def _safe_detail(reason: object) -> str:
    detail = " ".join(str(reason or "").split())
    if len(detail) > _TRANSPORT_DETAIL_LIMIT:
        return detail[: _TRANSPORT_DETAIL_LIMIT - 1].rstrip() + "…"
    return detail


def _duration_label(seconds: float) -> str:
    value = float(seconds)
    return f"{int(value)}s" if value.is_integer() else f"{value:g}s"
