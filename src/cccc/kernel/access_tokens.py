from __future__ import annotations

import secrets
import threading
from copy import deepcopy
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

from ..paths import ensure_home
from ..util.fs import atomic_write_text
from ..util.time import utc_now_iso

_TOKEN_PREFIX = "acc_"
_CACHE_LOCK = threading.Lock()
_CACHE_KEY: tuple[str, int, int] | None = None
_CACHE_TOKENS: Dict[str, Dict[str, Any]] | None = None


def _access_tokens_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "access_tokens.yaml"


def _clone_tokens(tokens: Dict[str, Dict[str, Any]]) -> Dict[str, Dict[str, Any]]:
    return {str(token): deepcopy(entry) for token, entry in tokens.items()}


def _invalidate_access_tokens_cache() -> None:
    global _CACHE_KEY, _CACHE_TOKENS
    with _CACHE_LOCK:
        _CACHE_KEY = None
        _CACHE_TOKENS = None


def _normalize_allowed_groups(raw: Any) -> List[str]:
    if not isinstance(raw, list):
        return []
    seen: set[str] = set()
    groups: List[str] = []
    for item in raw:
        gid = str(item or "").strip()
        if not gid or gid in seen:
            continue
        seen.add(gid)
        groups.append(gid)
    return groups


def _normalize_entry(token: str, raw: Any) -> Optional[Dict[str, Any]]:
    tok = str(token or "").strip()
    if not tok or not isinstance(raw, dict):
        return None
    user_id = str(raw.get("user_id") or "").strip()
    if not user_id:
        return None
    created_at = str(raw.get("created_at") or "").strip() or utc_now_iso()
    updated_at = str(raw.get("updated_at") or "").strip() or created_at
    is_admin = bool(raw.get("is_admin", False))
    return {
        "token": tok,
        "kind": "access",
        "user_id": user_id,
        "allowed_groups": [] if is_admin else _normalize_allowed_groups(raw.get("allowed_groups")),
        "is_admin": is_admin,
        "created_at": created_at,
        "updated_at": updated_at,
    }


def load_access_tokens(home: Optional[Path] = None) -> Dict[str, Dict[str, Any]]:
    global _CACHE_KEY, _CACHE_TOKENS
    path = _access_tokens_path(home)
    if not path.exists():
        return {}
    try:
        stat = path.stat()
        cache_key = (str(path), int(stat.st_mtime_ns), int(stat.st_size))
    except Exception:
        cache_key = None
    if cache_key is not None:
        with _CACHE_LOCK:
            if _CACHE_KEY == cache_key and _CACHE_TOKENS is not None:
                return _clone_tokens(_CACHE_TOKENS)
    try:
        raw = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
    except Exception:
        return {}
    if not isinstance(raw, dict):
        return {}
    token_map = raw.get("tokens") if isinstance(raw.get("tokens"), dict) else raw
    if not isinstance(token_map, dict):
        return {}
    out: Dict[str, Dict[str, Any]] = {}
    for token, entry in token_map.items():
        normalized = _normalize_entry(str(token or ""), entry)
        if normalized is None:
            continue
        out[normalized["token"]] = normalized
    if cache_key is not None:
        with _CACHE_LOCK:
            _CACHE_KEY = cache_key
            _CACHE_TOKENS = _clone_tokens(out)
    return out


def save_access_tokens(tokens: Dict[str, Dict[str, Any]], home: Optional[Path] = None) -> None:
    path = _access_tokens_path(home)
    payload: Dict[str, Any] = {"tokens": {}}
    for token, entry in sorted(tokens.items(), key=lambda item: item[0]):
        normalized = _normalize_entry(token, entry)
        if normalized is None:
            continue
        payload["tokens"][normalized["token"]] = {
            "user_id": normalized["user_id"],
            "allowed_groups": list(normalized["allowed_groups"]),
            "is_admin": bool(normalized["is_admin"]),
            "created_at": normalized["created_at"],
            "updated_at": normalized["updated_at"],
        }
    atomic_write_text(
        path,
        yaml.safe_dump(payload, allow_unicode=True, sort_keys=False, default_flow_style=False),
    )
    _invalidate_access_tokens_cache()


def lookup_access_token(token: str, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    tok = str(token or "").strip()
    if not tok:
        return None
    return load_access_tokens(home).get(tok)


def _new_access_token_value(existing: Dict[str, Dict[str, Any]]) -> str:
    while True:
        candidate = f"{_TOKEN_PREFIX}{secrets.token_hex(16)}"
        if candidate not in existing:
            return candidate


def create_access_token(
    user_id: str,
    *,
    allowed_groups: Optional[List[str]] = None,
    is_admin: bool = False,
    custom_token: Optional[str] = None,
    home: Optional[Path] = None,
) -> Dict[str, Any]:
    uid = str(user_id or "").strip()
    if not uid:
        raise ValueError("user_id is required")
    tokens = load_access_tokens(home)
    now = utc_now_iso()
    if custom_token and str(custom_token).strip():
        token = str(custom_token).strip()
        if token in tokens:
            raise ValueError("access token already exists")
    else:
        token = _new_access_token_value(tokens)
    effective_is_admin = bool(is_admin)
    entry = {
        "token": token,
        "kind": "access",
        "user_id": uid,
        "allowed_groups": [] if effective_is_admin else _normalize_allowed_groups(allowed_groups or []),
        "is_admin": effective_is_admin,
        "created_at": now,
        "updated_at": now,
    }
    tokens[token] = entry
    save_access_tokens(tokens, home)
    return dict(entry)


def update_access_token(
    token: str,
    *,
    allowed_groups: Optional[List[str]] = None,
    is_admin: Optional[bool] = None,
    home: Optional[Path] = None,
) -> Optional[Dict[str, Any]]:
    tok = str(token or "").strip()
    if not tok:
        return None
    tokens = load_access_tokens(home)
    if tok not in tokens:
        return None
    entry = tokens[tok]
    next_is_admin = entry.get("is_admin", False) if is_admin is None else bool(is_admin)
    if next_is_admin:
        entry["allowed_groups"] = []
    elif allowed_groups is not None:
        entry["allowed_groups"] = _normalize_allowed_groups(allowed_groups)
    if is_admin is not None:
        entry["is_admin"] = bool(is_admin)
    entry["updated_at"] = utc_now_iso()
    tokens[tok] = entry
    save_access_tokens(tokens, home)
    return dict(entry)


def delete_access_token(token: str, home: Optional[Path] = None) -> bool:
    tok = str(token or "").strip()
    if not tok:
        return False
    tokens = load_access_tokens(home)
    if tok not in tokens:
        return False
    del tokens[tok]
    save_access_tokens(tokens, home)
    return True


def list_access_tokens(home: Optional[Path] = None) -> List[Dict[str, Any]]:
    items = list(load_access_tokens(home).values())
    items.sort(key=lambda item: (str(item.get("created_at") or ""), str(item.get("token") or "")), reverse=True)
    return items
