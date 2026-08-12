from __future__ import annotations

import hashlib
import hmac
import secrets
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, TypeVar

import yaml

from ..paths import ensure_home
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.fs import atomic_write_text
from ..util.time import utc_now_iso

_CONNECTOR_PREFIX = "wmc_"
_SECRET_PREFIX = "wmcs_"
_SETTINGS_STORE_KEY = "web_model_connectors"
_T = TypeVar("_T")


def _connectors_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "web_model_connectors.yaml"


def _connectors_lock_path(home: Optional[Path] = None) -> Path:
    return _connectors_path(home).with_suffix(".yaml.lock")


def _settings_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "settings.yaml"


def _settings_lock_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "settings.yaml.lock"


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
        entry_rank = (
            str(entry.get("created_at") or ""),
            str(entry.get("updated_at") or ""),
            str(entry.get("last_activity_at") or ""),
            connector_id,
        )
        current_rank = (
            str(current.get("created_at") or ""),
            str(current.get("updated_at") or ""),
            str(current.get("last_activity_at") or ""),
            current_id,
        )
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


def _normalized_connector_map(raw: Any) -> Dict[str, Dict[str, Any]]:
    if isinstance(raw, dict):
        connector_map: Any = raw.get("connectors") if isinstance(raw.get("connectors"), dict) else raw
        items = connector_map.items() if isinstance(connector_map, dict) else ()
    elif isinstance(raw, list):
        items = (
            (str(entry.get("connector_id") or ""), entry)
            for entry in raw
            if isinstance(entry, dict)
        )
    else:
        items = ()
    out: Dict[str, Dict[str, Any]] = {}
    for connector_id, entry in items:
        normalized = _normalize_entry(str(connector_id or ""), entry)
        if normalized is not None:
            out[normalized["connector_id"]] = normalized
    return _collapse_active_connector_duplicates(out)


def _read_connectors_unlocked(home: Optional[Path] = None) -> Dict[str, Dict[str, Any]]:
    path = _connectors_path(home)
    if not path.exists():
        return {}
    try:
        raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    except Exception as exc:
        raise ValueError("web model connector store is invalid") from exc
    return _normalized_connector_map(raw)


def _connector_payload(connectors: Dict[str, Dict[str, Any]]) -> Dict[str, Any]:
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
    return payload


def _write_connectors_unlocked(
    connectors: Dict[str, Dict[str, Any]], home: Optional[Path] = None
) -> None:
    path = _connectors_path(home)
    atomic_write_text(
        path,
        yaml.safe_dump(
            _connector_payload(connectors),
            allow_unicode=True,
            sort_keys=False,
            default_flow_style=False,
        ),
    )


def _entry_rank(entry: Dict[str, Any], connector_id: str) -> tuple[str, str, str, str]:
    return (
        str(entry.get("created_at") or ""),
        str(entry.get("updated_at") or ""),
        str(entry.get("last_activity_at") or ""),
        connector_id,
    )


def _merge_connector_maps(
    canonical: Dict[str, Dict[str, Any]], imported: Dict[str, Dict[str, Any]]
) -> Dict[str, Dict[str, Any]]:
    merged = {connector_id: dict(entry) for connector_id, entry in canonical.items()}
    for connector_id, incoming in imported.items():
        existing = merged.get(connector_id)
        if not isinstance(existing, dict):
            merged[connector_id] = dict(incoming)
            continue
        if _entry_rank(incoming, connector_id) > _entry_rank(existing, connector_id):
            merged[connector_id] = {**existing, **incoming}
        else:
            merged[connector_id] = {**incoming, **existing}
    return _collapse_active_connector_duplicates(merged)


def _migrate_rust_settings_store(home: Optional[Path] = None) -> None:
    """Move the former Rust-only settings section into the shared connector file."""

    settings_path = _settings_path(home)
    if not settings_path.exists():
        return
    try:
        initial = yaml.safe_load(settings_path.read_text(encoding="utf-8")) or {}
    except Exception:
        return
    if not isinstance(initial, dict) or _SETTINGS_STORE_KEY not in initial:
        return

    settings_lock = acquire_lockfile(_settings_lock_path(home))
    try:
        try:
            settings = yaml.safe_load(settings_path.read_text(encoding="utf-8")) or {}
        except Exception:
            return
        if not isinstance(settings, dict) or _SETTINGS_STORE_KEY not in settings:
            return
        imported = _normalized_connector_map(settings.get(_SETTINGS_STORE_KEY))
        connector_lock = acquire_lockfile(_connectors_lock_path(home))
        try:
            canonical = _read_connectors_unlocked(home)
            if imported:
                _write_connectors_unlocked(
                    _merge_connector_maps(canonical, imported), home
                )
            settings.pop(_SETTINGS_STORE_KEY, None)
            atomic_write_text(
                settings_path,
                yaml.safe_dump(settings, allow_unicode=True, sort_keys=False),
            )
            try:
                from .settings import _invalidate_settings_cache

                _invalidate_settings_cache()
            except Exception:
                pass
        finally:
            release_lockfile(connector_lock)
    finally:
        release_lockfile(settings_lock)


def _mutate_web_model_connectors(
    change: Callable[[Dict[str, Dict[str, Any]]], _T],
    home: Optional[Path] = None,
) -> _T:
    _migrate_rust_settings_store(home)
    lock = acquire_lockfile(_connectors_lock_path(home))
    try:
        connectors = _read_connectors_unlocked(home)
        result = change(connectors)
        _write_connectors_unlocked(connectors, home)
        return result
    finally:
        release_lockfile(lock)


def load_web_model_connectors(home: Optional[Path] = None) -> Dict[str, Dict[str, Any]]:
    _migrate_rust_settings_store(home)
    lock = acquire_lockfile(_connectors_lock_path(home))
    try:
        return _read_connectors_unlocked(home)
    finally:
        release_lockfile(lock)


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

    def _create(connectors: Dict[str, Dict[str, Any]]) -> Dict[str, Any]:
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
        return {
            **entry,
            "secret": secret,
            "replaced_connector_ids": replaced_connector_ids,
        }

    return _mutate_web_model_connectors(_create, home)


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
    def _revoke(connectors: Dict[str, Dict[str, Any]]) -> bool:
        entry = connectors.get(cid)
        if not isinstance(entry, dict):
            return False
        entry["revoked"] = True
        entry["updated_at"] = utc_now_iso()
        connectors[cid] = entry
        return True

    return _mutate_web_model_connectors(_revoke, home)


def retire_web_model_connectors_for_actor(
    group_id: str,
    actor_id: str,
    home: Optional[Path] = None,
) -> List[Dict[str, Any]]:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        return []

    def _retire(connectors: Dict[str, Dict[str, Any]]) -> List[Dict[str, Any]]:
        retired: List[Dict[str, Any]] = []
        now = utc_now_iso()
        for connector_id, entry in connectors.items():
            if not isinstance(entry, dict) or bool(entry.get("revoked")):
                continue
            if str(entry.get("group_id") or "").strip() != gid:
                continue
            if str(entry.get("actor_id") or "").strip() != aid:
                continue
            retired.append(dict(entry))
            entry["revoked"] = True
            entry["updated_at"] = now
            connectors[connector_id] = entry
        return retired

    return _mutate_web_model_connectors(_retire, home)


def retire_web_model_connectors_for_group(
    group_id: str,
    home: Optional[Path] = None,
) -> List[Dict[str, Any]]:
    gid = str(group_id or "").strip()
    if not gid:
        return []

    def _retire(connectors: Dict[str, Dict[str, Any]]) -> List[Dict[str, Any]]:
        retired: List[Dict[str, Any]] = []
        now = utc_now_iso()
        for connector_id, entry in connectors.items():
            if not isinstance(entry, dict) or bool(entry.get("revoked")):
                continue
            if str(entry.get("group_id") or "").strip() != gid:
                continue
            retired.append(dict(entry))
            entry["revoked"] = True
            entry["updated_at"] = now
            connectors[connector_id] = entry
        return retired

    return _mutate_web_model_connectors(_retire, home)


def restore_web_model_connectors(
    entries: List[Dict[str, Any]],
    home: Optional[Path] = None,
) -> None:
    snapshots = [dict(entry) for entry in entries if isinstance(entry, dict)]
    if not snapshots:
        return

    def _restore(connectors: Dict[str, Dict[str, Any]]) -> None:
        for entry in snapshots:
            connector_id = str(entry.get("connector_id") or "").strip()
            normalized = _normalize_entry(connector_id, entry)
            if normalized is None:
                raise ValueError("invalid web-model connector snapshot")
            connectors[connector_id] = normalized
        _collapse_active_connector_duplicates(connectors)

    _mutate_web_model_connectors(_restore, home)


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
    def _record(
        connectors: Dict[str, Dict[str, Any]],
    ) -> Optional[Dict[str, Any]]:
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
        return mask_web_model_connector(entry)

    return _mutate_web_model_connectors(_record, home)
