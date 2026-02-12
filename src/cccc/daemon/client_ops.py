"""Daemon IPC client helpers."""

from __future__ import annotations

import json
import socket
from pathlib import Path
from typing import Any, Dict


def send_daemon_request(
    endpoint: Dict[str, Any],
    request_payload: Dict[str, Any],
    *,
    timeout_s: float,
    sock_path_default: Path,
) -> Dict[str, Any]:
    transport = str(endpoint.get("transport") or "").strip().lower()
    if transport == "tcp":
        host = str(endpoint.get("host") or "127.0.0.1").strip() or "127.0.0.1"
        try:
            port = int(endpoint.get("port") or 0)
        except Exception:
            port = 0
        if port <= 0:
            raise RuntimeError("invalid tcp daemon endpoint")
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        try:
            sock.settimeout(timeout_s)
            sock.connect((host, port))
            sock.sendall((json.dumps(request_payload, ensure_ascii=False) + "\n").encode("utf-8"))
            with sock.makefile("rb") as f:
                line = f.readline(4_000_000)
        finally:
            try:
                sock.close()
            except Exception:
                pass
    else:
        af_unix = getattr(socket, "AF_UNIX", None)
        if af_unix is None:
            raise RuntimeError("AF_UNIX not supported")
        path = str(endpoint.get("path") or sock_path_default)
        sock = socket.socket(af_unix, socket.SOCK_STREAM)
        try:
            sock.settimeout(timeout_s)
            sock.connect(path)
            sock.sendall((json.dumps(request_payload, ensure_ascii=False) + "\n").encode("utf-8"))
            with sock.makefile("rb") as f:
                line = f.readline(4_000_000)
        finally:
            try:
                sock.close()
            except Exception:
                pass
    return json.loads(line.decode("utf-8", errors="replace"))
