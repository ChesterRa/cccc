from __future__ import annotations

import hashlib
import os
import re
from pathlib import Path
from typing import Any, Dict

from ..paths import ensure_home
from ..util.fs import atomic_write_json, read_json

_PRIVATE_ENV_KEY_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
_PRIVATE_ENV_MAX_VALUE_CHARS = 200_000
PRIVATE_ENV_MAX_KEYS = 256


def _private_env_root(home: Path) -> Path:
    return home / "state" / "secrets" / "actors"


def _private_env_group_dir(home: Path, *, group_id: str) -> Path:
    gid = str(group_id or "").strip()
    if not gid:
        raise ValueError("missing group_id")
    if "/" in gid or "\\" in gid or ".." in gid:
        raise ValueError("invalid group_id")
    return _private_env_root(home) / gid


def _private_env_actor_filename(actor_id: str) -> str:
    raw = str(actor_id or "").strip()
    digest = hashlib.sha256(raw.encode("utf-8")).hexdigest()[:16]
    slug = re.sub(r"[^a-zA-Z0-9._-]+", "_", raw).strip("._-")
    if not slug:
        slug = "actor"
    slug = slug[:24]
    return f"{slug}.{digest}.json"


def _ensure_private_env_dir(path: Path) -> None:
    try:
        path.mkdir(parents=True, exist_ok=True)
        try:
            os.chmod(path, 0o700)
        except Exception:
            pass
    except Exception:
        pass


def validate_private_env_key(key: Any) -> str:
    k = str(key or "").strip()
    if not k:
        raise ValueError("missing env key")
    if not _PRIVATE_ENV_KEY_RE.match(k):
        raise ValueError(f"invalid env key: {k}")
    return k


def coerce_private_env_value(value: Any) -> str:
    if value is None:
        raise ValueError("missing env value")
    v = str(value)
    if len(v) > _PRIVATE_ENV_MAX_VALUE_CHARS:
        raise ValueError("env value too large")
    return v


def mask_private_env_value(value: Any) -> str:
    """Return a stable masked preview for UI metadata.

    This never returns the original value. Short values are fully masked.
    Longer values keep a tiny prefix/suffix to help users distinguish entries.
    """
    raw = str(value or "")
    if len(raw) <= 6:
        return "******"
    return f"{raw[:2]}******{raw[-2:]}"


def _private_env_path(group_id: str, actor_id: str) -> Path:
    home = ensure_home()
    gdir = _private_env_group_dir(home, group_id=group_id)
    return gdir / _private_env_actor_filename(actor_id)


def load_actor_private_env(group_id: str, actor_id: str) -> dict[str, str]:
    try:
        path = _private_env_path(group_id, actor_id)
    except Exception:
        return {}
    raw = read_json(path)
    if not isinstance(raw, dict):
        return {}
    out: dict[str, str] = {}
    for k, v in raw.items():
        if not isinstance(k, str):
            continue
        kk = k.strip()
        if not kk or not _PRIVATE_ENV_KEY_RE.match(kk):
            continue
        if v is None:
            continue
        out[kk] = str(v)
    return out


def update_actor_private_env(
    group_id: str,
    actor_id: str,
    *,
    set_vars: dict[str, str],
    unset_keys: list[str],
    clear: bool,
) -> dict[str, str]:
    current: dict[str, str] = {} if clear else load_actor_private_env(group_id, actor_id)
    for k in unset_keys:
        current.pop(k, None)
    for k, v in set_vars.items():
        current[k] = v

    try:
        home = ensure_home()
        root = _private_env_root(home)
        gdir = _private_env_group_dir(home, group_id=group_id)
        path = gdir / _private_env_actor_filename(actor_id)
    except Exception:
        raise RuntimeError("invalid private env path")

    if not current:
        try:
            path.unlink(missing_ok=True)
        except Exception:
            pass
        try:
            if gdir.exists() and gdir.is_dir() and not any(gdir.iterdir()):
                gdir.rmdir()
        except Exception:
            pass
        return {}

    _ensure_private_env_dir(root)
    _ensure_private_env_dir(gdir)
    atomic_write_json(path, current, indent=2)
    try:
        os.chmod(path, 0o600)
    except Exception:
        pass
    return dict(current)


def delete_actor_private_env(group_id: str, actor_id: str) -> None:
    try:
        home = ensure_home()
        gdir = _private_env_group_dir(home, group_id=group_id)
        path = gdir / _private_env_actor_filename(actor_id)
        path.unlink(missing_ok=True)
        if gdir.exists() and gdir.is_dir() and not any(gdir.iterdir()):
            try:
                gdir.rmdir()
            except Exception:
                pass
    except Exception:
        pass


def delete_group_private_env(group_id: str) -> None:
    try:
        home = ensure_home()
        gdir = _private_env_group_dir(home, group_id=group_id)
        if gdir.exists():
            import shutil

            shutil.rmtree(gdir, ignore_errors=True)
    except Exception:
        pass


def merge_actor_env_with_private(group_id: str, actor_id: str, env: Dict[str, Any]) -> Dict[str, Any]:
    base = dict(env or {})
    try:
        private_env = load_actor_private_env(group_id, actor_id)
        if private_env:
            base.update(private_env)
    except Exception:
        pass
    return base
