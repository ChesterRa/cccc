from __future__ import annotations

from typing import Any, Dict, Optional

from ..util.conv import coerce_bool
from .actors import INTERNAL_KIND_PET, add_actor, find_actor, find_foreman, remove_actor, update_actor
from .group import Group
from .internal_assistant_runtime import normalize_internal_assistant_launch_seed
from .runtime import get_runtime_command_with_flags

PET_ACTOR_ID = "pet-peer"
PET_ACTOR_TITLE = "Pet Peer"


def is_desktop_pet_enabled(group: Group) -> bool:
    features = group.doc.get("features") if isinstance(group.doc.get("features"), dict) else {}
    return coerce_bool(features.get("desktop_pet_enabled"), default=False)


def get_pet_actor(group: Group) -> Optional[Dict[str, Any]]:
    actor = find_actor(group, PET_ACTOR_ID)
    if not isinstance(actor, dict):
        return None
    if str(actor.get("internal_kind") or "").strip() != INTERNAL_KIND_PET:
        return None
    return actor


def require_pet_foreman(group: Group) -> Dict[str, Any]:
    foreman = find_foreman(group)
    if not isinstance(foreman, dict):
        raise ValueError("desktop pet requires a foreman actor")
    return foreman


def build_pet_actor_seed(
    group: Group,
    *,
    runtime: str,
    runner: str,
    command: list[str],
    env: Dict[str, str],
    default_scope_key: str,
    submit: str,
) -> Dict[str, Any]:
    runtime, runner, command = normalize_internal_assistant_launch_seed(
        runtime=runtime,
        runner=runner,
        command=list(command),
    )
    return {
        "title": PET_ACTOR_TITLE,
        "runtime": runtime,
        "runner": runner,
        "command": list(command or get_runtime_command_with_flags(runtime)),
        "env": dict(env or {}),
        "capability_autoload": ["pack:pet"],
        "default_scope_key": str(default_scope_key or group.doc.get("active_scope_key") or "").strip(),
        "submit": str(submit or "enter").strip() or "enter",
        "enabled": True,
        "internal_kind": INTERNAL_KIND_PET,
    }


def _pet_actor_seed(group: Group) -> Dict[str, Any]:
    source = require_pet_foreman(group)
    command = source.get("command") if isinstance(source.get("command"), list) else []
    env = source.get("env") if isinstance(source.get("env"), dict) else {}
    return build_pet_actor_seed(
        group,
        runtime=str(source.get("runtime") or ""),
        runner=str(source.get("runner") or ""),
        command=list(command),
        env=dict(env),
        default_scope_key=str(source.get("default_scope_key") or ""),
        submit=str(source.get("submit") or "enter"),
    )


def ensure_pet_actor(group: Group, *, seed: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    current = get_pet_actor(group)
    if current is None:
        seed = dict(seed or _pet_actor_seed(group))
        return add_actor(
            group,
            actor_id=PET_ACTOR_ID,
            title=str(seed["title"]),
            command=list(seed["command"]),
            env=dict(seed["env"]),
            capability_autoload=list(seed["capability_autoload"]),
            default_scope_key=str(seed["default_scope_key"]),
            submit=str(seed["submit"]),
            enabled=bool(seed["enabled"]),
            runner=str(seed["runner"]),  # type: ignore[arg-type]
            runtime=str(seed["runtime"]),  # type: ignore[arg-type]
            internal_kind=INTERNAL_KIND_PET,
        )
    seed = dict(seed or {})
    capability_autoload = (
        list(current.get("capability_autoload") or [])
        if isinstance(current.get("capability_autoload"), list)
        else list(seed.get("capability_autoload") or [])
    )
    if "pack:pet" not in capability_autoload:
        capability_autoload.append("pack:pet")
    patch: Dict[str, Any] = {
        "title": str(current.get("title") or seed.get("title") or PET_ACTOR_TITLE),
        "capability_autoload": capability_autoload,
        "enabled": bool(seed.get("enabled", current.get("enabled", True))),
        "internal_kind": INTERNAL_KIND_PET,
    }
    if not str(current.get("default_scope_key") or "").strip() and str(seed.get("default_scope_key") or "").strip():
        patch["default_scope_key"] = str(seed.get("default_scope_key") or "").strip()
    current_runtime = str(current.get("runtime") or "").strip()
    missing_launch = (
        not current_runtime
        or not str(current.get("runner") or "").strip()
        or current_runtime.lower() == "web_model"
    )
    if missing_launch and seed:
        patch.update(
            {
                "command": list(seed.get("command") or []),
                "env": dict(seed.get("env") or {}),
                "submit": str(seed.get("submit") or "enter").strip() or "enter",
                "runner": str(seed.get("runner") or "pty"),
                "runtime": str(seed.get("runtime") or "codex"),
            }
        )
    return update_actor(
        group,
        PET_ACTOR_ID,
        patch,
    )


def sync_pet_actor(group: Group) -> Optional[Dict[str, Any]]:
    if not is_desktop_pet_enabled(group):
        current = get_pet_actor(group)
        if current is not None:
            remove_actor(group, PET_ACTOR_ID)
        return None
    return ensure_pet_actor(group)
