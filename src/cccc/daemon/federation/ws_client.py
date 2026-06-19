"""Client side of federation WebSocket sessions."""

from __future__ import annotations

import json
import logging
import socket
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

logger = logging.getLogger("cccc.daemon.federation.ws")
_IDLE = object()


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
    connect_timeout: float | None = None,
    handshake_timeout: float | None = None,
    idle_tick_seconds: float = 30.0,
) -> Dict[str, Any]:
    connector = connect or _default_connect
    connect_timeout_value = max(0.1, float(connect_timeout if connect_timeout is not None else timeout or 5.0))
    handshake_timeout_value = max(0.1, float(handshake_timeout if handshake_timeout is not None else timeout or 5.0))
    idle_tick_value = max(0.1, float(idle_tick_seconds or 30.0))
    try:
        ws = connector(federation_session_ws_url(remote_base_url), connect_timeout_value)
    except Exception as exc:
        return {"ok": False, "error": {"code": "session_connect_failed", "message": str(exc)}}
    _set_ws_timeout(ws, handshake_timeout_value)
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
        _set_ws_timeout(ws, idle_tick_value)
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
            _set_ws_timeout(ws, min(0.05, idle_tick_value))
            initial_frame = _ws_recv_json_or_idle(ws)
            if initial_frame is not _IDLE:
                _handle_session_frame(ws, initial_frame, peer=peer, handler=handler, send_lock=send_lock)
            _set_ws_timeout(ws, idle_tick_value)
            if on_ready is not None:
                on_ready()
            while True:
                frame = _ws_recv_json_or_idle(ws)
                if frame is _IDLE:
                    _ws_send_json(ws, {"type": "ping"}, send_lock=send_lock)
                    continue
                _handle_session_frame(ws, frame, peer=peer, handler=handler, send_lock=send_lock)
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
    connect_timeout: float | None = None,
    handshake_timeout: float | None = None,
    idle_tick_seconds: float = 30.0,
    stop: threading.Event | None = None,
) -> threading.Thread:
    stop_event = stop or threading.Event()

    def run() -> None:
        while not stop_event.is_set():
            result = connect_federation_session_once(
                remote_base_url=remote_base_url,
                local_group_id=local_group_id,
                remote_group_id=remote_group_id,
                remote_peer_id=remote_peer_id,
                connect=connect,
                handle_request=handle_request,
                connect_timeout=connect_timeout,
                handshake_timeout=handshake_timeout,
                idle_tick_seconds=idle_tick_seconds,
            )
            if not stop_event.is_set():
                error = result.get("error") if isinstance(result.get("error"), dict) else {}
                logger.warning(
                    "federation session client reconnecting remote_base_url=%s local_group=%s remote_group=%s remote_peer=%s ok=%s error_code=%s error_message=%s retry_seconds=%s",
                    remote_base_url,
                    local_group_id,
                    remote_group_id,
                    remote_peer_id,
                    bool(result.get("ok")),
                    str(error.get("code") or ""),
                    str(error.get("message") or ""),
                    max(0.1, float(retry_seconds or 0.1)),
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


def _ws_recv_json_or_idle(ws: Any) -> Dict[str, Any] | object:
    try:
        return _ws_recv_json(ws)
    except Exception as exc:
        if _is_idle_timeout(exc):
            return _IDLE
        raise


def _handle_session_frame(
    ws: Any,
    frame: Dict[str, Any],
    *,
    peer: ThreadFederationWsPeer,
    handler: RequestHandler,
    send_lock: threading.Lock,
) -> None:
    frame_type = str(frame.get("type") or "").strip()
    if frame_type == "response":
        peer.receive_response(frame)
        return
    if frame_type == "pong":
        return
    if frame_type != "request":
        return
    request_id = str(frame.get("request_id") or "").strip()
    result = handler(frame)
    _ws_send_json(ws, {"type": "response", "response_to": request_id, "result": result}, send_lock=send_lock)


def _is_idle_timeout(exc: BaseException) -> bool:
    if isinstance(exc, (TimeoutError, socket.timeout)):
        return True
    name = type(exc).__name__
    if name == "WebSocketTimeoutException":
        return True
    message = str(exc).lower()
    return "timed out" in message or "timeout" in message


def _set_ws_timeout(ws: Any, timeout: float) -> None:
    value = max(0.1, float(timeout or 0.1))
    setter = getattr(ws, "settimeout", None)
    if callable(setter):
        setter(value)
        return
    sock = getattr(ws, "sock", None)
    sock_setter = getattr(sock, "settimeout", None)
    if callable(sock_setter):
        sock_setter(value)


async def _send_request_async(peer: ThreadFederationWsPeer, request: Dict[str, Any], timeout: float) -> Dict[str, Any]:
    return peer.send_request(request, timeout)


def _run_coro(coro: Any) -> Any:
    import asyncio

    return asyncio.run(coro)
