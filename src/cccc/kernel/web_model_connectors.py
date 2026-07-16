from __future__ import annotations

import hashlib
import hmac
import secrets
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

from ..paths import ensure_home
from ..util.fs import atomic_write_text
from ..util.time import utc_now_iso

_CONNECTOR_PREFIX = "wmc_"
_SECRET_PREFIX = "wmcs_"


def _connectors_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "web_model_connectors.yaml"


def _hash_secret(secret: str) -> str:
    return hashlib.sha256(str(secret or "").encode("utf-8")).hexdigest()


def _preview(secret: str) -> str:
    raw = str(secret or "")
    if len(raw) <= 10:
        return "****"
    return raw[:6] + "..." + raw[-4:]


def _normalize_entry(connector_id: str, raw: Any) -> Optional[Dict[str, Any]]:
    cid = str(connector_id or "").strip()
    if not cid or not isinstance(raw, dict):
        return None
    group_id = str(raw.get("group_id") or "").strip()
    actor_id = str(raw.get("actor_id") or "").strip()
    secret = str(raw.get("secret") or raw.get("secret_value") or "").strip()
    secret_hash = str(raw.get("secret_hash") or "").strip() or (_hash_secret(secret) if secret else "")
    if not group_id or not actor_id or not secret_hash:
        return None
    created_at = str(raw.get("created_at") or "").strip() or utc_now_iso()
    updated_at = str(raw.get("updated_at") or "").strip() or created_at
    out = {
        "connector_id": cid,
        "kind": "web_model_connector",
        "group_id": group_id,
        "actor_id": actor_id,
        "provider": str(raw.get("provider") or "").strip(),
        "label": str(raw.get("label") or "").strip(),
        "secret_hash": secret_hash,
        "secret_preview": str(raw.get("secret_preview") or "").strip(),
        "revoked": bool(raw.get("revoked", False)),
        "created_at": created_at,
        "updated_at": updated_at,
        "last_activity_at": str(raw.get("last_activity_at") or "").strip(),
        "last_method": str(raw.get("last_method") or "").strip(),
        "last_tool_name": str(raw.get("last_tool_name") or "").strip(),
        "last_call_status": str(raw.get("last_call_status") or "").strip(),
        "last_wait_status": str(raw.get("last_wait_status") or "").strip(),
        "last_turn_id": str(raw.get("last_turn_id") or "").strip(),
        "last_error": str(raw.get("last_error") or "").strip(),
    }
    if secret:
        out["secret"] = secret
        if not out["secret_preview"]:
            out["secret_preview"] = _preview(secret)
    return out


def _collapse_active_connector_duplicates(connectors: Dict[str, Dict[str, Any]]) -> Dict[str, Dict[str, Any]]:
    current_by_actor: Dict[tuple[str, str], str] = {}
    for connector_id, entry in connectors.items():
        if not isinstance(entry, dict) or bool(entry.get("revoked")):
            continue
        group_id = str(entry.get("group_id") or "").strip()
        actor_id = str(entry.get("actor_id") or "").strip()
        if not group_id or not actor_id:
            continue
        key = (group_id, actor_id)
        current_id = current_by_actor.get(key)
        if not current_id:
            current_by_actor[key] = connector_id
            continue
        current = connectors.get(current_id, {})
        entry_rank = (str(entry.get("created_at") or ""), str(entry.get("updated_at") or ""), connector_id)
        current_rank = (str(current.get("created_at") or ""), str(current.get("updated_at") or ""), current_id)
        if entry_rank > current_rank:
            current_by_actor[key] = connector_id

    current_ids = set(current_by_actor.values())
    for connector_id, entry in connectors.items():
        if not isinstance(entry, dict) or bool(entry.get("revoked")):
            continue
        group_id = str(entry.get("group_id") or "").strip()
        actor_id = str(entry.get("actor_id") or "").strip()
        if group_id and actor_id and connector_id not in current_ids:
            entry["revoked"] = True
            entry["updated_at"] = str(entry.get("updated_at") or entry.get("created_at") or utc_now_iso())
    return connectors


def load_web_model_connectors(home: Optional[Path] = None) -> Dict[str, Dict[str, Any]]:
    path = _connectors_path(home)
    if not path.exists():
        return {}
    try:
        raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    except Exception:
        return {}
    if not isinstance(raw, dict):
        return {}
    connector_map = raw.get("connectors") if isinstance(raw.get("connectors"), dict) else raw
    if not isinstance(connector_map, dict):
        return {}
    out: Dict[str, Dict[str, Any]] = {}
    for connector_id, entry in connector_map.items():
        normalized = _normalize_entry(str(connector_id or ""), entry)
        if normalized is not None:
            out[normalized["connector_id"]] = normalized
    return _collapse_active_connector_duplicates(out)


def save_web_model_connectors(connectors: Dict[str, Dict[str, Any]], home: Optional[Path] = None) -> None:
    path = _connectors_path(home)
    payload: Dict[str, Any] = {"connectors": {}}
    for connector_id, entry in sorted(connectors.items(), key=lambda item: item[0]):
        normalized = _normalize_entry(connector_id, entry)
        if normalized is None:
            continue
        payload["connectors"][normalized["connector_id"]] = {
            "group_id": normalized["group_id"],
            "actor_id": normalized["actor_id"],
            "provider": normalized["provider"],
            "label": normalized["label"],
            **({"secret": str(normalized.get("secret") or "")} if str(normalized.get("secret") or "").strip() else {}),
            "secret_hash": normalized["secret_hash"],
            "secret_preview": normalized["secret_preview"],
            "revoked": bool(normalized["revoked"]),
            "created_at": normalized["created_at"],
            "updated_at": normalized["updated_at"],
            "last_activity_at": normalized["last_activity_at"],
            "last_method": normalized["last_method"],
            "last_tool_name": normalized["last_tool_name"],
            "last_call_status": normalized["last_call_status"],
            "last_wait_status": normalized["last_wait_status"],
            "last_turn_id": normalized["last_turn_id"],
            "last_error": normalized["last_error"],
        }
    atomic_write_text(
        path,
        yaml.safe_dump(payload, allow_unicode=True, sort_keys=False, default_flow_style=False),
    )


def _new_connector_id(existing: Dict[str, Dict[str, Any]]) -> str:
    while True:
        candidate = f"{_CONNECTOR_PREFIX}{secrets.token_hex(8)}"
        if candidate not in existing:
            return candidate


def _new_secret() -> str:
    return f"{_SECRET_PREFIX}{secrets.token_urlsafe(32)}"


def create_web_model_connector(
    *,
    group_id: str,
    actor_id: str,
    provider: str = "",
    label: str = "",
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid:
        raise ValueError("group_id is required")
    if not aid:
        raise ValueError("actor_id is required")
    connectors = load_web_model_connectors(home)
    connector_id = _new_connector_id(connectors)
    secret = _new_secret()
    now = utc_now_iso()
    replaced_connector_ids: List[str] = []
    for existing_id, existing in connectors.items():
        if not isinstance(existing, dict) or bool(existing.get("revoked")):
            continue
        if str(existing.get("group_id") or "").strip() != gid:
            continue
        if str(existing.get("actor_id") or "").strip() != aid:
            continue
        existing["revoked"] = True
        existing["updated_at"] = now
        connectors[existing_id] = existing
        replaced_connector_ids.append(str(existing_id or "").strip())
    entry = {
        "connector_id": connector_id,
        "kind": "web_model_connector",
        "group_id": gid,
        "actor_id": aid,
        "provider": str(provider or "").strip(),
        "label": str(label or "").strip(),
        "secret": secret,
        "secret_hash": _hash_secret(secret),
        "secret_preview": _preview(secret),
        "revoked": False,
        "created_at": now,
        "updated_at": now,
    }
    connectors[connector_id] = entry
    save_web_model_connectors(connectors, home)
    return {**entry, "secret": secret, "replaced_connector_ids": replaced_connector_ids}


def list_web_model_connectors(home: Optional[Path] = None) -> List[Dict[str, Any]]:
    items = list(load_web_model_connectors(home).values())
    items.sort(key=lambda item: (str(item.get("created_at") or ""), str(item.get("connector_id") or "")), reverse=True)
    return [mask_web_model_connector(item) for item in items]


def mask_web_model_connector(entry: Dict[str, Any]) -> Dict[str, Any]:
    out = dict(entry)
    out.pop("secret", None)
    out.pop("secret_hash", None)
    out.pop("replaced_connector_ids", None)
    return out


def lookup_web_model_connector(connector_id: str, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    cid = str(connector_id or "").strip()
    if not cid:
        return None
    return load_web_model_connectors(home).get(cid)


def verify_web_model_connector_secret(
    connector_id: str,
    secret: str,
    home: Optional[Path] = None,
) -> Optional[Dict[str, Any]]:
    entry = lookup_web_model_connector(connector_id, home)
    if not isinstance(entry, dict) or bool(entry.get("revoked")):
        return None
    expected = str(entry.get("secret_hash") or "").strip()
    actual = _hash_secret(str(secret or "").strip())
    if not expected or not hmac.compare_digest(expected, actual):
        return None
    return dict(entry)


def revoke_web_model_connector(connector_id: str, home: Optional[Path] = None) -> bool:
    cid = str(connector_id or "").strip()
    if not cid:
        return False
    connectors = load_web_model_connectors(home)
    entry = connectors.get(cid)
    if not isinstance(entry, dict):
        return False
    entry["revoked"] = True
    entry["updated_at"] = utc_now_iso()
    connectors[cid] = entry
    save_web_model_connectors(connectors, home)
    return True


def record_web_model_connector_activity(
    connector_id: str,
    *,
    method: str = "",
    tool_name: str = "",
    call_status: str = "",
    wait_status: str = "",
    turn_id: str = "",
    error: str = "",
    home: Optional[Path] = None,
) -> Optional[Dict[str, Any]]:
    cid = str(connector_id or "").strip()
    if not cid:
        return None
    connectors = load_web_model_connectors(home)
    entry = connectors.get(cid)
    if not isinstance(entry, dict) or bool(entry.get("revoked")):
        return None
    entry["last_activity_at"] = utc_now_iso()
    entry["last_method"] = str(method or "").strip()
    entry["last_tool_name"] = str(tool_name or "").strip()
    entry["last_call_status"] = str(call_status or "").strip()
    if wait_status:
        entry["last_wait_status"] = str(wait_status or "").strip()
    if turn_id:
        entry["last_turn_id"] = str(turn_id or "").strip()
    entry["last_error"] = str(error or "").strip()
    connectors[cid] = entry
    save_web_model_connectors(connectors, home)
    return mask_web_model_connector(entry)
