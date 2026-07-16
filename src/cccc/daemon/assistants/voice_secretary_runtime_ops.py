from __future__ import annotations

from copy import deepcopy
from typing import Any, Callable, Dict, Optional

from ...kernel.actors import INTERNAL_KIND_VOICE_SECRETARY, add_actor, find_actor, remove_actor, update_actor
from ...kernel.group import Group
from ...kernel.ledger import append_event
from ...kernel.voice_secretary_actor import (
    VOICE_SECRETARY_ACTOR_ID,
    VOICE_SECRETARY_ACTOR_TITLE,
    build_voice_secretary_actor_seed,
    ensure_voice_secretary_actor,
    get_voice_secretary_actor,
    is_voice_secretary_enabled,
    require_voice_secretary_foreman,
)
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ..actors.actor_runtime_ops import resolve_actor_launch_config
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor


ResolveLinkedActorBeforeStart = Callable[..., Dict[str, Any]]


def voice_secretary_runtime_signature(
    actor: Optional[Dict[str, Any]],
    *,
    private_env: Optional[Dict[str, str]] = None,
) -> Dict[str, Any]:
    if not isinstance(actor, dict):
        return {}
    signature = {
        "runtime": str(actor.get("runtime") or "").strip(),
        "runner": str(actor.get("runner") or "").strip(),
        "command": list(actor.get("command") or []) if isinstance(actor.get("command"), list) else [],
        "env": dict(actor.get("env") or {}) if isinstance(actor.get("env"), dict) else {},
        "default_scope_key": str(actor.get("default_scope_key") or "").strip(),
        "submit": str(actor.get("submit") or "").strip(),
        "enabled": bool(actor.get("enabled")),
    }
    if private_env is not None:
        signature["env_private"] = dict(private_env)
    return signature


def voice_secretary_runtime_changed(
    before: Optional[Dict[str, Any]],
    after: Optional[Dict[str, Any]],
    *,
    before_private_env: Optional[Dict[str, str]] = None,
    after_private_env: Optional[Dict[str, str]] = None,
) -> bool:
    return voice_secretary_runtime_signature(before, private_env=before_private_env) != voice_secretary_runtime_signature(
        after,
        private_env=after_private_env,
    )


def capture_voice_secretary_actor_state(
    group: Group,
    *,
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
) -> Dict[str, Any]:
    actor = get_voice_secretary_actor(group)
    actor_doc = deepcopy(actor) if isinstance(actor, dict) else None
    private_env = load_actor_private_env(group.group_id, VOICE_SECRETARY_ACTOR_ID) if actor_doc is not None else {}
    return {
        "actor_doc": actor_doc,
        "private_env": dict(private_env),
    }


def is_voice_secretary_actor_running(
    group: Group,
    *,
    actor: Optional[Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
) -> bool:
    if not isinstance(actor, dict):
        return False
    runner_kind = str(actor.get("runner") or "pty").strip()
    runtime = str(actor.get("runtime") or "codex").strip() or "codex"
    if effective_runner_kind(runner_kind) == "headless":
        if runtime == "codex":
            return bool(codex_app_supervisor.actor_running(group.group_id, VOICE_SECRETARY_ACTOR_ID))
        if runtime == "claude":
            return bool(claude_app_supervisor.actor_running(group.group_id, VOICE_SECRETARY_ACTOR_ID))
        return bool(headless_runner.SUPERVISOR.actor_running(group.group_id, VOICE_SECRETARY_ACTOR_ID))
    return bool(pty_runner.SUPERVISOR.actor_running(group.group_id, VOICE_SECRETARY_ACTOR_ID))


def stop_voice_secretary_actor_runtime(
    group: Group,
    *,
    actor: Optional[Dict[str, Any]],
    by: str,
    effective_runner_kind: Callable[[str], str],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    emit_event: bool = True,
) -> None:
    if not isinstance(actor, dict):
        return
    runner_kind = str(actor.get("runner") or "pty").strip()
    runtime = str(actor.get("runtime") or "codex").strip() or "codex"
    if effective_runner_kind(runner_kind) == "headless":
        if runtime == "codex":
            codex_app_supervisor.stop_actor(group_id=group.group_id, actor_id=VOICE_SECRETARY_ACTOR_ID)
        elif runtime == "claude":
            claude_app_supervisor.stop_actor(group_id=group.group_id, actor_id=VOICE_SECRETARY_ACTOR_ID)
        else:
            headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=VOICE_SECRETARY_ACTOR_ID)
        remove_headless_state(group.group_id, VOICE_SECRETARY_ACTOR_ID)
        remove_pty_state_if_pid(group.group_id, VOICE_SECRETARY_ACTOR_ID, pid=0)
    else:
        pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=VOICE_SECRETARY_ACTOR_ID)
        remove_pty_state_if_pid(group.group_id, VOICE_SECRETARY_ACTOR_ID, pid=0)
        remove_headless_state(group.group_id, VOICE_SECRETARY_ACTOR_ID)
    if not emit_event:
        return
    event = append_event(
        group.ledger_path,
        kind="actor.stop",
        group_id=group.group_id,
        scope_key="",
        by=str(by or "system").strip() or "system",
        data={"actor_id": VOICE_SECRETARY_ACTOR_ID},
    )
    try:
        from ...kernel.events import publish_event

        publish_event(
            "actor.stop",
            {"group_id": group.group_id, "actor_id": VOICE_SECRETARY_ACTOR_ID, "event_id": str(event.get("id") or "")},
        )
    except Exception:
        pass


def restore_voice_secretary_actor_state(
    group: Group,
    state: Optional[Dict[str, Any]],
    *,
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
) -> Optional[Dict[str, Any]]:
    snapshot = state if isinstance(state, dict) else {}
    actor_doc = snapshot.get("actor_doc") if isinstance(snapshot.get("actor_doc"), dict) else None
    private_env = snapshot.get("private_env") if isinstance(snapshot.get("private_env"), dict) else {}
    restored = restore_voice_secretary_actor_doc(group, actor_doc)
    if isinstance(actor_doc, dict):
        if private_env:
            update_actor_private_env(
                group.group_id,
                VOICE_SECRETARY_ACTOR_ID,
                set_vars={str(key): str(value) for key, value in private_env.items() if isinstance(key, str) and isinstance(value, str)},
                unset_keys=[],
                clear=True,
            )
        else:
            delete_actor_private_env(group.group_id, VOICE_SECRETARY_ACTOR_ID)
        return restored
    delete_actor_private_env(group.group_id, VOICE_SECRETARY_ACTOR_ID)
    return None


def sync_voice_secretary_actor_from_foreman(
    group: Group,
    *,
    effective_runner_kind: Callable[[str], str],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
    resolve_linked_actor_before_start: Optional[ResolveLinkedActorBeforeStart] = None,
    caller_id: str = "",
    is_admin: bool = False,
) -> Optional[Dict[str, Any]]:
    if not is_voice_secretary_enabled(group):
        current = get_voice_secretary_actor(group)
        if current is not None:
            remove_actor(group, VOICE_SECRETARY_ACTOR_ID)
        delete_actor_private_env(group.group_id, VOICE_SECRETARY_ACTOR_ID)
        return None

    source = require_voice_secretary_foreman(group)
    source_id = str(source.get("id") or "").strip()
    if not source_id:
        raise ValueError("voice secretary requires a foreman actor")

    current = get_voice_secretary_actor(group)
    current_runtime = str((current or {}).get("runtime") or "").strip().lower() if isinstance(current, dict) else ""
    current_runner = str((current or {}).get("runner") or "").strip() if isinstance(current, dict) else ""
    if isinstance(current, dict) and current_runtime and current_runner and current_runtime != "web_model":
        return ensure_voice_secretary_actor(
            group,
            seed={
                "title": VOICE_SECRETARY_ACTOR_TITLE,
                "default_scope_key": str(group.doc.get("active_scope_key") or "").strip(),
                "enabled": True,
            },
        )

    resolved_caller_id = str(caller_id or "").strip()
    if not resolved_caller_id:
        profile_scope = str(source.get("profile_scope") or "").strip().lower()
        profile_owner = str(source.get("profile_owner") or "").strip()
        if profile_scope == "user" and profile_owner:
            resolved_caller_id = profile_owner

    launch_config = resolve_actor_launch_config(
        group,
        source_id,
        command=list(source.get("command") or []) if isinstance(source.get("command"), list) else [],
        env=dict(source.get("env") or {}) if isinstance(source.get("env"), dict) else {},
        runner=str(source.get("runner") or "pty"),
        runtime=str(source.get("runtime") or "codex"),
        effective_runner_kind=effective_runner_kind,
        caller_id=resolved_caller_id,
        is_admin=is_admin,
        resolve_linked_actor_before_start=resolve_linked_actor_before_start,
        load_actor_private_env=load_actor_private_env,
    )

    seed = build_voice_secretary_actor_seed(
        group,
        runtime=launch_config["runtime"],
        runner=launch_config["runner"],
        command=list(launch_config["command"] or []),
        env=dict(launch_config["public_env"] or {}),
        default_scope_key=launch_config["default_scope_key"],
        submit=launch_config["submit"],
    )
    actor = ensure_voice_secretary_actor(group, seed=seed)

    if launch_config["private_env"]:
        update_actor_private_env(
            group.group_id,
            VOICE_SECRETARY_ACTOR_ID,
            set_vars=dict(launch_config["private_env"]),
            unset_keys=[],
            clear=True,
        )
    else:
        delete_actor_private_env(group.group_id, VOICE_SECRETARY_ACTOR_ID)
    return actor


def restore_voice_secretary_actor_doc(group: Group, actor_doc: Optional[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    current = find_actor(group, VOICE_SECRETARY_ACTOR_ID)
    if not isinstance(actor_doc, dict):
        if isinstance(current, dict):
            remove_actor(group, VOICE_SECRETARY_ACTOR_ID)
        return None
    if not isinstance(current, dict):
        add_actor(
            group,
            actor_id=VOICE_SECRETARY_ACTOR_ID,
            title=str(actor_doc.get("title") or ""),
            command=list(actor_doc.get("command") or []) if isinstance(actor_doc.get("command"), list) else [],
            env=dict(actor_doc.get("env") or {}) if isinstance(actor_doc.get("env"), dict) else {},
            default_scope_key=str(actor_doc.get("default_scope_key") or ""),
            submit=str(actor_doc.get("submit") or "enter"),
            enabled=bool(actor_doc.get("enabled", True)),
            runner=str(actor_doc.get("runner") or "pty"),  # type: ignore[arg-type]
            runtime=str(actor_doc.get("runtime") or "codex"),  # type: ignore[arg-type]
            internal_kind=INTERNAL_KIND_VOICE_SECRETARY,
        )
    else:
        update_actor(
            group,
            VOICE_SECRETARY_ACTOR_ID,
            {
                "title": str(actor_doc.get("title") or ""),
                "command": list(actor_doc.get("command") or []) if isinstance(actor_doc.get("command"), list) else [],
                "env": dict(actor_doc.get("env") or {}) if isinstance(actor_doc.get("env"), dict) else {},
                "default_scope_key": str(actor_doc.get("default_scope_key") or ""),
                "submit": str(actor_doc.get("submit") or "enter"),
                "enabled": bool(actor_doc.get("enabled", True)),
                "runner": str(actor_doc.get("runner") or "pty"),
                "runtime": str(actor_doc.get("runtime") or "codex"),
                "internal_kind": INTERNAL_KIND_VOICE_SECRETARY,
            },
        )
    restored = find_actor(group, VOICE_SECRETARY_ACTOR_ID)
    return deepcopy(restored) if isinstance(restored, dict) else None
