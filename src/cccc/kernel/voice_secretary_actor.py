from __future__ import annotations

from typing import Any, Dict, Optional

from ..util.conv import coerce_bool
from .actors import INTERNAL_KIND_VOICE_SECRETARY, add_actor, find_actor, find_foreman, remove_actor, update_actor
from .group import Group
from .runtime import PRIMARY_RUNTIMES, detect_runtime, get_runtime_command_with_flags, runtime_start_preflight_error


VOICE_SECRETARY_ACTOR_ID = "voice-secretary"
VOICE_SECRETARY_ACTOR_TITLE = "Voice Secretary"
VOICE_SECRETARY_ASSISTANT_ID = "voice_secretary"


def is_voice_secretary_enabled(group: Group) -> bool:
    assistants = group.doc.get("assistants") if isinstance(group.doc.get("assistants"), dict) else {}
    entry = assistants.get(VOICE_SECRETARY_ASSISTANT_ID) if isinstance(assistants.get(VOICE_SECRETARY_ASSISTANT_ID), dict) else {}
    return coerce_bool(entry.get("enabled"), default=False)


def get_voice_secretary_actor(group: Group) -> Optional[Dict[str, Any]]:
    actor = find_actor(group, VOICE_SECRETARY_ACTOR_ID)
    if not isinstance(actor, dict):
        return None
    if str(actor.get("internal_kind") or "").strip() != INTERNAL_KIND_VOICE_SECRETARY:
        return None
    return actor


def require_voice_secretary_foreman(group: Group) -> Dict[str, Any]:
    foreman = find_foreman(group)
    if not isinstance(foreman, dict):
        raise ValueError("voice secretary requires a foreman actor")
    return foreman


def build_voice_secretary_actor_seed(
    group: Group,
    *,
    runtime: str,
    runner: str,
    command: list[str],
    env: Dict[str, str],
    default_scope_key: str,
    submit: str,
) -> Dict[str, Any]:
    runtime_value = str(runtime or "").strip()
    runner_value = str(runner or "").strip()
    runner = runner_value if runner_value else "pty"
    runtime = runtime_value if runtime_value else "codex"
    if runtime.lower() == "web_model":
        runtime = "codex"
        runner = "headless"
        command = get_runtime_command_with_flags(runtime)
    if runtime_start_preflight_error(runtime, list(command), runner=runner):
        for candidate in PRIMARY_RUNTIMES:
            if not detect_runtime(candidate).available:
                continue
            runtime = candidate
            command = get_runtime_command_with_flags(candidate)
            break
    return {
        "title": VOICE_SECRETARY_ACTOR_TITLE,
        "runtime": runtime,
        "runner": runner,
        "command": list(command or get_runtime_command_with_flags(runtime)),
        "env": dict(env or {}),
        "capability_autoload": [],
        "default_scope_key": str(default_scope_key or group.doc.get("active_scope_key") or "").strip(),
        "submit": str(submit or "enter").strip() or "enter",
        "enabled": True,
        "internal_kind": INTERNAL_KIND_VOICE_SECRETARY,
    }


def _voice_secretary_actor_seed(group: Group) -> Dict[str, Any]:
    source = require_voice_secretary_foreman(group)
    command = source.get("command") if isinstance(source.get("command"), list) else []
    env = source.get("env") if isinstance(source.get("env"), dict) else {}
    return build_voice_secretary_actor_seed(
        group,
        runtime=str(source.get("runtime") or ""),
        runner=str(source.get("runner") or ""),
        command=list(command),
        env=dict(env),
        default_scope_key=str(source.get("default_scope_key") or ""),
        submit=str(source.get("submit") or "enter"),
    )


def ensure_voice_secretary_actor(group: Group, *, seed: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    current = get_voice_secretary_actor(group)
    seed = dict(seed or _voice_secretary_actor_seed(group))
    if current is None:
        return add_actor(
            group,
            actor_id=VOICE_SECRETARY_ACTOR_ID,
            title=str(seed["title"]),
            command=list(seed["command"]),
            env=dict(seed["env"]),
            capability_autoload=list(seed["capability_autoload"]),
            default_scope_key=str(seed["default_scope_key"]),
            submit=str(seed["submit"]),
            enabled=bool(seed["enabled"]),
            runner=str(seed["runner"]),  # type: ignore[arg-type]
            runtime=str(seed["runtime"]),  # type: ignore[arg-type]
            internal_kind=INTERNAL_KIND_VOICE_SECRETARY,
        )

    patch: Dict[str, Any] = {
        "title": seed["title"],
        "default_scope_key": seed["default_scope_key"],
        "enabled": seed["enabled"],
        "internal_kind": INTERNAL_KIND_VOICE_SECRETARY,
    }
    # Actor profiles are runtime templates, not identity templates. When the
    # Voice Secretary is linked to a profile, preserve profile-controlled
    # launch fields while still enforcing its internal actor identity.
    if not str(current.get("profile_id") or "").strip():
        patch.update(
            {
                "command": seed["command"],
                "env": seed["env"],
                "capability_autoload": seed["capability_autoload"],
                "submit": seed["submit"],
                "runner": seed["runner"],
                "runtime": seed["runtime"],
            }
        )
    update_actor(group, VOICE_SECRETARY_ACTOR_ID, patch)
    actor = get_voice_secretary_actor(group)
    if isinstance(actor, dict):
        return actor
    raise ValueError("failed to sync voice-secretary actor")


def sync_voice_secretary_actor(group: Group) -> Optional[Dict[str, Any]]:
    if not is_voice_secretary_enabled(group):
        current = get_voice_secretary_actor(group)
        if current is not None:
            remove_actor(group, VOICE_SECRETARY_ACTOR_ID)
        return None
    return ensure_voice_secretary_actor(group)
