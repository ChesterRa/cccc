"""Small direct stream sidecar.

This is intentionally narrow: TCP, known IPv4 multiaddr dialing, one CCCC
remote-send stream protocol. It is the daemon/client boundary that a future
full libp2p runtime can replace without changing remote_send dispatch.
"""

from __future__ import annotations

import json
import secrets
import socket
import threading
import ipaddress
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from .identity import Libp2pIdentity, canonical_payload_bytes, get_libp2p_identity, peer_id_from_public_key_b64
from .receiver import defer_pending_retry_for_peer, receive_address_announce, receive_remote_send
from .session import Libp2pSession

PROTOCOL_ID = "/cccc/federation/remote-send/1.0.0"
OP_ADDRESS_ANNOUNCE = "address_announce"
OP_REMOTE_SEND = "remote_send"
_MAX_FRAME_BYTES = 256_000
_READ_CHUNK = 8192
_DEFAULT_TIMEOUT = 3.0


@dataclass(frozen=True)
class ParsedMultiaddr:
    host: str
    port: int
    peer_id: str


def parse_direct_multiaddr(value: str) -> ParsedMultiaddr:
    raw = str(value or "").strip()
    parts = [part for part in raw.split("/") if part]
    if len(parts) != 6 or parts[0] != "ip4" or parts[2] != "tcp" or parts[4] != "p2p":
        raise ValueError("only /ip4/<host>/tcp/<port>/p2p/<peer_id> multiaddrs are supported in C1")
    host = parts[1].strip()
    try:
        ip = ipaddress.IPv4Address(host)
    except Exception as exc:
        raise ValueError("dial multiaddr host must be a concrete IPv4 address") from exc
    if ip.is_unspecified or ip.is_multicast or ip.is_reserved:
        raise ValueError("dial multiaddr host must be a concrete IPv4 address")
    try:
        port = int(parts[3])
    except Exception as exc:
        raise ValueError("invalid multiaddr tcp port") from exc
    if port <= 0 or port > 65535:
        raise ValueError("invalid multiaddr tcp port")
    peer_id = parts[5].strip()
    if not peer_id:
        raise ValueError("multiaddr peer id is required")
    return ParsedMultiaddr(host=str(ip), port=port, peer_id=peer_id)


class Libp2pNode:
    def __init__(
        self,
        *,
        home: Optional[Path] = None,
        listen_multiaddr: str = "/ip4/127.0.0.1/tcp/0",
        advertise_host: str = "",
    ) -> None:
        self.home = Path(home) if home is not None else None
        self.identity = get_libp2p_identity(home=self.home)
        self._listen = listen_multiaddr
        self._advertise_host = advertise_host
        self._sock: Optional[socket.socket] = None
        self._thread: Optional[threading.Thread] = None
        self._stop = threading.Event()
        self._bound: Optional[Tuple[str, int]] = None
        self._sessions: dict[str, Libp2pSession] = {}
        self._sessions_lock = threading.RLock()

    def start(self) -> None:
        if self._sock is not None:
            return
        host, port = _parse_listen_addr(self._listen)
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind((host, port))
        sock.listen(16)
        sock.settimeout(0.2)
        self._sock = sock
        self._bound = sock.getsockname()[:2]
        self._thread = threading.Thread(target=self._serve, name=f"cccc-libp2p-{self.identity.peer_id[:12]}", daemon=True)
        self._thread.start()
        try:
            from .client import register_node

            register_node(self)
        except Exception:
            pass

    def stop(self) -> None:
        self._stop.set()
        sock = self._sock
        self._sock = None
        if sock is not None:
            try:
                sock.close()
            except Exception:
                pass
        thread = self._thread
        self._thread = None
        if thread is not None:
            thread.join(timeout=1.0)
        with self._sessions_lock:
            sessions = list(self._sessions.values())
            self._sessions.clear()
        for session in sessions:
            session.close()
        try:
            from .client import unregister_node

            unregister_node(self)
        except Exception:
            pass

    def multiaddrs(self) -> list[str]:
        if self._bound is None:
            return []
        host, port = self._bound
        advertised_host = _advertise_host_for_bound(host, self._advertise_host)
        return [f"/ip4/{advertised_host}/tcp/{port}/p2p/{self.identity.peer_id}"]

    def update_advertise_host(self, advertise_host: str) -> bool:
        next_host = str(advertise_host or "").strip()
        if next_host == str(self._advertise_host or "").strip():
            return False
        _advertise_host_for_bound("0.0.0.0", next_host)
        self._advertise_host = next_host
        return True

    def send_remote(self, *, multiaddr: str, request: Dict[str, Any], timeout: float = _DEFAULT_TIMEOUT) -> Dict[str, Any]:
        return self._send_authenticated(multiaddr=multiaddr, body=_remote_send_body(request), timeout=timeout)

    def announce_addresses(self, *, multiaddr: str, request: Dict[str, Any], timeout: float = _DEFAULT_TIMEOUT) -> Dict[str, Any]:
        return self._send_authenticated(multiaddr=multiaddr, body=_address_announce_body(request), timeout=timeout)

    def _send_authenticated(self, *, multiaddr: str, body: Dict[str, Any], timeout: float) -> Dict[str, Any]:
        parsed = parse_direct_multiaddr(multiaddr)
        if str(body.get("remote_peer_id") or "").strip() and str(body.get("remote_peer_id") or "").strip() != parsed.peer_id:
            return _error("peer_mismatch", "multiaddr peer id does not match registration remote_peer_id")
        session = self._session_for_peer(parsed.peer_id)
        if session is not None:
            result = session.send_request(body, timeout=timeout)
            if not _is_session_transport_error(result):
                return result
        try:
            sock = socket.create_connection((parsed.host, parsed.port), timeout=timeout)
            sock.settimeout(timeout)
            challenge = _read_frame(sock)
            nonce = str(challenge.get("nonce") or "").strip()
            if str(challenge.get("protocol") or "") != PROTOCOL_ID or not nonce:
                sock.close()
                return _error("libp2p_auth_failed", "remote did not provide a valid challenge")
            client_nonce = secrets.token_urlsafe(24)
            sock.settimeout(None)
            session = Libp2pSession(
                sock=sock,
                remote_peer_id=parsed.peer_id,
                inbound_nonce=client_nonce,
                outbound_nonce=nonce,
                public_key_b64=self.identity.public_key_b64,
                sign=self._sign_session_request,
                handle_request=self._handle_session_request,
                unregister=self._unregister_session,
                stop_event=self._stop,
                write_frame=_write_frame,
                error=_error,
            )
            self._register_session(session)
            result = session.send_request(body, timeout=timeout, extra={"session_nonce": client_nonce}, inline_response=True)
            if result.get("ok") is False and str(result.get("error", {}).get("code") or "") in {
                "libp2p_auth_failed",
                "unauthorized_peer",
                "unsupported_protocol",
            }:
                session.close()
            elif not session.closed:
                session.start_reader()
            return result
        except Exception:
            return _error("libp2p_dial_failed", "direct libp2p dial failed")

    def _session_for_peer(self, peer_id: str) -> Optional[Libp2pSession]:
        pid = str(peer_id or "").strip()
        if not pid:
            return None
        with self._sessions_lock:
            session = self._sessions.get(pid)
            if session is None or session.closed:
                self._sessions.pop(pid, None)
                return None
            return session

    def _register_session(self, session: Libp2pSession) -> None:
        with self._sessions_lock:
            old = self._sessions.get(session.remote_peer_id)
            self._sessions[session.remote_peer_id] = session
        if old is not None and old is not session:
            old.close()

    def _unregister_session(self, session: Libp2pSession) -> None:
        with self._sessions_lock:
            if self._sessions.get(session.remote_peer_id) is session:
                self._sessions.pop(session.remote_peer_id, None)

    def _serve(self) -> None:
        sock = self._sock
        if sock is None:
            return
        while not self._stop.is_set():
            try:
                conn, _addr = sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            threading.Thread(target=self._handle_conn, args=(conn,), daemon=True).start()

    def _handle_conn(self, conn: socket.socket) -> None:
        conn.settimeout(_DEFAULT_TIMEOUT)
        session: Optional[Libp2pSession] = None
        try:
            nonce = secrets.token_urlsafe(24)
            _write_frame(conn, {"protocol": PROTOCOL_ID, "nonce": nonce})
            req = _read_frame(conn)
            if str(req.get("protocol") or "") != PROTOCOL_ID:
                _write_frame(conn, _error("unsupported_protocol", "unsupported libp2p protocol"))
                conn.close()
                return
            auth = _authenticated_peer_id(nonce=nonce, req=req)
            if auth is None:
                _write_frame(conn, _error("unauthorized_peer", "remote peer authentication failed"))
                conn.close()
                return
            client_nonce = str(req.get("session_nonce") or "").strip()
            if client_nonce:
                conn.settimeout(None)
                session = Libp2pSession(
                    sock=conn,
                    remote_peer_id=auth,
                    inbound_nonce=nonce,
                    outbound_nonce=client_nonce,
                    public_key_b64=self.identity.public_key_b64,
                    sign=self._sign_session_request,
                    handle_request=self._handle_session_request,
                    unregister=self._unregister_session,
                    stop_event=self._stop,
                    write_frame=_write_frame,
                    error=_error,
                )
                self._register_session(session)
                result = self._handle_authenticated_request(req=req, auth=auth, defer_address_retry=True)
                session.write_response(req, result)
                self._defer_address_retry(req=req, auth=auth, result=result)
                session.read_loop()
            else:
                result = self._handle_authenticated_request(req=req, auth=auth, defer_address_retry=False)
                _write_frame(conn, result)
                conn.close()
        except Exception:
            try:
                _write_frame(conn, _error("remote_stream_error", "remote libp2p stream failed"))
            except Exception:
                pass
            if session is None:
                try:
                    conn.close()
                except Exception:
                    pass

    def _handle_session_request(self, req: Dict[str, Any], nonce: str, expected_peer_id: str) -> Dict[str, Any]:
        if str(req.get("protocol") or "") != PROTOCOL_ID:
            return _error("unsupported_protocol", "unsupported libp2p protocol")
        auth = _authenticated_peer_id(nonce=nonce, req=req)
        if auth is None or auth != expected_peer_id:
            return _error("unauthorized_peer", "remote peer authentication failed")
        return self._handle_authenticated_request(req=req, auth=auth, defer_address_retry=False)

    def _sign_session_request(self, nonce: str, body: Dict[str, Any]) -> str:
        return self.identity.sign(_auth_material(nonce=nonce, body=body))

    def _handle_authenticated_request(self, *, req: Dict[str, Any], auth: str, defer_address_retry: bool) -> Dict[str, Any]:
        op = str(req.get("op") or OP_REMOTE_SEND).strip() or OP_REMOTE_SEND
        if op == OP_ADDRESS_ANNOUNCE:
            return receive_address_announce(
                target_group_id=str(req.get("target_group_id") or ""),
                src_group_id=str(req.get("src_group_id") or ""),
                remote_peer_id=auth,
                multiaddrs=list(req.get("multiaddrs") or []) if isinstance(req.get("multiaddrs"), list) else [],
                home=self.home,
                retry_pending=not defer_address_retry,
            )
        return receive_remote_send(
            target_group_id=str(req.get("target_group_id") or ""),
            src_group_id=str(req.get("src_group_id") or ""),
            remote_peer_id=auth,
            payload=dict(req.get("payload") or {}) if isinstance(req.get("payload"), dict) else {},
            idempotency_key=str(req.get("idempotency_key") or ""),
            home=self.home,
        )

    def _defer_address_retry(self, *, req: Dict[str, Any], auth: str, result: Dict[str, Any]) -> None:
        if str(req.get("op") or OP_REMOTE_SEND).strip() != OP_ADDRESS_ANNOUNCE:
            return
        if not result.get("ok"):
            return
        defer_pending_retry_for_peer(
            target_group_id=str(req.get("target_group_id") or ""),
            src_group_id=str(req.get("src_group_id") or ""),
            remote_peer_id=auth,
            home=self.home,
        )


def _parse_listen_addr(value: str) -> tuple[str, int]:
    raw = str(value or "").strip()
    parts = [part for part in raw.split("/") if part]
    if len(parts) != 4 or parts[0] != "ip4" or parts[2] != "tcp":
        raise ValueError("listen multiaddr must be /ip4/<host>/tcp/<port>")
    try:
        host_ip = ipaddress.IPv4Address(parts[1].strip())
    except Exception as exc:
        raise ValueError("listen multiaddr host must be an IPv4 address") from exc
    if host_ip.is_multicast or host_ip.is_reserved:
        raise ValueError("listen multiaddr host must be a bindable IPv4 address")
    try:
        port = int(parts[3])
    except Exception as exc:
        raise ValueError("invalid listen tcp port") from exc
    if port < 0 or port > 65535:
        raise ValueError("invalid listen tcp port")
    return (str(host_ip), port)


def _advertise_host_for_bound(bound_host: str, advertise_host: str) -> str:
    raw = str(advertise_host or "").strip()
    if raw:
        try:
            advertised_ip = ipaddress.IPv4Address(raw)
        except Exception as exc:
            raise ValueError("advertise host must be an IPv4 address") from exc
        if advertised_ip.is_unspecified or advertised_ip.is_multicast or advertised_ip.is_reserved:
            raise ValueError("advertise host must be a concrete IPv4 address")
        return str(advertised_ip)
    bound_ip = ipaddress.IPv4Address(str(bound_host or "").strip())
    if bound_ip.is_unspecified:
        return "127.0.0.1"
    return str(bound_ip)


def _write_frame(sock: socket.socket, payload: Dict[str, Any]) -> None:
    raw = (json.dumps(payload, ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8")
    if len(raw) > _MAX_FRAME_BYTES:
        raise ValueError("frame too large")
    sock.sendall(raw)


def _read_frame(sock: socket.socket) -> Dict[str, Any]:
    buf = bytearray()
    while True:
        chunk = sock.recv(_READ_CHUNK)
        if not chunk:
            break
        buf.extend(chunk)
        if len(buf) > _MAX_FRAME_BYTES:
            raise ValueError("frame too large")
        if b"\n" in chunk:
            break
    line = bytes(buf).split(b"\n", 1)[0]
    parsed = json.loads(line.decode("utf-8"))
    return dict(parsed) if isinstance(parsed, dict) else {}


def _error(code: str, message: str) -> Dict[str, Any]:
    return {"ok": False, "error": {"code": code, "message": message}}


def _is_session_transport_error(result: Dict[str, Any]) -> bool:
    if not isinstance(result, dict):
        return True
    err = result.get("error") if isinstance(result.get("error"), dict) else {}
    return str(err.get("code") or "") in {
        "libp2p_session_closed",
        "libp2p_response_timeout",
        "libp2p_dial_failed",
    }


def _remote_send_body(request: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "protocol": PROTOCOL_ID,
        "op": OP_REMOTE_SEND,
        "src_group_id": str(request.get("src_group_id") or "").strip(),
        "target_group_id": str(request.get("remote_group_id") or "").strip(),
        "remote_peer_id": str(request.get("remote_peer_id") or "").strip(),
        "idempotency_key": str(request.get("idempotency_key") or "").strip(),
        "payload": dict(request.get("payload") or {}),
    }


def _address_announce_body(request: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "protocol": PROTOCOL_ID,
        "op": OP_ADDRESS_ANNOUNCE,
        "src_group_id": str(request.get("src_group_id") or "").strip(),
        "target_group_id": str(request.get("remote_group_id") or "").strip(),
        "remote_peer_id": str(request.get("remote_peer_id") or "").strip(),
        "multiaddrs": [str(addr or "").strip() for addr in (request.get("multiaddrs") or []) if str(addr or "").strip()],
    }


def _auth_material(*, nonce: str, body: Dict[str, Any]) -> bytes:
    payload = body.get("payload") if isinstance(body.get("payload"), dict) else {}
    material = {
        "nonce": str(nonce or ""),
        "protocol": PROTOCOL_ID,
        "op": str(body.get("op") or OP_REMOTE_SEND),
        "src_group_id": str(body.get("src_group_id") or ""),
        "target_group_id": str(body.get("target_group_id") or ""),
        "idempotency_key": str(body.get("idempotency_key") or ""),
        "multiaddrs": list(body.get("multiaddrs") or []) if isinstance(body.get("multiaddrs"), list) else [],
        "payload": payload,
    }
    return canonical_payload_bytes(material)


def _legacy_auth_material(*, nonce: str, body: Dict[str, Any]) -> bytes:
    payload = body.get("payload") if isinstance(body.get("payload"), dict) else {}
    material = {
        "nonce": str(nonce or ""),
        "protocol": PROTOCOL_ID,
        "src_group_id": str(body.get("src_group_id") or ""),
        "target_group_id": str(body.get("target_group_id") or ""),
        "idempotency_key": str(body.get("idempotency_key") or ""),
        "payload": payload,
    }
    return canonical_payload_bytes(material)


def _authenticated_peer_id(*, nonce: str, req: Dict[str, Any]) -> Optional[str]:
    public_key_b64 = str(req.get("public_key") or "").strip()
    signature_b64 = str(req.get("signature") or "").strip()
    if not public_key_b64 or not signature_b64:
        return None
    try:
        import base64

        public_raw = base64.b64decode(public_key_b64.encode("ascii"), validate=True)
        signature = base64.b64decode(signature_b64.encode("ascii"), validate=True)
        public_key = Ed25519PublicKey.from_public_bytes(public_raw)
        try:
            public_key.verify(signature, _auth_material(nonce=nonce, body=req))
        except Exception:
            public_key.verify(signature, _legacy_auth_material(nonce=nonce, body=req))
        return peer_id_from_public_key_b64(public_key_b64)
    except Exception:
        return None
