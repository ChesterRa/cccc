from __future__ import annotations

import hashlib
import json
import os
import re
from pathlib import Path
from typing import Any, Dict

from ...paths import ensure_home
from ...util.file_lock import acquire_lockfile, release_lockfile
from ...util.fs import atomic_write_json, read_json

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


def _migrate_legacy_actor_private_env(home: Path, group_id: str) -> None:
    """Consume Rust's former group-local secret store before canonical access."""
    gdir = _private_env_group_dir(home, group_id=group_id)
    marker = gdir / ".rust-actor-secrets-migrated-v1"
    legacy = home / "groups" / group_id / "state" / "actor-secrets.json"
    if marker.exists() or not legacy.exists():
        return
    _ensure_private_env_dir(_private_env_root(home))
    _ensure_private_env_dir(gdir)
    lock = acquire_lockfile(gdir / ".migration.lock", blocking=True)
    try:
        if marker.exists():
            return
        raw = json.loads(legacy.read_text(encoding="utf-8"))
        actors = raw.get("actors") if isinstance(raw, dict) else None
        if isinstance(actors, dict):
            for actor_id, values in actors.items():
                if not isinstance(values, dict):
                    continue
                target = gdir / _private_env_actor_filename(str(actor_id))
                if target.exists():
                    continue
                migrated = {
                    str(key): str(value)
                    for key, value in values.items()
                    if isinstance(key, str)
                    and _PRIVATE_ENV_KEY_RE.match(key)
                    and value is not None
                }
                if not migrated:
                    continue
                _ensure_private_env_dir(_private_env_root(home))
                _ensure_private_env_dir(gdir)
                atomic_write_json(target, migrated, indent=2)
                try:
                    os.chmod(target, 0o600)
                except Exception:
                    pass
        _ensure_private_env_dir(gdir)
        marker.write_text("migrated from state/actor-secrets.json\n", encoding="utf-8")
    finally:
        release_lockfile(lock)


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


def _private_env_lock_path(path: Path) -> Path:
    # Shared with Rust: <actor-slug>.<digest>.json.lock.
    return path.with_suffix(path.suffix + ".lock")


def _load_actor_private_env_path(path: Path) -> dict[str, str]:
    if not path.exists():
        return {}
    raw = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(raw, dict):
        raise ValueError(f"actor private env store must be a JSON object: {path}")
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


def _write_actor_private_env_path(path: Path, values: dict[str, str]) -> None:
    if not values:
        path.unlink(missing_ok=True)
        return
    _ensure_private_env_dir(path.parent.parent)
    _ensure_private_env_dir(path.parent)
    atomic_write_json(path, values, indent=2)
    try:
        os.chmod(path, 0o600)
    except Exception:
        pass


def load_actor_private_env(group_id: str, actor_id: str) -> dict[str, str]:
    _migrate_legacy_actor_private_env(ensure_home(), group_id)
    try:
        path = _private_env_path(group_id, actor_id)
    except Exception:
        return {}
    return _load_actor_private_env_path(path)


def update_actor_private_env(
    group_id: str,
    actor_id: str,
    *,
    set_vars: dict[str, str],
    unset_keys: list[str],
    clear: bool,
) -> dict[str, str]:
    home = ensure_home()
    _migrate_legacy_actor_private_env(home, group_id)
    try:
        path = _private_env_group_dir(home, group_id=group_id) / _private_env_actor_filename(actor_id)
    except Exception as error:
        raise RuntimeError("invalid private env path") from error

    lock = acquire_lockfile(_private_env_lock_path(path), blocking=True)
    try:
        current: dict[str, str] = {} if clear else _load_actor_private_env_path(path)
        for k in unset_keys:
            current.pop(k, None)
        for k, v in set_vars.items():
            current[k] = v
        _write_actor_private_env_path(path, current)
        return dict(current)
    finally:
        release_lockfile(lock)


def delete_actor_private_env(group_id: str, actor_id: str) -> None:
    home = ensure_home()
    _migrate_legacy_actor_private_env(home, group_id)
    gdir = _private_env_group_dir(home, group_id=group_id)
    path = gdir / _private_env_actor_filename(actor_id)
    lock = acquire_lockfile(_private_env_lock_path(path), blocking=True)
    try:
        path.unlink(missing_ok=True)
    finally:
        release_lockfile(lock)


def delete_group_private_env(group_id: str) -> None:
    try:
        home = ensure_home()
        gdir = _private_env_group_dir(home, group_id=group_id)
        if gdir.exists():
            import shutil

            shutil.rmtree(gdir, ignore_errors=True)
    except Exception:
        pass


def copy_group_private_env(source_group_id: str, target_group_id: str) -> int:
    """Copy actor private-env files between groups without exposing secret values."""
    try:
        home = ensure_home()
        _migrate_legacy_actor_private_env(home, source_group_id)
        _migrate_legacy_actor_private_env(home, target_group_id)
        src = _private_env_group_dir(home, group_id=source_group_id)
        dst = _private_env_group_dir(home, group_id=target_group_id)
    except Exception:
        return 0
    if not src.exists() or not src.is_dir():
        return 0

    count = 0
    _ensure_private_env_dir(_private_env_root(home))
    _ensure_private_env_dir(dst)
    for path in sorted(src.iterdir(), key=lambda item: item.name):
        if not path.is_file() or path.suffix != ".json":
            continue
        raw = read_json(path)
        if not isinstance(raw, dict):
            continue
        target = dst / path.name
        atomic_write_json(target, raw, indent=2)
        try:
            os.chmod(target, 0o600)
        except Exception:
            pass
        count += 1
    return count


def merge_actor_env_with_private(group_id: str, actor_id: str, env: Dict[str, Any]) -> Dict[str, Any]:
    base = dict(env or {})
    try:
        private_env = load_actor_private_env(group_id, actor_id)
        if private_env:
            base.update(private_env)
    except Exception:
        pass
    return base
