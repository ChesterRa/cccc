from __future__ import annotations

import json
import socket
import threading
from typing import Any, Dict, Optional

from ..contracts.v1 import DaemonError, DaemonResponse


def recv_json_line(conn: socket.socket) -> Dict[str, Any]:
    buf = b""
    while b"\n" not in buf:
        chunk = conn.recv(65536)
        if not chunk:
            break
        buf += chunk
        if len(buf) > 2_000_000:
            break
    line = buf.split(b"\n", 1)[0]
    try:
        return json.loads(line.decode("utf-8", errors="replace"))
    except Exception:
        return {}


def send_json(conn: socket.socket, obj: Dict[str, Any]) -> None:
    data = (json.dumps(obj, ensure_ascii=False) + "\n").encode("utf-8")
    conn.sendall(data)


def dump_response(resp: Any) -> Dict[str, Any]:
    if resp is None:
        return {"ok": False, "error": {"code": "internal_error", "message": "invalid daemon response: None"}}
    try:
        fn = getattr(resp, "model_dump", None)
        if callable(fn):
            return fn()
    except Exception:
        pass
    try:
        fn = getattr(resp, "dict", None)
        if callable(fn):
            out = fn()
            if isinstance(out, dict):
                return out
    except Exception:
        pass
    if isinstance(resp, dict):
        return resp
    return {
        "ok": False,
        "error": {"code": "internal_error", "message": f"invalid daemon response type: {type(resp).__name__}"},
    }


def supported_stream_kinds() -> set[str]:
    try:
        from .streaming import STREAMABLE_KINDS_V1

        return set(STREAMABLE_KINDS_V1)
    except Exception:
        return set()


def start_events_stream(
    *,
    sock: socket.socket,
    group_id: str,
    by: str,
    kinds: Optional[set[str]],
    since_event_id: str,
    since_ts: str,
) -> bool:
    try:
        from .streaming import stream_events_to_socket

        threading.Thread(
            target=stream_events_to_socket,
            kwargs={
                "sock": sock,
                "group_id": group_id,
                "by": by,
                "kinds": kinds,
                "since_event_id": since_event_id,
                "since_ts": since_ts,
            },
            daemon=True,
            name=f"cccc-events-{group_id[:8]}",
        ).start()
        return True
    except Exception:
        return False


def error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))
