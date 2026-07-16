"""Daemon IPC client helpers."""

from __future__ import annotations

import json
import socket
from pathlib import Path
from typing import Any, Dict


class DaemonClientError(RuntimeError):
    """Transport/client-side IPC failure with phase-level diagnostics."""

    def __init__(
        self,
        *,
        phase: str,
        reason: str,
        transport: str,
        endpoint: Dict[str, Any],
        op: str,
        timeout_s: float,
        cause: BaseException | None = None,
    ) -> None:
        self.phase = str(phase or "").strip() or "unknown"
        self.reason = str(reason or "").strip() or "unknown"
        self.transport = str(transport or "").strip() or "unknown"
        self.endpoint = dict(endpoint) if isinstance(endpoint, dict) else {}
        self.op = str(op or "").strip() or "unknown"
        self.timeout_s = float(timeout_s or 0.0)
        self.cause = cause
        self.error_type = type(cause).__name__ if cause is not None else ""
        self.error = str(cause or "").strip()
        super().__init__(self._message())

    def _message(self) -> str:
        parts = [f"daemon client failure phase={self.phase}", f"reason={self.reason}", f"transport={self.transport}", f"op={self.op}"]
        if self.error_type:
            parts.append(f"error_type={self.error_type}")
        if self.error:
            parts.append(f"error={self.error}")
        return " ".join(parts)

    def details(self) -> Dict[str, Any]:
        details: Dict[str, Any] = {
            "phase": self.phase,
            "reason": self.reason,
            "transport": self.transport,
            "op": self.op,
            "timeout_s": self.timeout_s,
        }
        endpoint = _endpoint_summary(self.transport, self.endpoint)
        if endpoint:
            details["endpoint"] = endpoint
        if self.error_type:
            details["error_type"] = self.error_type
        if self.error:
            details["error"] = self.error
        return details


def _endpoint_summary(transport: str, endpoint: Dict[str, Any]) -> Dict[str, Any]:
    mode = str(transport or "").strip().lower()
    if mode == "tcp":
        return {
            "host": str(endpoint.get("host") or "").strip() or "127.0.0.1",
            "port": int(endpoint.get("port") or 0),
        }
    return {"path": str(endpoint.get("path") or "").strip()}


def _raise_client_error(
    *,
    phase: str,
    reason: str,
    transport: str,
    endpoint: Dict[str, Any],
    op: str,
    timeout_s: float,
    cause: BaseException | None = None,
) -> None:
    raise DaemonClientError(
        phase=phase,
        reason=reason,
        transport=transport,
        endpoint=endpoint,
        op=op,
        timeout_s=timeout_s,
        cause=cause,
    ) from cause


def _read_response_line(
    sock: socket.socket,
    *,
    transport: str,
    endpoint: Dict[str, Any],
    op: str,
    timeout_s: float,
) -> bytes:
    try:
        with sock.makefile("rb") as handle:
            line = handle.readline(4_000_000)
    except socket.timeout as exc:
        _raise_client_error(
            phase="read",
            reason="timeout",
            transport=transport,
            endpoint=endpoint,
            op=op,
            timeout_s=timeout_s,
            cause=exc,
        )
    except OSError as exc:
        _raise_client_error(
            phase="read",
            reason="os_error",
            transport=transport,
            endpoint=endpoint,
            op=op,
            timeout_s=timeout_s,
            cause=exc,
        )
    if not line:
        _raise_client_error(
            phase="read",
            reason="eof",
            transport=transport,
            endpoint=endpoint,
            op=op,
            timeout_s=timeout_s,
        )
    return line


def send_daemon_request(
    endpoint: Dict[str, Any],
    request_payload: Dict[str, Any],
    *,
    timeout_s: float,
    sock_path_default: Path,
) -> Dict[str, Any]:
    transport = str(endpoint.get("transport") or "").strip().lower()
    op = str(request_payload.get("op") or "").strip() or "unknown"
    if transport == "tcp":
        host = str(endpoint.get("host") or "127.0.0.1").strip() or "127.0.0.1"
        try:
            port = int(endpoint.get("port") or 0)
        except Exception:
            port = 0
        if port <= 0:
            _raise_client_error(
                phase="endpoint",
                reason="invalid_endpoint",
                transport="tcp",
                endpoint={"transport": "tcp", "host": host, "port": port},
                op=op,
                timeout_s=timeout_s,
            )
        resolved_endpoint = {"transport": "tcp", "host": host, "port": port}
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            sock.settimeout(timeout_s)
            try:
                sock.connect((host, port))
            except socket.timeout as exc:
                _raise_client_error(
                    phase="connect",
                    reason="timeout",
                    transport="tcp",
                    endpoint=resolved_endpoint,
                    op=op,
                    timeout_s=timeout_s,
                    cause=exc,
                )
            except OSError as exc:
                _raise_client_error(
                    phase="connect",
                    reason="os_error",
                    transport="tcp",
                    endpoint=resolved_endpoint,
                    op=op,
                    timeout_s=timeout_s,
                    cause=exc,
                )
            try:
                sock.sendall((json.dumps(request_payload, ensure_ascii=False) + "\n").encode("utf-8"))
            except socket.timeout as exc:
                _raise_client_error(
                    phase="send",
                    reason="timeout",
                    transport="tcp",
                    endpoint=resolved_endpoint,
                    op=op,
                    timeout_s=timeout_s,
                    cause=exc,
                )
            except OSError as exc:
                _raise_client_error(
                    phase="send",
                    reason="os_error",
                    transport="tcp",
                    endpoint=resolved_endpoint,
                    op=op,
                    timeout_s=timeout_s,
                    cause=exc,
                )
            line = _read_response_line(
                sock,
                transport="tcp",
                endpoint=resolved_endpoint,
                op=op,
                timeout_s=timeout_s,
            )
        finally:
            try:
                sock.close()
            except Exception:
                pass
    else:
        af_unix = getattr(socket, "AF_UNIX", None)
        if af_unix is None:
            _raise_client_error(
                phase="endpoint",
                reason="unsupported_transport",
                transport="unix",
                endpoint={"transport": "unix", "path": str(endpoint.get("path") or sock_path_default)},
                op=op,
                timeout_s=timeout_s,
            )
        path = str(endpoint.get("path") or sock_path_default)
        resolved_endpoint = {"transport": "unix", "path": path}
        sock = socket.socket(af_unix, socket.SOCK_STREAM)
        try:
            sock.settimeout(timeout_s)
            try:
                sock.connect(path)
            except socket.timeout as exc:
                _raise_client_error(
                    phase="connect",
                    reason="timeout",
                    transport="unix",
                    endpoint=resolved_endpoint,
                    op=op,
                    timeout_s=timeout_s,
                    cause=exc,
                )
            except OSError as exc:
                _raise_client_error(
                    phase="connect",
                    reason="os_error",
                    transport="unix",
                    endpoint=resolved_endpoint,
                    op=op,
                    timeout_s=timeout_s,
                    cause=exc,
                )
            try:
                sock.sendall((json.dumps(request_payload, ensure_ascii=False) + "\n").encode("utf-8"))
            except socket.timeout as exc:
                _raise_client_error(
                    phase="send",
                    reason="timeout",
                    transport="unix",
                    endpoint=resolved_endpoint,
                    op=op,
                    timeout_s=timeout_s,
                    cause=exc,
                )
            except OSError as exc:
                _raise_client_error(
                    phase="send",
                    reason="os_error",
                    transport="unix",
                    endpoint=resolved_endpoint,
                    op=op,
                    timeout_s=timeout_s,
                    cause=exc,
                )
            line = _read_response_line(
                sock,
                transport="unix",
                endpoint=resolved_endpoint,
                op=op,
                timeout_s=timeout_s,
            )
        finally:
            try:
                sock.close()
            except Exception:
                pass
    try:
        return json.loads(line.decode("utf-8", errors="replace"))
    except json.JSONDecodeError as exc:
        _raise_client_error(
            phase="decode",
            reason="invalid_json",
            transport=transport or "unix",
            endpoint=resolved_endpoint,
            op=op,
            timeout_s=timeout_s,
            cause=exc,
        )
