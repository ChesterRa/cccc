"""Outbound registration config/store (Stage 1).

A registration binds a local group to a remote target (URL + transport) plus an
opaque ``credential_ref``. The natural key is ``(normalized_url, group_id)`` —
re-registering the same pair upserts the existing record rather than creating a
duplicate.

Security invariant: no raw token/secret is ever accepted or persisted here.
Only ``credential_ref`` (a pointer into a separate secrets store) is stored.
"""

from __future__ import annotations

import re
import secrets
import urllib.parse as urlparse
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

from ...contracts.v1.federation import RegistrationRecord
from ...paths import ensure_home
from ...util.fs import atomic_write_text
from ...util.time import utc_now_iso

_REG_PREFIX = "reg_"
_DEFAULT_PORTS = {"http": 80, "https": 443}
# An access-token-shaped credential_ref means a raw secret leaked into the
# registration record. Reject it at the boundary rather than silently storing
# (or rewriting) it — the caller must pass a stable opaque reference instead.
_TOKEN_SHAPED_REF = re.compile(r"^acc_[0-9A-Za-z_-]{4,}$")
_CREDENTIAL_REF = re.compile(r"^(?:sec_[A-Za-z0-9_-]+|fsec_[A-Za-z0-9_-]+|cred-[A-Za-z0-9_-]+)$")
_JWT_SHAPED_REF = re.compile(r"^eyJ[A-Za-z0-9_-]*\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]*$")
_RAW_SECRET_PATTERNS = (
    re.compile(r"^(?:ghp|github_pat|glpat|xox[baprs]|sk|pat|bearer)[A-Za-z0-9_-]*", re.IGNORECASE),
    _JWT_SHAPED_REF,
)


def is_valid_credential_ref(credential_ref: str) -> bool:
    ref = str(credential_ref or "").strip()
    if not ref:
        return True
    return bool(_CREDENTIAL_REF.fullmatch(ref))


def _reject_raw_credential_ref(credential_ref: str) -> None:
    ref = str(credential_ref or "").strip()
    if not ref:
        return
    if is_valid_credential_ref(ref):
        return
    if _TOKEN_SHAPED_REF.match(ref) or any(pattern.match(ref) for pattern in _RAW_SECRET_PATTERNS) or len(ref) >= 24:
        raise ValueError(
            "credential_ref must be empty or a stable credential reference such as sec_* or fsec_*; "
            "raw tokens or secrets are not accepted."
        )
    raise ValueError("credential_ref must be empty or a stable credential reference such as sec_* or fsec_*")


def normalize_url(url: str) -> str:
    """Canonicalize a registration URL so the unique key is stable.

    - lowercase scheme + host
    - drop the default port for the scheme
    - strip trailing slashes from the path
    """
    raw = str(url or "").strip()
    if not raw:
        return ""
    parts = urlparse.urlsplit(raw)
    scheme = (parts.scheme or "").lower()
    host = (parts.hostname or "").lower()
    if not scheme or not host:
        return raw
    port = parts.port
    if port is not None and _DEFAULT_PORTS.get(scheme) == port:
        port = None
    netloc = f"{host}:{port}" if port is not None else host
    path = (parts.path or "").rstrip("/")
    return f"{scheme}://{netloc}{path}"


def _registrations_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "federation_registrations.yaml"


def load_registrations(home: Optional[Path] = None) -> Dict[str, Dict[str, Any]]:
    path = _registrations_path(home)
    if not path.exists():
        return {}
    try:
        raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    except Exception:
        return {}
    records = raw.get("registrations") if isinstance(raw, dict) else None
    if not isinstance(records, dict):
        return {}
    return {str(k): dict(v) for k, v in records.items() if isinstance(v, dict)}


def _save_registrations(records: Dict[str, Dict[str, Any]], home: Optional[Path] = None) -> None:
    payload = {"registrations": {str(k): dict(v) for k, v in records.items()}}
    atomic_write_text(
        _registrations_path(home),
        yaml.safe_dump(payload, allow_unicode=True, sort_keys=True, default_flow_style=False),
    )


def list_registrations(home: Optional[Path] = None) -> List[Dict[str, Any]]:
    items = list(load_registrations(home).values())
    items.sort(key=lambda item: (str(item.get("created_at") or ""), str(item.get("registration_id") or "")))
    return items


def get_registration(registration_id: str, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    rid = str(registration_id or "").strip()
    if not rid:
        return None
    entry = load_registrations(home).get(rid)
    return dict(entry) if isinstance(entry, dict) else None


def get_registration_by_target(url: str, group_id: str, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    norm = normalize_url(url)
    gid = str(group_id or "").strip()
    for entry in load_registrations(home).values():
        if str(entry.get("url") or "") == norm and str(entry.get("group_id") or "") == gid:
            return dict(entry)
    return None


def _new_registration_id(existing: Dict[str, Dict[str, Any]]) -> str:
    while True:
        candidate = f"{_REG_PREFIX}{secrets.token_hex(8)}"
        if candidate not in existing:
            return candidate


def upsert_registration(
    group_id: str,
    url: str,
    *,
    transport: str = "registry_hub",
    remote_group_id: str = "",
    remote_peer_id: str = "",
    multiaddrs: Optional[List[str]] = None,
    credential_ref: str = "",
    user_id: str = "",
    status: str = "active",
    home: Optional[Path] = None,
    _approved_by_pairing: bool = False,
) -> Dict[str, Any]:
    """Create or update the registration for ``(normalized_url, group_id)``."""
    gid = str(group_id or "").strip()
    if not gid:
        raise ValueError("group_id is required")
    norm = normalize_url(url)
    if not norm:
        raise ValueError("url is required")
    cred_ref = str(credential_ref or "").strip()
    _reject_raw_credential_ref(cred_ref)
    transport_name = str(transport or "registry_hub").strip() or "registry_hub"
    remote_gid = str(remote_group_id or "").strip()
    remote_pid = str(remote_peer_id or "").strip()
    addrs = [str(addr or "").strip() for addr in (multiaddrs or []) if str(addr or "").strip()]
    # The peer_cccc_http transport posts to /api/v1/groups/<remote_group_id>/send;
    # an empty remote group id produces a broken target URL, so require it.
    if transport_name == "peer_cccc_http" and not remote_gid:
        raise ValueError("remote_group_id is required for the peer_cccc_http transport")
    if transport_name == "federation_session":
        if str(status or "active").strip() == "active" and not _approved_by_pairing:
            raise ValueError(f"active {transport_name} registrations must be created by pairing approval")
        if not remote_gid:
            raise ValueError(f"remote_group_id is required for the {transport_name} transport")
        if not remote_pid:
            raise ValueError(f"remote_peer_id is required for the {transport_name} transport")

    records = load_registrations(home)
    now = utc_now_iso()

    existing_id = ""
    for rid, entry in records.items():
        if str(entry.get("url") or "") == norm and str(entry.get("group_id") or "") == gid:
            existing_id = rid
            break

    if existing_id:
        created_at = str(records[existing_id].get("created_at") or now)
        registration_id = existing_id
    else:
        registration_id = _new_registration_id(records)
        created_at = now

    record = RegistrationRecord(
        registration_id=registration_id,
        group_id=gid,
        url=norm,
        transport=transport_name,
        remote_group_id=remote_gid,
        remote_peer_id=remote_pid,
        multiaddrs=addrs,
        credential_ref=cred_ref,
        user_id=str(user_id or "").strip(),
        status=str(status or "active").strip() or "active",  # type: ignore[arg-type]
        created_at=created_at,
        updated_at=now,
    )
    records[registration_id] = record.model_dump()
    _save_registrations(records, home)
    return record.model_dump()


def delete_registration(registration_id: str, home: Optional[Path] = None) -> bool:
    rid = str(registration_id or "").strip()
    if not rid:
        return False
    records = load_registrations(home)
    if rid not in records:
        return False
    del records[rid]
    _save_registrations(records, home)
    return True
