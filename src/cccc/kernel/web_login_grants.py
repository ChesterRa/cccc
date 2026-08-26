from __future__ import annotations

import hashlib
import json
import os
import secrets
import time
from pathlib import Path
from typing import Any, Optional
from urllib.parse import urlsplit

from ..paths import ensure_home
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.fs import atomic_write_json

DEFAULT_TTL_SECONDS = 120
_MAX_GRANTS = 64
_STORE_FILE = "web_login_grants.json"
_LOCK_FILE = "web_login_grants.lock"


def access_token_id(raw: str) -> str:
    return hashlib.sha256(str(raw or "").encode("utf-8")).hexdigest()[:16]


def normalize_origin(value: str) -> str:
    try:
        parsed = urlsplit(str(value or "").strip())
        hostname = str(parsed.hostname or "").lower()
        port = parsed.port
    except ValueError:
        return ""
    scheme = parsed.scheme.lower()
    if (
        scheme not in {"http", "https"}
        or not hostname
        or parsed.username is not None
        or parsed.password is not None
    ):
        return ""
    display_host = f"[{hostname}]" if ":" in hostname else hostname
    default_port = 443 if scheme == "https" else 80
    suffix = f":{port}" if port is not None and port != default_port else ""
    return f"{scheme}://{display_host}{suffix}"


def issue_web_login_grant(
    origin: str,
    token_id: str,
    *,
    ttl_seconds: int = DEFAULT_TTL_SECONDS,
    home: Optional[Path] = None,
) -> dict[str, Any]:
    normalized_origin = normalize_origin(origin)
    normalized_token_id = str(token_id or "").strip().lower()
    if not normalized_origin:
        raise ValueError("Web login grant origin must be HTTP(S)")
    if len(normalized_token_id) != 16 or not all(
        char in "0123456789abcdef" for char in normalized_token_id
    ):
        raise ValueError("Web login grant token id is invalid")
    now = int(time.time())
    expires_at = now + max(30, min(300, int(ttl_seconds)))
    code = f"wlg_{secrets.token_hex(16)}"
    digest = _code_digest(code)
    root = _root(home)
    lock = acquire_lockfile(root / _LOCK_FILE, blocking=True)
    try:
        document = _load(root)
        grants = _pruned_grants(document.get("grants"), now)
        while len(grants) >= _MAX_GRANTS:
            oldest = min(
                grants,
                key=lambda key: int((grants.get(key) or {}).get("created_at_epoch") or 0),
            )
            grants.pop(oldest, None)
        grants[digest] = {
            "token_id": normalized_token_id,
            "origin": normalized_origin,
            "created_at_epoch": now,
            "expires_at_epoch": expires_at,
        }
        _save(root, grants)
    finally:
        release_lockfile(lock)
    return {
        "code": code,
        "origin": normalized_origin,
        "expires_at_epoch": expires_at,
    }


def consume_web_login_grant(
    code: str,
    origin: str,
    *,
    home: Optional[Path] = None,
) -> Optional[str]:
    candidate = str(code or "").strip()
    normalized_origin = normalize_origin(origin)
    if (
        not normalized_origin
        or len(candidate) != 36
        or not candidate.startswith("wlg_")
        or not all(char in "0123456789abcdef" for char in candidate[4:])
    ):
        return None
    now = int(time.time())
    root = _root(home)
    lock = acquire_lockfile(root / _LOCK_FILE, blocking=True)
    try:
        document = _load(root)
        grants = _pruned_grants(document.get("grants"), now)
        digest = _code_digest(candidate)
        record = grants.get(digest)
        token_id = None
        if (
            isinstance(record, dict)
            and str(record.get("origin") or "") == normalized_origin
            and int(record.get("expires_at_epoch") or 0) > now
        ):
            token_id = str(record.get("token_id") or "").strip()
            grants.pop(digest, None)
        _save(root, grants)
        return token_id or None
    finally:
        release_lockfile(lock)


def _root(home: Optional[Path]) -> Path:
    root = Path(home) if home is not None else ensure_home()
    root.mkdir(parents=True, exist_ok=True)
    return root


def _load(root: Path) -> dict[str, Any]:
    path = root / _STORE_FILE
    if not path.exists():
        return {"v": 1, "grants": {}}
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or int(value.get("v") or 1) != 1:
        raise ValueError("Web login grant store is invalid")
    return value


def _pruned_grants(raw: Any, now: int) -> dict[str, dict[str, Any]]:
    source = raw if isinstance(raw, dict) else {}
    grants: dict[str, dict[str, Any]] = {}
    for digest, record in source.items():
        if (
            isinstance(digest, str)
            and len(digest) == 64
            and all(char in "0123456789abcdef" for char in digest)
            and isinstance(record, dict)
            and int(record.get("expires_at_epoch") or 0) > now
            and normalize_origin(str(record.get("origin") or ""))
            == str(record.get("origin") or "")
        ):
            grants[digest] = dict(record)
    return grants


def _save(root: Path, grants: dict[str, dict[str, Any]]) -> None:
    path = root / _STORE_FILE
    atomic_write_json(path, {"v": 1, "grants": grants})
    if os.name != "nt":
        path.chmod(0o600)


def _code_digest(code: str) -> str:
    return hashlib.sha256(code.encode("utf-8")).hexdigest()
