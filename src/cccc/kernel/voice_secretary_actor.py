from __future__ import annotations

from typing import Any, Dict, Optional

from ..util.conv import coerce_bool
from .actors import INTERNAL_KIND_VOICE_SECRETARY, add_actor, find_actor, find_foreman, remove_actor, update_actor
from .group import Group
from .internal_assistant_runtime import normalize_internal_assistant_launch_seed
from .runtime import get_runtime_command_with_flags


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
    runtime, runner, command = normalize_internal_assistant_launch_seed(
        runtime=runtime,
        runner=runner,
        command=list(command),
    )
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
    if current is None:
        seed = dict(seed or _voice_secretary_actor_seed(group))
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
    seed = dict(seed or {})

    patch: Dict[str, Any] = {
        "title": str(current.get("title") or seed.get("title") or VOICE_SECRETARY_ACTOR_TITLE),
        "enabled": bool(seed.get("enabled", current.get("enabled", True))),
        "internal_kind": INTERNAL_KIND_VOICE_SECRETARY,
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
                "capability_autoload": list(seed.get("capability_autoload") or []),
                "submit": str(seed.get("submit") or "enter").strip() or "enter",
                "runner": str(seed.get("runner") or "pty"),
                "runtime": str(seed.get("runtime") or "codex"),
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
