"""Local group_bridge credential store.

Registrations persist only ``credential_ref``. This module owns the private
mapping from group_bridge credential refs to raw bearer tokens used by transports.
"""

from __future__ import annotations

import hashlib
import hmac
import secrets
from pathlib import Path
from typing import Any, Dict, Optional

import yaml

from ...paths import ensure_home
from ...util.fs import atomic_write_text
from ...util.time import utc_now_iso

_PAIRING_REF_PREFIX = "fsec_pairing_"
_REMOTE_SEND_REF_PREFIX = "fsec_remote_send_"
_REMOTE_SEND_TOKEN_PREFIX = "frs_"


def _path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "group_bridge_credentials.yaml"


def _load(home: Optional[Path] = None) -> Dict[str, Dict[str, Any]]:
    path = _path(home)
    if not path.exists():
        return {}
    try:
        raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    except Exception:
        return {}
    records = raw.get("credentials") if isinstance(raw, dict) else None
    if not isinstance(records, dict):
        return {}
    return {str(k): dict(v) for k, v in records.items() if isinstance(v, dict)}


def _save(records: Dict[str, Dict[str, Any]], home: Optional[Path] = None) -> None:
    atomic_write_text(_path(home), yaml.safe_dump({"credentials": records}, allow_unicode=True, sort_keys=True))


def _new_remote_send_token(records: Dict[str, Dict[str, Any]]) -> str:
    existing = {str(record.get("token") or "").strip() for record in records.values() if isinstance(record, dict)}
    while True:
        token = _REMOTE_SEND_TOKEN_PREFIX + secrets.token_urlsafe(32)
        if token not in existing:
            return token


def save_pairing_bearer_token(
    *,
    local_group_id: str,
    remote_group_id: str,
    remote_endpoint: str,
    token: str,
    home: Optional[Path] = None,
) -> str:
    raw_token = str(token or "").strip()
    if not raw_token:
        return ""
    material = "|".join(
        str(item or "").strip()
        for item in (local_group_id, remote_group_id, remote_endpoint, hashlib.sha256(raw_token.encode("utf-8")).hexdigest())
    )
    ref = _PAIRING_REF_PREFIX + hashlib.sha256(material.encode("utf-8")).hexdigest()[:24]
    records = _load(home)
    now = utc_now_iso()
    records[ref] = {
        "credential_ref": ref,
        "kind": "bearer",
        "token": raw_token,
        "local_group_id": str(local_group_id or "").strip(),
        "remote_group_id": str(remote_group_id or "").strip(),
        "remote_endpoint": str(remote_endpoint or "").strip(),
        "created_at": str(records.get(ref, {}).get("created_at") or now),
        "updated_at": now,
    }
    _save(records, home)
    return ref


def resolve_group_bridge_credential(credential_ref: str, *, home: Optional[Path] = None) -> str:
    ref = str(credential_ref or "").strip()
    if not ref:
        return ""
    record = _load(home).get(ref)
    if not isinstance(record, dict):
        return ""
    if str(record.get("kind") or "") != "bearer":
        return ""
    return str(record.get("token") or "").strip()


def create_pairing_remote_send_credential(
    *,
    group_id: str,
    remote_group_id: str,
    remote_peer_id: str,
    request_id: str,
    home: Optional[Path] = None,
) -> Dict[str, str]:
    gid = str(group_id or "").strip()
    remote_gid = str(remote_group_id or "").strip()
    peer_id = str(remote_peer_id or "").strip()
    rid = str(request_id or "").strip()
    if not gid or not remote_gid or not peer_id or not rid:
        return {"credential_ref": "", "token": ""}
    material = "|".join(("remote_send", gid, remote_gid, peer_id, rid))
    ref = _REMOTE_SEND_REF_PREFIX + hashlib.sha256(material.encode("utf-8")).hexdigest()[:24]
    records = _load(home)
    now = utc_now_iso()
    existing = records.get(ref) if isinstance(records.get(ref), dict) else {}
    token = str(existing.get("token") or "").strip() or _new_remote_send_token(records)
    records[ref] = {
        "credential_ref": ref,
        "kind": "remote_send",
        "token": token,
        "group_id": gid,
        "remote_group_id": remote_gid,
        "remote_peer_id": peer_id,
        "request_id": rid,
        "created_at": str(existing.get("created_at") or now),
        "updated_at": now,
    }
    _save(records, home)
    return {"credential_ref": ref, "token": token}


def resolve_pairing_remote_send_token(credential_ref: str, *, home: Optional[Path] = None) -> str:
    ref = str(credential_ref or "").strip()
    if not ref:
        return ""
    record = _load(home).get(ref)
    if not isinstance(record, dict):
        return ""
    if str(record.get("kind") or "") != "remote_send":
        return ""
    return str(record.get("token") or "").strip()


def lookup_pairing_remote_send_credential(token: str, *, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    raw_token = str(token or "").strip()
    if not raw_token:
        return None
    for ref, record in _load(home).items():
        if not isinstance(record, dict):
            continue
        if str(record.get("kind") or "") != "remote_send":
            continue
        stored = str(record.get("token") or "").strip()
        if stored and hmac.compare_digest(stored, raw_token):
            return {**record, "credential_ref": str(record.get("credential_ref") or ref)}
    return None


def delete_group_bridge_credential(credential_ref: str, *, home: Optional[Path] = None) -> bool:
    ref = str(credential_ref or "").strip()
    if not ref:
        return False
    records = _load(home)
    if ref not in records:
        return False
    del records[ref]
    _save(records, home)
    return True
