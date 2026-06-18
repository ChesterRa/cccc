"""Small Stage C1 direct stream sidecar.

This is intentionally narrow: localhost TCP, known multiaddr dialing, one CCCC
remote-send stream protocol. It is the daemon/client boundary that a future
full libp2p runtime can replace without changing remote_send dispatch.
"""

from __future__ import annotations

import json
import secrets
import socket
import threading
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from .identity import Libp2pIdentity, canonical_payload_bytes, get_libp2p_identity, peer_id_from_public_key_b64
from .receiver import receive_address_announce, receive_remote_send

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
    if host not in {"127.0.0.1", "localhost"}:
        raise ValueError("Stage C1 direct libp2p only supports localhost multiaddrs")
    try:
        port = int(parts[3])
    except Exception as exc:
        raise ValueError("invalid multiaddr tcp port") from exc
    if port <= 0 or port > 65535:
        raise ValueError("invalid multiaddr tcp port")
    peer_id = parts[5].strip()
    if not peer_id:
        raise ValueError("multiaddr peer id is required")
    return ParsedMultiaddr(host="127.0.0.1", port=port, peer_id=peer_id)


class Libp2pNode:
    def __init__(self, *, home: Optional[Path] = None, listen_multiaddr: str = "/ip4/127.0.0.1/tcp/0") -> None:
        self.home = Path(home) if home is not None else None
        self.identity = get_libp2p_identity(home=self.home)
        self._listen = listen_multiaddr
        self._sock: Optional[socket.socket] = None
        self._thread: Optional[threading.Thread] = None
        self._stop = threading.Event()
        self._bound: Optional[Tuple[str, int]] = None

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
        try:
            from .client import unregister_node

            unregister_node(self)
        except Exception:
            pass

    def multiaddrs(self) -> list[str]:
        if self._bound is None:
            return []
        host, port = self._bound
        return [f"/ip4/{host}/tcp/{port}/p2p/{self.identity.peer_id}"]

    def send_remote(self, *, multiaddr: str, request: Dict[str, Any], timeout: float = _DEFAULT_TIMEOUT) -> Dict[str, Any]:
        return self._send_authenticated(multiaddr=multiaddr, body=_remote_send_body(request), timeout=timeout)

    def announce_addresses(self, *, multiaddr: str, request: Dict[str, Any], timeout: float = _DEFAULT_TIMEOUT) -> Dict[str, Any]:
        return self._send_authenticated(multiaddr=multiaddr, body=_address_announce_body(request), timeout=timeout)

    def _send_authenticated(self, *, multiaddr: str, body: Dict[str, Any], timeout: float) -> Dict[str, Any]:
        parsed = parse_direct_multiaddr(multiaddr)
        if str(body.get("remote_peer_id") or "").strip() and str(body.get("remote_peer_id") or "").strip() != parsed.peer_id:
            return _error("peer_mismatch", "multiaddr peer id does not match registration remote_peer_id")
        try:
            with socket.create_connection((parsed.host, parsed.port), timeout=timeout) as sock:
                sock.settimeout(timeout)
                challenge = _read_frame(sock)
                nonce = str(challenge.get("nonce") or "").strip()
                if str(challenge.get("protocol") or "") != PROTOCOL_ID or not nonce:
                    return _error("libp2p_auth_failed", "remote did not provide a valid challenge")
                auth_body = {
                    **body,
                    "public_key": self.identity.public_key_b64,
                    "signature": self.identity.sign(_auth_material(nonce=nonce, body=body)),
                }
                _write_frame(sock, auth_body)
                return _read_frame(sock)
        except Exception:
            return _error("libp2p_dial_failed", "direct libp2p dial failed")

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
        with conn:
            conn.settimeout(_DEFAULT_TIMEOUT)
            try:
                nonce = secrets.token_urlsafe(24)
                _write_frame(conn, {"protocol": PROTOCOL_ID, "nonce": nonce})
                req = _read_frame(conn)
                if str(req.get("protocol") or "") != PROTOCOL_ID:
                    _write_frame(conn, _error("unsupported_protocol", "unsupported libp2p protocol"))
                    return
                auth = _authenticated_peer_id(nonce=nonce, req=req)
                if auth is None:
                    _write_frame(conn, _error("unauthorized_peer", "remote peer authentication failed"))
                    return
                op = str(req.get("op") or OP_REMOTE_SEND).strip() or OP_REMOTE_SEND
                if op == OP_ADDRESS_ANNOUNCE:
                    result = receive_address_announce(
                        target_group_id=str(req.get("target_group_id") or ""),
                        src_group_id=str(req.get("src_group_id") or ""),
                        remote_peer_id=auth,
                        multiaddrs=list(req.get("multiaddrs") or []) if isinstance(req.get("multiaddrs"), list) else [],
                        home=self.home,
                    )
                else:
                    result = receive_remote_send(
                        target_group_id=str(req.get("target_group_id") or ""),
                        src_group_id=str(req.get("src_group_id") or ""),
                        remote_peer_id=auth,
                        payload=dict(req.get("payload") or {}) if isinstance(req.get("payload"), dict) else {},
                        idempotency_key=str(req.get("idempotency_key") or ""),
                        home=self.home,
                    )
                _write_frame(conn, result)
            except Exception:
                try:
                    _write_frame(conn, _error("remote_stream_error", "remote libp2p stream failed"))
                except Exception:
                    pass


def _parse_listen_addr(value: str) -> tuple[str, int]:
    raw = str(value or "").strip()
    parts = [part for part in raw.split("/") if part]
    if len(parts) != 4 or parts[0] != "ip4" or parts[2] != "tcp":
        raise ValueError("listen multiaddr must be /ip4/127.0.0.1/tcp/<port>")
    if parts[1] not in {"127.0.0.1", "localhost"}:
        raise ValueError("Stage C1 only supports localhost listen multiaddrs")
    try:
        port = int(parts[3])
    except Exception as exc:
        raise ValueError("invalid listen tcp port") from exc
    return ("127.0.0.1", port)


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
