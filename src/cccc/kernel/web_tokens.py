from __future__ import annotations

import secrets
from pathlib import Path
from typing import Any, Dict, List, Optional

import yaml

from ..paths import ensure_home
from ..util.fs import atomic_write_text
from ..util.time import utc_now_iso

_TOKEN_PREFIX = "usr_"


def _web_tokens_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "web_tokens.yaml"


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
    return {
        "token": tok,
        "kind": "user",
        "user_id": user_id,
        "allowed_groups": _normalize_allowed_groups(raw.get("allowed_groups")),
        "is_admin": bool(raw.get("is_admin", False)),
        "created_at": created_at,
        "updated_at": updated_at,
    }


def load_tokens(home: Optional[Path] = None) -> Dict[str, Dict[str, Any]]:
    path = _web_tokens_path(home)
    if not path.exists():
        return {}
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
    return out


def save_tokens(tokens: Dict[str, Dict[str, Any]], home: Optional[Path] = None) -> None:
    path = _web_tokens_path(home)
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


def lookup_token(token: str, home: Optional[Path] = None) -> Optional[Dict[str, Any]]:
    tok = str(token or "").strip()
    if not tok:
        return None
    return load_tokens(home).get(tok)


def _new_token_value(existing: Dict[str, Dict[str, Any]]) -> str:
    while True:
        candidate = f"{_TOKEN_PREFIX}{secrets.token_hex(16)}"
        if candidate not in existing:
            return candidate


def create_token(
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
    tokens = load_tokens(home)
    now = utc_now_iso()
    if custom_token and str(custom_token).strip():
        token = str(custom_token).strip()
        if token in tokens:
            raise ValueError("token already exists")
    else:
        token = _new_token_value(tokens)
    entry = {
        "token": token,
        "kind": "user",
        "user_id": uid,
        "allowed_groups": _normalize_allowed_groups(allowed_groups or []),
        "is_admin": bool(is_admin),
        "created_at": now,
        "updated_at": now,
    }
    tokens[token] = entry
    save_tokens(tokens, home)
    return dict(entry)


def update_token(
    token: str,
    *,
    allowed_groups: Optional[List[str]] = None,
    is_admin: Optional[bool] = None,
    home: Optional[Path] = None,
) -> Optional[Dict[str, Any]]:
    tok = str(token or "").strip()
    if not tok:
        return None
    tokens = load_tokens(home)
    if tok not in tokens:
        return None
    entry = tokens[tok]
    if allowed_groups is not None:
        entry["allowed_groups"] = _normalize_allowed_groups(allowed_groups)
    if is_admin is not None:
        entry["is_admin"] = bool(is_admin)
    entry["updated_at"] = utc_now_iso()
    tokens[tok] = entry
    save_tokens(tokens, home)
    return dict(entry)


def delete_token(token: str, home: Optional[Path] = None) -> bool:
    tok = str(token or "").strip()
    if not tok:
        return False
    tokens = load_tokens(home)
    if tok not in tokens:
        return False
    del tokens[tok]
    save_tokens(tokens, home)
    return True


def list_tokens(home: Optional[Path] = None) -> List[Dict[str, Any]]:
    items = list(load_tokens(home).values())
    items.sort(key=lambda item: (str(item.get("created_at") or ""), str(item.get("token") or "")), reverse=True)
    return items
