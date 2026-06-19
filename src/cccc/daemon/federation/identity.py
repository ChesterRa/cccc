"""Stable federation signing identity."""

from __future__ import annotations

import base64
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional

import yaml
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, NoEncryption, PrivateFormat, PublicFormat

from ...paths import ensure_home
from ...util.fs import atomic_write_text

_BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


@dataclass(frozen=True)
class FederationIdentity:
    peer_id: str
    public_key_b64: str
    private_key_b64: str

    def public_dict(self) -> Dict[str, str]:
        return {"peer_id": self.peer_id, "public_key": self.public_key_b64}

    def sign(self, payload: bytes) -> str:
        raw = base64.b64decode(self.private_key_b64.encode("ascii"), validate=True)
        key = Ed25519PrivateKey.from_private_bytes(raw)
        return base64.b64encode(key.sign(payload)).decode("ascii")


def _identity_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "federation_identity_key.yaml"


def _load_yaml(path: Path) -> Dict[str, Any]:
    if not path.exists():
        return {}
    try:
        raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    except Exception:
        return {}
    return dict(raw) if isinstance(raw, dict) else {}


def _save_yaml(path: Path, payload: Dict[str, Any]) -> None:
    atomic_write_text(path, yaml.safe_dump(payload, allow_unicode=True, sort_keys=True, default_flow_style=False))


def _b58encode(raw: bytes) -> str:
    value = int.from_bytes(raw, "big")
    encoded = ""
    while value:
        value, rem = divmod(value, 58)
        encoded = _BASE58_ALPHABET[rem] + encoded
    pad = 0
    for byte in raw:
        if byte == 0:
            pad += 1
            continue
        break
    return (_BASE58_ALPHABET[0] * pad) + (encoded or _BASE58_ALPHABET[0])


def _peer_id_for_public_key(public_key: bytes) -> str:
    protobuf = bytes([0x08, 0x01, 0x12, len(public_key)]) + public_key
    return _b58encode(bytes([0x00, len(protobuf)]) + protobuf)


def peer_id_from_public_key_b64(public_key_b64: str) -> str:
    public_raw = base64.b64decode(str(public_key_b64 or "").encode("ascii"), validate=True)
    return _peer_id_for_public_key(public_raw)


def canonical_payload_bytes(payload: Dict[str, Any]) -> bytes:
    return json.dumps(payload, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode("utf-8")


def _new_private_key_b64() -> str:
    key = Ed25519PrivateKey.generate()
    raw = key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption())
    return base64.b64encode(raw).decode("ascii")


def get_federation_identity(*, home: Optional[Path] = None) -> FederationIdentity:
    path = _identity_path(home)
    raw = _load_yaml(path)
    private_b64 = str(raw.get("private_key") or "").strip()
    try:
        private_raw = base64.b64decode(private_b64.encode("ascii"), validate=True)
        key = Ed25519PrivateKey.from_private_bytes(private_raw)
    except Exception:
        private_b64 = _new_private_key_b64()
        private_raw = base64.b64decode(private_b64.encode("ascii"))
        key = Ed25519PrivateKey.from_private_bytes(private_raw)

    public_raw = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    public_b64 = base64.b64encode(public_raw).decode("ascii")
    peer_id = _peer_id_for_public_key(public_raw)
    if raw.get("private_key") != private_b64 or raw.get("peer_id") != peer_id or not path.exists():
        _save_yaml(path, {"private_key": private_b64, "public_key": public_b64, "peer_id": peer_id})
    return FederationIdentity(peer_id=peer_id, public_key_b64=public_b64, private_key_b64=private_b64)
