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
    seed = dict(seed or _pet_actor_seed(group))
    if current is None:
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
    return update_actor(
        group,
        PET_ACTOR_ID,
        {
            "title": seed["title"],
            "command": seed["command"],
            "env": seed["env"],
            "capability_autoload": seed["capability_autoload"],
            "default_scope_key": seed["default_scope_key"],
            "submit": seed["submit"],
            "enabled": seed["enabled"],
            "runner": seed["runner"],
            "runtime": seed["runtime"],
            "internal_kind": INTERNAL_KIND_PET,
        },
    )


def sync_pet_actor(group: Group) -> Optional[Dict[str, Any]]:
    if not is_desktop_pet_enabled(group):
        current = get_pet_actor(group)
        if current is not None:
            remove_actor(group, PET_ACTOR_ID)
        return None
    return ensure_pet_actor(group)
