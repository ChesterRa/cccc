"""Client side of federation WebSocket sessions."""

from __future__ import annotations

import json
import threading
from typing import Any, Callable, Dict
from urllib.parse import urlparse, urlunparse

from ...kernel.federation.pairing import get_local_identity
from .ws_auth import sign_session_hello
from .ws_peer import ThreadFederationWsPeer
from .ws_session import FederationWsSession, register_session, unregister_session

WsConnect = Callable[[str, float], Any]
RequestHandler = Callable[[Dict[str, Any]], Dict[str, Any]]
ReadyHook = Callable[[], Any]


def federation_session_ws_url(base_url: str) -> str:
    raw = str(base_url or "").strip().rstrip("/")
    parsed = urlparse(raw if "://" in raw else f"http://{raw}")
    scheme = "wss" if parsed.scheme == "https" else "ws"
    return urlunparse((scheme, parsed.netloc, "/api/federation/session/ws", "", "", ""))


def connect_federation_session_once(
    *,
    remote_base_url: str,
    local_group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
    connect: WsConnect | None = None,
    handle_request: RequestHandler | None = None,
    on_ready: ReadyHook | None = None,
    timeout: float = 5.0,
) -> Dict[str, Any]:
    connector = connect or _default_connect
    try:
        ws = connector(federation_session_ws_url(remote_base_url), timeout)
    except Exception as exc:
        return {"ok": False, "error": {"code": "session_connect_failed", "message": str(exc)}}
    send_lock = threading.Lock()
    local_peer_id = str(get_local_identity().get("peer_id") or "").strip()
    try:
        _ws_send_json(
            ws,
            sign_session_hello({
                "target_group_id": str(remote_group_id or "").strip(),
                "src_group_id": str(local_group_id or "").strip(),
                "remote_peer_id": local_peer_id or str(remote_peer_id or "").strip(),
            }),
            send_lock=send_lock,
        )
        ready = _ws_recv_json(ws)
        if not bool(ready.get("ok")):
            return {"ok": False, "error": ready.get("error") or {"code": "session_rejected", "message": "remote rejected federation session"}}
        peer = ThreadFederationWsPeer(lambda payload: _ws_send_json(ws, payload, send_lock=send_lock))
        session = FederationWsSession(
            target_group_id=str(local_group_id or "").strip(),
            src_group_id=str(remote_group_id or "").strip(),
            remote_peer_id=str(remote_peer_id or "").strip(),
            send_request=lambda request, request_timeout: _send_request_async(peer, request, request_timeout),
        )
        _run_coro(register_session(session))
        handler = handle_request or (lambda frame: _default_handle_request(frame, remote_peer_id=remote_peer_id))
        try:
            if on_ready is not None:
                on_ready()
            while True:
                frame = _ws_recv_json(ws)
                frame_type = str(frame.get("type") or "").strip()
                if frame_type == "response":
                    peer.receive_response(frame)
                    continue
                if frame_type != "request":
                    continue
                request_id = str(frame.get("request_id") or "").strip()
                result = handler(frame)
                _ws_send_json(ws, {"type": "response", "response_to": request_id, "result": result}, send_lock=send_lock)
        finally:
            _run_coro(unregister_session(session))
    except Exception as exc:
        return {"ok": False, "error": {"code": "session_closed", "message": str(exc)}}
    finally:
        try:
            ws.close()
        except Exception:
            pass


def start_federation_session_client(
    *,
    remote_base_url: str,
    local_group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
    connect: WsConnect | None = None,
    handle_request: RequestHandler | None = None,
    retry_seconds: float = 5.0,
    stop: threading.Event | None = None,
) -> threading.Thread:
    stop_event = stop or threading.Event()

    def run() -> None:
        while not stop_event.is_set():
            connect_federation_session_once(
                remote_base_url=remote_base_url,
                local_group_id=local_group_id,
                remote_group_id=remote_group_id,
                remote_peer_id=remote_peer_id,
                connect=connect,
                handle_request=handle_request,
            )
            stop_event.wait(max(0.1, float(retry_seconds or 0.1)))

    thread = threading.Thread(target=run, name="cccc-federation-ws-client", daemon=True)
    thread.start()
    return thread


def _default_connect(url: str, timeout: float) -> Any:
    try:
        from websocket import create_connection
    except Exception as exc:  # pragma: no cover - dependency is declared but keep error crisp
        raise RuntimeError("websocket-client is required for federation WebSocket sessions") from exc
    return create_connection(url, timeout=timeout)


def _default_handle_request(frame: Dict[str, Any], *, remote_peer_id: str) -> Dict[str, Any]:
    from .ws_endpoint import handle_federation_session_request

    return handle_federation_session_request(
        frame,
        target_group_id=str(frame.get("target_group_id") or ""),
        src_group_id=str(frame.get("src_group_id") or ""),
        remote_peer_id=str(remote_peer_id or ""),
    )


def _ws_send_json(ws: Any, payload: Dict[str, Any], *, send_lock: threading.Lock | None = None) -> None:
    lock = send_lock or threading.Lock()
    with lock:
        if hasattr(ws, "send_json"):
            ws.send_json(payload)
            return
        ws.send(json.dumps(payload, ensure_ascii=False, separators=(",", ":")))


def _ws_recv_json(ws: Any) -> Dict[str, Any]:
    if hasattr(ws, "receive_json"):
        raw = ws.receive_json()
        return dict(raw) if isinstance(raw, dict) else {}
    raw = ws.recv()
    parsed = json.loads(raw)
    return dict(parsed) if isinstance(parsed, dict) else {}


async def _send_request_async(peer: ThreadFederationWsPeer, request: Dict[str, Any], timeout: float) -> Dict[str, Any]:
    return peer.send_request(request, timeout)


def _run_coro(coro: Any) -> Any:
    import asyncio

    return asyncio.run(coro)
