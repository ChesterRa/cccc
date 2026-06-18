"""Local federation credential store.

Registrations persist only ``credential_ref``. This module owns the private
mapping from federation credential refs to raw bearer tokens used by transports.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any, Dict, Optional

import yaml

from ...paths import ensure_home
from ...util.fs import atomic_write_text
from ...util.time import utc_now_iso

_PAIRING_REF_PREFIX = "fsec_pairing_"


def _path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "federation_credentials.yaml"


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


def resolve_federation_credential(credential_ref: str, *, home: Optional[Path] = None) -> str:
    ref = str(credential_ref or "").strip()
    if not ref:
        return ""
    record = _load(home).get(ref)
    if not isinstance(record, dict):
        return ""
    if str(record.get("kind") or "") != "bearer":
        return ""
    return str(record.get("token") or "").strip()
