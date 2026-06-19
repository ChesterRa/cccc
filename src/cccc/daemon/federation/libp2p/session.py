"""Bidirectional request/response session for the C1 libp2p sidecar."""

from __future__ import annotations

import secrets
import socket
import threading
import json
from typing import Any, Callable, Dict, Optional

FrameReader = Callable[[socket.socket], Dict[str, Any]]
FrameWriter = Callable[[socket.socket, Dict[str, Any]], None]
RequestHandler = Callable[[Dict[str, Any], str, str], Dict[str, Any]]
Signer = Callable[[str, Dict[str, Any]], str]
ErrorFactory = Callable[[str, str], Dict[str, Any]]


class Libp2pSession:
    def __init__(
        self,
        *,
        sock: socket.socket,
        remote_peer_id: str,
        inbound_nonce: str,
        outbound_nonce: str,
        public_key_b64: str,
        sign: Signer,
        handle_request: RequestHandler,
        unregister: Callable[["Libp2pSession"], None],
        stop_event: threading.Event,
        write_frame: FrameWriter,
        error: ErrorFactory,
        read_frame: Optional[FrameReader] = None,
        max_frame_bytes: int = 256_000,
        read_chunk: int = 8192,
    ) -> None:
        self.sock = sock
        self.remote_peer_id = str(remote_peer_id or "").strip()
        self.inbound_nonce = str(inbound_nonce or "").strip()
        self.outbound_nonce = str(outbound_nonce or "").strip()
        self._public_key_b64 = public_key_b64
        self._sign = sign
        self._handle_request = handle_request
        self._unregister = unregister
        self._stop_event = stop_event
        self._read_frame = read_frame
        self._write_frame = write_frame
        self._error = error
        self._max_frame_bytes = max(1, int(max_frame_bytes or 1))
        self._read_chunk = max(1, int(read_chunk or 1))
        self._read_buffer = bytearray()
        self._send_lock = threading.RLock()
        self._write_lock = threading.RLock()
        self._pending: dict[str, tuple[threading.Event, Dict[str, Any]]] = {}
        self._pending_lock = threading.RLock()
        self._closed = threading.Event()
        self._reader: Optional[threading.Thread] = None

    @property
    def closed(self) -> bool:
        return self._closed.is_set()

    def start_reader(self) -> None:
        if self._reader is not None:
            return
        self._reader = threading.Thread(
            target=self.read_loop,
            name=f"cccc-libp2p-session-{self.remote_peer_id[:12]}",
            daemon=True,
        )
        self._reader.start()

    def send_request(
        self,
        body: Dict[str, Any],
        *,
        timeout: float,
        extra: Optional[Dict[str, Any]] = None,
        inline_response: bool = False,
    ) -> Dict[str, Any]:
        if self.closed:
            return self._error("libp2p_session_closed", "direct libp2p session is closed")
        with self._send_lock:
            if self._reader is threading.current_thread():
                inline_response = True
            req_id = secrets.token_urlsafe(12)
            ready = threading.Event()
            holder: Dict[str, Any] = {}
            with self._pending_lock:
                self._pending[req_id] = (ready, holder)
            try:
                auth_body = {
                    **body,
                    **(extra or {}),
                    "request_id": req_id,
                    "public_key": self._public_key_b64,
                    "signature": self._sign(self.outbound_nonce, body),
                }
                with self._write_lock:
                    self._write_frame(self.sock, auth_body)
                if inline_response:
                    return self._read_inline_response(req_id, timeout=timeout, op=str(body.get("op") or ""))
                if not ready.wait(timeout):
                    self.close()
                    return self._error("libp2p_response_timeout", "direct libp2p session timed out")
                return dict(holder.get("response") or {})
            except Exception:
                self.close()
                return self._error("libp2p_dial_failed", "direct libp2p dial failed")
            finally:
                with self._pending_lock:
                    self._pending.pop(req_id, None)

    def write_response(self, req: Dict[str, Any], result: Dict[str, Any]) -> None:
        req_id = str(req.get("request_id") or "").strip()
        response = dict(result or {})
        if req_id:
            response["response_to"] = req_id
        with self._write_lock:
            self._write_frame(self.sock, response)

    def read_loop(self) -> None:
        if self._reader is None:
            self._reader = threading.current_thread()
        try:
            while not self.closed and not self._stop_event.is_set():
                frame = self._read_next_frame()
                if not frame:
                    break
                self._handle_frame(frame)
        except Exception:
            pass
        finally:
            self.close()

    def close(self) -> None:
        if self._closed.is_set():
            return
        self._closed.set()
        try:
            self.sock.close()
        except Exception:
            pass
        with self._pending_lock:
            pending = list(self._pending.values())
            self._pending.clear()
        for ready, holder in pending:
            holder["response"] = self._error("libp2p_session_closed", "direct libp2p session is closed")
            ready.set()
        self._unregister(self)

    def _read_inline_response(self, req_id: str, *, timeout: float, op: str) -> Dict[str, Any]:
        self.sock.settimeout(timeout)
        try:
            while not self.closed:
                frame = self._read_next_frame()
                if not frame:
                    break
                response_to = str(frame.get("response_to") or "").strip()
                if not response_to or response_to == req_id:
                    if not response_to and _looks_like_request(frame) and not _looks_like_response_for_op(frame, op):
                        self._handle_frame(frame)
                        continue
                    return frame
                self._handle_frame(frame)
        finally:
            self.sock.settimeout(None)
        self.close()
        return self._error("libp2p_session_closed", "direct libp2p session is closed")

    def _read_next_frame(self) -> Dict[str, Any]:
        if self._read_frame is not None:
            return self._read_frame(self.sock)
        while b"\n" not in self._read_buffer:
            chunk = self.sock.recv(self._read_chunk)
            if not chunk:
                if not self._read_buffer:
                    return {}
                break
            self._read_buffer.extend(chunk)
            if len(self._read_buffer) > self._max_frame_bytes:
                raise ValueError("frame too large")
        if b"\n" in self._read_buffer:
            line, rest = bytes(self._read_buffer).split(b"\n", 1)
            self._read_buffer = bytearray(rest)
        else:
            line = bytes(self._read_buffer)
            self._read_buffer.clear()
        parsed = json.loads(line.decode("utf-8"))
        return dict(parsed) if isinstance(parsed, dict) else {}

    def _handle_frame(self, frame: Dict[str, Any]) -> None:
        response_to = str(frame.get("response_to") or "").strip()
        if response_to:
            self._resolve_response(response_to, frame)
            return
        result = self._handle_request(frame, self.inbound_nonce, self.remote_peer_id)
        self.write_response(frame, result)

    def _resolve_response(self, req_id: str, response: Dict[str, Any]) -> None:
        with self._pending_lock:
            pending = self._pending.get(req_id)
        if pending is None:
            return
        ready, holder = pending
        holder["response"] = dict(response)
        ready.set()


def _looks_like_request(frame: Dict[str, Any]) -> bool:
    return bool(str(frame.get("request_id") or "").strip()) and (
        bool(str(frame.get("protocol") or "").strip())
        or bool(str(frame.get("op") or "").strip())
        or bool(str(frame.get("signature") or "").strip())
    )


def _looks_like_response_for_op(frame: Dict[str, Any], op: str) -> bool:
    operation = str(op or "").strip()
    if operation == "address_announce":
        return any(key in frame for key in ("updated", "trust_updates", "registration_updates", "retried"))
    if operation == "remote_send":
        return any(key in frame for key in ("event_id", "duplicate"))
    return bool("ok" in frame and "error" in frame)
