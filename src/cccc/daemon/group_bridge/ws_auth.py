"""Authentication helpers for Group Bridge WebSocket sessions."""

from __future__ import annotations

import base64
from pathlib import Path
from typing import Any, Dict, Optional

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

from .identity import canonical_payload_bytes, get_group_bridge_identity, peer_id_from_public_key_b64

PROTOCOL_ID = "/cccc/group_bridge/session-ws/1.0.0"


def session_hello_material(hello: Dict[str, Any]) -> bytes:
    return canonical_payload_bytes(
        {
            "protocol": PROTOCOL_ID,
            "target_group_id": str(hello.get("target_group_id") or "").strip(),
            "src_group_id": str(hello.get("src_group_id") or "").strip(),
            "remote_peer_id": str(hello.get("remote_peer_id") or "").strip(),
        }
    )


def sign_session_hello(hello: Dict[str, Any], *, home: Optional[Path] = None) -> Dict[str, Any]:
    identity = get_group_bridge_identity(home=home)
    signed = dict(hello or {})
    signed["remote_peer_id"] = identity.peer_id
    signed["public_key"] = identity.public_key_b64
    signed["signature"] = identity.sign(session_hello_material(signed))
    return signed


def authenticated_session_peer_id(hello: Dict[str, Any]) -> str:
    public_key_b64 = str((hello or {}).get("public_key") or "").strip()
    signature_b64 = str((hello or {}).get("signature") or "").strip()
    expected_peer_id = str((hello or {}).get("remote_peer_id") or "").strip()
    if not public_key_b64 or not signature_b64 or not expected_peer_id:
        return ""
    try:
        public_raw = base64.b64decode(public_key_b64.encode("ascii"), validate=True)
        signature = base64.b64decode(signature_b64.encode("ascii"), validate=True)
        peer_id = peer_id_from_public_key_b64(public_key_b64)
        if peer_id != expected_peer_id:
            return ""
        Ed25519PublicKey.from_public_bytes(public_raw).verify(signature, session_hello_material(hello))
        return peer_id
    except Exception:
        return ""
