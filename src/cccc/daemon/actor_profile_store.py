from __future__ import annotations

import hashlib
import os
import re
import secrets
import shlex
from pathlib import Path
from typing import Any, Dict, List, Optional

from ..contracts.v1 import ActorProfile
from ..kernel.runtime import get_runtime_command_with_flags
from ..paths import ensure_home
from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso

_PROFILE_ID_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")


class ProfileRevisionMismatchError(RuntimeError):
    pass


def _profiles_root(home: Path) -> Path:
    return home / "state" / "actor_profiles"


def _profiles_path(home: Path) -> Path:
    return _profiles_root(home) / "profiles.json"


def _profile_secret_root(home: Path) -> Path:
    return home / "state" / "secrets" / "actor_profiles"


def _ensure_dir(path: Path, mode: int) -> None:
    path.mkdir(parents=True, exist_ok=True)
    try:
        os.chmod(path, mode)
    except Exception:
        pass


def validate_actor_profile_id(profile_id: Any) -> str:
    pid = str(profile_id or "").strip()
    if not pid:
        raise ValueError("missing profile_id")
    if "/" in pid or "\\" in pid or ".." in pid:
        raise ValueError("invalid profile_id")
    if not _PROFILE_ID_RE.match(pid):
        raise ValueError("invalid profile_id")
    return pid


def _new_profile_id() -> str:
    return f"ap_{secrets.token_hex(6)}"


def _normalize_profile_command(*, runtime: str, runner: str, command: Any) -> List[str]:
    cmd: List[str] = []
    if isinstance(command, list):
        cmd = [str(item).strip() for item in command if isinstance(item, str) and str(item).strip()]
    elif isinstance(command, str):
        raw = command.strip()
        if raw:
            try:
                cmd = shlex.split(raw)
            except Exception:
                cmd = [raw]
    if runner != "headless" and runtime != "custom" and not cmd:
        cmd = get_runtime_command_with_flags(runtime)
    if runtime == "custom" and runner != "headless" and not cmd:
        raise ValueError("custom runtime requires a command (PTY runner)")
    return cmd


def _new_profiles_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 1,
        "created_at": now,
        "updated_at": now,
        "profiles": {},
    }


def _normalize_profiles_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict) or not raw:
        return _new_profiles_doc()
    doc = dict(raw)
    if not isinstance(doc.get("profiles"), dict):
        doc["profiles"] = {}
    if "v" not in doc:
        doc["v"] = 1
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = utc_now_iso()
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = utc_now_iso()
    return doc


def _load_profiles_doc() -> tuple[Path, Dict[str, Any]]:
    home = ensure_home()
    path = _profiles_path(home)
    raw = read_json(path)
    doc = _normalize_profiles_doc(raw)
    return path, doc


def _save_profiles_doc(path: Path, doc: Dict[str, Any]) -> None:
    _ensure_dir(path.parent, 0o700)
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(path, doc, indent=2)
    try:
        os.chmod(path, 0o600)
    except Exception:
        pass


def list_actor_profiles() -> List[Dict[str, Any]]:
    _, doc = _load_profiles_doc()
    out: List[Dict[str, Any]] = []
    raw_profiles = doc.get("profiles") if isinstance(doc.get("profiles"), dict) else {}
    for pid, raw in raw_profiles.items():
        if not isinstance(raw, dict):
            continue
        try:
            validate_actor_profile_id(pid)
            profile = ActorProfile.model_validate(raw).model_dump(exclude_none=True)
            out.append(profile)
        except Exception:
            continue
    out.sort(key=lambda item: str(item.get("name") or str(item.get("id") or "")).casefold())
    return out


def get_actor_profile(profile_id: str) -> Optional[Dict[str, Any]]:
    pid = validate_actor_profile_id(profile_id)
    _, doc = _load_profiles_doc()
    raw_profiles = doc.get("profiles") if isinstance(doc.get("profiles"), dict) else {}
    raw = raw_profiles.get(pid)
    if not isinstance(raw, dict):
        return None
    try:
        return ActorProfile.model_validate(raw).model_dump(exclude_none=True)
    except Exception:
        return None


def upsert_actor_profile(
    profile: Dict[str, Any],
    *,
    expected_revision: Optional[int] = None,
) -> Dict[str, Any]:
    if not isinstance(profile, dict):
        raise ValueError("profile must be an object")

    path, doc = _load_profiles_doc()
    profiles = doc.get("profiles") if isinstance(doc.get("profiles"), dict) else {}
    doc["profiles"] = profiles

    profile_id_raw = profile.get("id")
    pid = validate_actor_profile_id(profile_id_raw) if str(profile_id_raw or "").strip() else _new_profile_id()
    now = utc_now_iso()

    existing_raw = profiles.get(pid)
    existing: Optional[Dict[str, Any]] = None
    if isinstance(existing_raw, dict):
        try:
            existing = ActorProfile.model_validate(existing_raw).model_dump(exclude_none=True)
        except Exception:
            existing = None

    if expected_revision is not None:
        expected = int(expected_revision)
        current = int(existing.get("revision") or 0) if isinstance(existing, dict) else 0
        if current != expected:
            raise ProfileRevisionMismatchError(f"profile revision mismatch (expected {expected}, got {current})")

    runtime = str(profile.get("runtime") if "runtime" in profile else (existing or {}).get("runtime") or "codex").strip() or "codex"
    runner = str(profile.get("runner") if "runner" in profile else (existing or {}).get("runner") or "pty").strip() or "pty"
    submit = str(profile.get("submit") if "submit" in profile else (existing or {}).get("submit") or "enter").strip() or "enter"
    name = str(profile.get("name") if "name" in profile else (existing or {}).get("name") or "").strip()

    env_in = profile.get("env") if "env" in profile else (existing or {}).get("env")
    env: Dict[str, str] = {}
    if isinstance(env_in, dict):
        for key, value in env_in.items():
            if not isinstance(key, str):
                continue
            env[str(key)] = str(value)

    command_in = profile.get("command") if "command" in profile else (existing or {}).get("command")
    command = _normalize_profile_command(runtime=runtime, runner=runner, command=command_in)

    created_at = str((existing or {}).get("created_at") or now)
    revision = int((existing or {}).get("revision") or 0) + 1

    payload = {
        "id": pid,
        "name": name,
        "runtime": runtime,
        "runner": runner,
        "command": command,
        "submit": submit,
        "env": env,
        "created_at": created_at,
        "updated_at": now,
        "revision": revision,
    }
    model = ActorProfile.model_validate(payload)
    out = model.model_dump(exclude_none=True)
    profiles[pid] = out
    _save_profiles_doc(path, doc)
    return out


def delete_actor_profile(profile_id: str) -> None:
    pid = validate_actor_profile_id(profile_id)
    path, doc = _load_profiles_doc()
    profiles = doc.get("profiles") if isinstance(doc.get("profiles"), dict) else {}
    if pid not in profiles:
        raise ValueError(f"profile not found: {pid}")
    profiles.pop(pid, None)
    doc["profiles"] = profiles
    _save_profiles_doc(path, doc)


def _profile_secret_filename(profile_id: str) -> str:
    pid = validate_actor_profile_id(profile_id)
    digest = hashlib.sha256(pid.encode("utf-8")).hexdigest()[:16]
    slug = re.sub(r"[^A-Za-z0-9._-]+", "_", pid).strip("._-")
    if not slug:
        slug = "profile"
    slug = slug[:32]
    return f"{slug}.{digest}.json"


def _profile_secret_path(profile_id: str) -> Path:
    home = ensure_home()
    root = _profile_secret_root(home)
    return root / _profile_secret_filename(profile_id)


def load_actor_profile_secrets(profile_id: str) -> Dict[str, str]:
    pid = validate_actor_profile_id(profile_id)
    path = _profile_secret_path(pid)
    raw = read_json(path)
    out: Dict[str, str] = {}
    if not isinstance(raw, dict):
        return out
    for key, value in raw.items():
        if not isinstance(key, str):
            continue
        k = key.strip()
        if not k:
            continue
        if value is None:
            continue
        out[k] = str(value)
    return out


def update_actor_profile_secrets(
    profile_id: str,
    *,
    set_vars: Dict[str, str],
    unset_keys: List[str],
    clear: bool,
) -> Dict[str, str]:
    pid = validate_actor_profile_id(profile_id)
    current = {} if clear else load_actor_profile_secrets(pid)
    for key in unset_keys:
        current.pop(str(key), None)
    for key, value in set_vars.items():
        current[str(key)] = str(value)

    path = _profile_secret_path(pid)
    root = path.parent
    if not current:
        try:
            path.unlink(missing_ok=True)
        except Exception:
            pass
        return {}

    _ensure_dir(root, 0o700)
    atomic_write_json(path, current, indent=2)
    try:
        os.chmod(path, 0o600)
    except Exception:
        pass
    return dict(current)


def delete_actor_profile_secrets(profile_id: str) -> None:
    pid = validate_actor_profile_id(profile_id)
    path = _profile_secret_path(pid)
    try:
        path.unlink(missing_ok=True)
    except Exception:
        pass
