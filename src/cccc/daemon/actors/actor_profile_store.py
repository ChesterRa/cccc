from __future__ import annotations

import hashlib
import json
import os
import re
import secrets
import shlex
from pathlib import Path
from typing import Any, Dict, List, Literal, Optional

from ...contracts.v1 import ActorProfile, ActorProfileRef
from ...kernel.runtime import get_runtime_command_with_flags
from ...paths import ensure_home
from ...util.fs import atomic_write_json, read_json
from ...util.time import utc_now_iso

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


def validate_actor_profile_scope(scope: Any) -> Literal["global", "user"]:
    raw = str(scope or "global").strip().lower() or "global"
    if raw not in {"global", "user"}:
        raise ValueError("invalid profile scope")
    return raw  # type: ignore[return-value]


def normalize_actor_profile_owner(scope: Any, owner_id: Any) -> str:
    profile_scope = validate_actor_profile_scope(scope)
    owner = str(owner_id or "").strip()
    if profile_scope == "global":
        return ""
    if not owner:
        raise ValueError("user scope profile requires owner_id")
    return owner


def normalize_actor_profile_ref(ref: ActorProfileRef | Dict[str, Any] | str) -> ActorProfileRef:
    if isinstance(ref, ActorProfileRef):
        profile_id = validate_actor_profile_id(ref.profile_id)
        profile_scope = validate_actor_profile_scope(ref.profile_scope)
        profile_owner = normalize_actor_profile_owner(profile_scope, ref.profile_owner)
        return ActorProfileRef(profile_id=profile_id, profile_scope=profile_scope, profile_owner=profile_owner)
    if isinstance(ref, str):
        return ActorProfileRef(profile_id=validate_actor_profile_id(ref))
    if not isinstance(ref, dict):
        raise ValueError("invalid profile ref")
    profile_id = validate_actor_profile_id(ref.get("profile_id") or ref.get("id"))
    profile_scope = validate_actor_profile_scope(ref.get("profile_scope") or ref.get("scope") or "global")
    profile_owner = normalize_actor_profile_owner(profile_scope, ref.get("profile_owner") or ref.get("owner_id") or "")
    return ActorProfileRef(profile_id=profile_id, profile_scope=profile_scope, profile_owner=profile_owner)


def _new_profile_id(raw_profiles: Dict[str, Any], *, scope: str, owner_id: str) -> str:
    while True:
        pid = f"ap_{secrets.token_hex(6)}"
        _, existing = _find_profile_entry(raw_profiles, pid, scope=scope, owner_id=owner_id)
        if existing is None:
            return pid


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


def _profile_storage_key(profile_id: str, *, scope: str, owner_id: str) -> str:
    return json.dumps(
        [validate_actor_profile_scope(scope), normalize_actor_profile_owner(scope, owner_id), validate_actor_profile_id(profile_id)],
        separators=(",", ":"),
    )


def _parse_profile_storage_key(storage_key: str) -> tuple[Optional[str], Optional[str], Optional[str]]:
    raw = str(storage_key or "").strip()
    if not raw.startswith("["):
        return None, None, None
    try:
        parsed = json.loads(raw)
    except Exception:
        return None, None, None
    if not isinstance(parsed, list) or len(parsed) != 3:
        return None, None, None
    try:
        scope = validate_actor_profile_scope(parsed[0])
        owner_id = normalize_actor_profile_owner(scope, parsed[1])
        profile_id = validate_actor_profile_id(parsed[2])
    except Exception:
        return None, None, None
    return scope, owner_id, profile_id


def _model_from_raw_profile(raw: Any, *, storage_key: str) -> Optional[ActorProfile]:
    if not isinstance(raw, dict):
        return None
    key_scope, key_owner, key_profile_id = _parse_profile_storage_key(storage_key)
    payload = dict(raw)
    if key_profile_id:
        payload.setdefault("id", key_profile_id)
        payload.setdefault("scope", key_scope)
        payload.setdefault("owner_id", key_owner)
    else:
        payload.setdefault("id", storage_key)
    payload.setdefault("scope", "global")
    payload.setdefault("owner_id", "")
    try:
        return ActorProfile.model_validate(payload)
    except Exception:
        return None


def _iter_profiles(raw_profiles: Dict[str, Any]) -> List[tuple[str, ActorProfile]]:
    items: List[tuple[str, ActorProfile]] = []
    for storage_key, raw in raw_profiles.items():
        model = _model_from_raw_profile(raw, storage_key=str(storage_key))
        if model is None:
            continue
        items.append((str(storage_key), model))
    return items


def _find_profile_entry(
    raw_profiles: Dict[str, Any],
    profile_id: str,
    *,
    scope: str,
    owner_id: str,
) -> tuple[str, Optional[ActorProfile]]:
    pid = validate_actor_profile_id(profile_id)
    profile_scope = validate_actor_profile_scope(scope)
    profile_owner = normalize_actor_profile_owner(profile_scope, owner_id)
    composite_key = _profile_storage_key(pid, scope=profile_scope, owner_id=profile_owner)
    direct = _model_from_raw_profile(raw_profiles.get(composite_key), storage_key=composite_key)
    if direct is not None:
        return composite_key, direct
    if profile_scope == "global":
        legacy = _model_from_raw_profile(raw_profiles.get(pid), storage_key=pid)
        if legacy is not None and legacy.scope == "global":
            return pid, legacy
    for storage_key, model in _iter_profiles(raw_profiles):
        if model.id == pid and model.scope == profile_scope and model.owner_id == profile_owner:
            return storage_key, model
    return composite_key, None


def _sort_profile_models(items: List[ActorProfile]) -> List[ActorProfile]:
    return sorted(items, key=lambda item: (str(item.name or item.id).casefold(), item.scope, item.owner_id, item.id))


def _save_actor_profile(
    profile: Dict[str, Any],
    *,
    expected_revision: Optional[int] = None,
) -> ActorProfile:
    if not isinstance(profile, dict):
        raise ValueError("profile must be an object")

    path, doc = _load_profiles_doc()
    raw_profiles = doc.get("profiles") if isinstance(doc.get("profiles"), dict) else {}
    doc["profiles"] = raw_profiles

    desired_scope = validate_actor_profile_scope(profile.get("scope") or "global")
    desired_owner = normalize_actor_profile_owner(desired_scope, profile.get("owner_id") or "")
    profile_id_raw = profile.get("id")
    pid = validate_actor_profile_id(profile_id_raw) if str(profile_id_raw or "").strip() else _new_profile_id(raw_profiles, scope=desired_scope, owner_id=desired_owner)

    existing_key, existing_model = _find_profile_entry(raw_profiles, pid, scope=desired_scope, owner_id=desired_owner)
    existing = existing_model.model_dump(exclude_none=True) if isinstance(existing_model, ActorProfile) else {}
    if expected_revision is not None:
        expected = int(expected_revision)
        current = int(existing.get("revision") or 0)
        if current != expected:
            raise ProfileRevisionMismatchError(f"profile revision mismatch (expected {expected}, got {current})")

    now = utc_now_iso()
    runtime = str(profile.get("runtime") if "runtime" in profile else existing.get("runtime") or "codex").strip() or "codex"
    runner = str(profile.get("runner") if "runner" in profile else existing.get("runner") or "pty").strip() or "pty"
    if runtime == "web_model":
        runner = "headless"
    submit = str(profile.get("submit") if "submit" in profile else existing.get("submit") or "enter").strip() or "enter"
    name = str(profile.get("name") if "name" in profile else existing.get("name") or "").strip()

    env_in = profile.get("env") if "env" in profile else existing.get("env")
    env: Dict[str, str] = {}
    if isinstance(env_in, dict):
        for key, value in env_in.items():
            if not isinstance(key, str):
                continue
            env[str(key)] = str(value)

    command_in = profile.get("command") if "command" in profile else existing.get("command")
    command = [] if runtime == "web_model" else _normalize_profile_command(runtime=runtime, runner=runner, command=command_in)

    capability_defaults_in = profile.get("capability_defaults") if "capability_defaults" in profile else existing.get("capability_defaults")
    capability_defaults: Optional[Dict[str, Any]] = None
    if capability_defaults_in is not None:
        if not isinstance(capability_defaults_in, dict):
            raise ValueError("capability_defaults must be an object or null")
        autoload_raw = capability_defaults_in.get("autoload_capabilities")
        autoload: List[str] = []
        seen: set[str] = set()
        if isinstance(autoload_raw, list):
            for item in autoload_raw:
                cap_id = str(item or "").strip()
                if not cap_id or cap_id in seen:
                    continue
                seen.add(cap_id)
                autoload.append(cap_id)
        default_scope = str(capability_defaults_in.get("default_scope") or "actor").strip().lower()
        if default_scope not in {"actor", "session"}:
            raise ValueError("capability_defaults.default_scope must be actor or session")
        try:
            ttl_seconds = int(capability_defaults_in.get("session_ttl_seconds") or 3600)
        except Exception:
            ttl_seconds = 3600
        ttl_seconds = max(60, min(ttl_seconds, 24 * 3600))
        capability_defaults = {
            "autoload_capabilities": autoload,
            "default_scope": default_scope,
            "session_ttl_seconds": ttl_seconds,
        }

    payload = {
        "id": pid,
        "name": name,
        "scope": desired_scope,
        "owner_id": desired_owner,
        "runtime": runtime,
        "runner": runner,
        "command": command,
        "submit": submit,
        "env": env,
        "created_at": str(existing.get("created_at") or now),
        "updated_at": now,
        "revision": int(existing.get("revision") or 0) + 1,
        "capability_defaults": capability_defaults,
    }
    model = ActorProfile.model_validate(payload)
    storage_key = _profile_storage_key(model.id, scope=model.scope, owner_id=model.owner_id)
    if existing_key and existing_key != storage_key:
        raw_profiles.pop(existing_key, None)
    # Store by composite key so duplicate ids can exist across scope/owner.
    raw_profiles[storage_key] = model.model_dump(exclude_none=True)
    _save_profiles_doc(path, doc)
    return model


def _delete_profile(ref: ActorProfileRef) -> bool:
    path, doc = _load_profiles_doc()
    raw_profiles = doc.get("profiles") if isinstance(doc.get("profiles"), dict) else {}
    doc["profiles"] = raw_profiles
    storage_key, model = _find_profile_entry(
        raw_profiles,
        ref.profile_id,
        scope=ref.profile_scope,
        owner_id=ref.profile_owner,
    )
    if model is None:
        return False
    raw_profiles.pop(storage_key, None)
    if storage_key != ref.profile_id:
        raw_profiles.pop(ref.profile_id, None)
    _save_profiles_doc(path, doc)
    return True


class ProfileResolver:
    def resolve(self, ref: ActorProfileRef | Dict[str, Any] | str, caller_id: str, is_admin: bool) -> Optional[ActorProfile]:
        normalized = normalize_actor_profile_ref(ref)
        if normalized.profile_scope == "user" and not is_admin and normalized.profile_owner != str(caller_id or "").strip():
            return None
        _, doc = _load_profiles_doc()
        raw_profiles = doc.get("profiles") if isinstance(doc.get("profiles"), dict) else {}
        _, model = _find_profile_entry(
            raw_profiles,
            normalized.profile_id,
            scope=normalized.profile_scope,
            owner_id=normalized.profile_owner,
        )
        return model

    def list_profiles(self, view: str, caller_id: str, is_admin: bool) -> List[ActorProfile]:
        normalized_view = str(view or "").strip().lower()
        _, doc = _load_profiles_doc()
        raw_profiles = doc.get("profiles") if isinstance(doc.get("profiles"), dict) else {}
        profiles = [model for _, model in _iter_profiles(raw_profiles)]
        caller = str(caller_id or "").strip()
        if normalized_view == "global":
            return _sort_profile_models([item for item in profiles if item.scope == "global"])
        if normalized_view == "my":
            if not caller:
                return []
            return _sort_profile_models([item for item in profiles if item.scope == "user" and item.owner_id == caller])
        if normalized_view == "all":
            return _sort_profile_models(profiles) if is_admin else []
        raise ValueError("invalid profile view")

    def save_profile(
        self,
        profile: ActorProfile | Dict[str, Any],
        caller_id: str,
        is_admin: bool,
        *,
        expected_revision: Optional[int] = None,
    ) -> bool:
        payload = profile.model_dump(exclude_none=True) if isinstance(profile, ActorProfile) else dict(profile or {})
        scope = validate_actor_profile_scope(payload.get("scope") or "global")
        owner_id = normalize_actor_profile_owner(scope, payload.get("owner_id") or "")
        caller = str(caller_id or "").strip()
        if scope == "global":
            if not is_admin:
                return False
        elif not is_admin and owner_id != caller:
            return False
        saved = _save_actor_profile(payload, expected_revision=expected_revision)
        if isinstance(profile, dict):
            profile["id"] = saved.id
            profile["scope"] = saved.scope
            profile["owner_id"] = saved.owner_id
        return True

    def delete_profile(self, ref: ActorProfileRef | Dict[str, Any] | str, caller_id: str, is_admin: bool) -> bool:
        normalized = normalize_actor_profile_ref(ref)
        caller = str(caller_id or "").strip()
        if normalized.profile_scope == "global" and not is_admin:
            return False
        if normalized.profile_scope == "user" and not is_admin and normalized.profile_owner != caller:
            return False
        return _delete_profile(normalized)


def list_actor_profiles() -> List[Dict[str, Any]]:
    resolver = ProfileResolver()
    return [item.model_dump(exclude_none=True) for item in resolver.list_profiles("global", caller_id="", is_admin=True)]


def get_actor_profile(profile_id: str) -> Optional[Dict[str, Any]]:
    resolver = ProfileResolver()
    model = resolver.resolve(profile_id, caller_id="", is_admin=True)
    return model.model_dump(exclude_none=True) if isinstance(model, ActorProfile) else None


def get_actor_profile_by_ref(ref: ActorProfileRef | Dict[str, Any] | str) -> Optional[Dict[str, Any]]:
    normalized = normalize_actor_profile_ref(ref)
    _, doc = _load_profiles_doc()
    raw_profiles = doc.get("profiles") if isinstance(doc.get("profiles"), dict) else {}
    _, model = _find_profile_entry(
        raw_profiles,
        normalized.profile_id,
        scope=normalized.profile_scope,
        owner_id=normalized.profile_owner,
    )
    return model.model_dump(exclude_none=True) if isinstance(model, ActorProfile) else None


def upsert_actor_profile(
    profile: Dict[str, Any],
    *,
    expected_revision: Optional[int] = None,
) -> Dict[str, Any]:
    model = _save_actor_profile(profile, expected_revision=expected_revision)
    return model.model_dump(exclude_none=True)


def delete_actor_profile(profile_id: str) -> None:
    if not _delete_profile(normalize_actor_profile_ref(profile_id)):
        raise ValueError(f"profile not found: {profile_id}")


def _legacy_profile_secret_filename(profile_id: str) -> str:
    pid = validate_actor_profile_id(profile_id)
    digest = hashlib.sha256(pid.encode("utf-8")).hexdigest()[:16]
    slug = re.sub(r"[^A-Za-z0-9._-]+", "_", pid).strip("._-")
    if not slug:
        slug = "profile"
    slug = slug[:32]
    return f"{slug}.{digest}.json"


def _profile_secret_filename(ref: ActorProfileRef | Dict[str, Any] | str) -> str:
    normalized = normalize_actor_profile_ref(ref)
    if normalized.profile_scope == "global" and not normalized.profile_owner:
        return _legacy_profile_secret_filename(normalized.profile_id)
    storage_key = _profile_storage_key(
        normalized.profile_id,
        scope=normalized.profile_scope,
        owner_id=normalized.profile_owner,
    )
    digest = hashlib.sha256(storage_key.encode("utf-8")).hexdigest()[:16]
    slug_source = f"{normalized.profile_owner}__{normalized.profile_id}"
    slug = re.sub(r"[^A-Za-z0-9._-]+", "_", slug_source).strip("._-")
    if not slug:
        slug = "profile"
    slug = slug[:48]
    return f"{slug}.{digest}.json"


def _profile_secret_path(ref: ActorProfileRef | Dict[str, Any] | str) -> Path:
    home = ensure_home()
    root = _profile_secret_root(home)
    return root / _profile_secret_filename(ref)


def load_actor_profile_secrets(ref: ActorProfileRef | Dict[str, Any] | str) -> Dict[str, str]:
    normalized = normalize_actor_profile_ref(ref)
    path = _profile_secret_path(normalized)
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
    ref: ActorProfileRef | Dict[str, Any] | str,
    *,
    set_vars: Dict[str, str],
    unset_keys: List[str],
    clear: bool,
) -> Dict[str, str]:
    normalized = normalize_actor_profile_ref(ref)
    current = {} if clear else load_actor_profile_secrets(normalized)
    for key in unset_keys:
        current.pop(str(key), None)
    for key, value in set_vars.items():
        current[str(key)] = str(value)

    path = _profile_secret_path(normalized)
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


def delete_actor_profile_secrets(ref: ActorProfileRef | Dict[str, Any] | str) -> None:
    normalized = normalize_actor_profile_ref(ref)
    path = _profile_secret_path(normalized)
    try:
        path.unlink(missing_ok=True)
    except Exception:
        pass
