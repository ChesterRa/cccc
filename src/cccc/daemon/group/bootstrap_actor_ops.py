"""Actor autostart bootstrap helpers."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...kernel.context import ContextStorage
from ...kernel.actors import list_actors
from ...kernel.group import load_group
from ...kernel.runtime import runtime_start_preflight_error
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ..runner_state_ops import web_model_group_running
from ...util.conv import coerce_bool
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ..actors.actor_runtime_ops import model_from_runtime_command, resolve_actor_launch_spec
from ..assistants.voice_secretary_runtime_ops import (
    capture_voice_secretary_actor_state,
    restore_voice_secretary_actor_state,
    sync_voice_secretary_actor_from_foreman,
)
from ..pet.pet_runtime_ops import capture_pet_actor_state, restore_pet_actor_state, sync_pet_actor_from_foreman

logger = logging.getLogger("cccc.daemon.server")

ResolveLinkedActorBeforeStart = Callable[..., Dict[str, Any]]


def autostart_running_groups(
    home: Path,
    *,
    effective_runner_kind: Callable[[str], str],
    find_scope_url: Callable[[Any, str], str],
    supported_runtimes: tuple[str, ...],
    ensure_mcp_installed: Callable[..., bool],
    auto_mcp_runtimes: tuple[str, ...],
    pty_supported: Optional[Callable[[], bool]] = None,
    merge_actor_env_with_private: Callable[[str, str, dict[str, Any]], dict[str, Any]],
    inject_actor_context_env: Callable[[dict[str, Any], str, str], dict[str, Any]],
    prepare_pty_env: Callable[[dict[str, Any]], dict[str, str]],
    normalize_runtime_command: Callable[[str, list[str]], list[str]],
    pty_backlog_bytes: Callable[[], int],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[[str, str, int], None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[[str, str], None],
    automation_on_resume: Callable[[Any], None],
    get_group_state: Callable[[Any], str],
    load_actor_private_env: Callable[[str, str], dict[str, str]],
    update_actor_private_env: Callable[..., dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
    resolve_linked_actor_before_start: Optional[ResolveLinkedActorBeforeStart] = None,
) -> None:
    base = home / "groups"
    if not base.exists():
        return

    for group_yaml in base.glob("*/group.yaml"):
        group_id = group_yaml.parent.name
        group = load_group(group_id)
        if group is None:
            continue
        if not coerce_bool(group.doc.get("running"), default=False):
            continue
        logger.info("autostart group=%s state=%s running=%s", group_id, str(group.doc.get("state") or "active"), True)

        group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
        if not group_scope_key:
            group.doc["running"] = False
            try:
                group.save()
            except Exception:
                pass
            continue

        pet_state_before = capture_pet_actor_state(group, load_actor_private_env=load_actor_private_env)
        try:
            sync_pet_actor_from_foreman(
                group,
                effective_runner_kind=effective_runner_kind,
                load_actor_private_env=load_actor_private_env,
                update_actor_private_env=update_actor_private_env,
                delete_actor_private_env=delete_actor_private_env,
                resolve_linked_actor_before_start=resolve_linked_actor_before_start,
            )
        except Exception as e:
            logger.warning("Pet actor sync failed for %s: %s", group_id, e)
            try:
                restore_pet_actor_state(
                    group,
                    None if str(e).strip() == "desktop pet requires a foreman actor" else pet_state_before,
                    update_actor_private_env=update_actor_private_env,
                    delete_actor_private_env=delete_actor_private_env,
                )
            except Exception:
                pass

        voice_state_before = capture_voice_secretary_actor_state(group, load_actor_private_env=load_actor_private_env)
        try:
            sync_voice_secretary_actor_from_foreman(
                group,
                effective_runner_kind=effective_runner_kind,
                load_actor_private_env=load_actor_private_env,
                update_actor_private_env=update_actor_private_env,
                delete_actor_private_env=delete_actor_private_env,
                resolve_linked_actor_before_start=resolve_linked_actor_before_start,
            )
        except Exception as e:
            logger.warning("Voice Secretary actor sync failed for %s: %s", group_id, e)
            try:
                restore_voice_secretary_actor_state(
                    group,
                    None if str(e).strip() == "voice secretary requires a foreman actor" else voice_state_before,
                    update_actor_private_env=update_actor_private_env,
                    delete_actor_private_env=delete_actor_private_env,
                )
            except Exception:
                pass

        for actor in list_actors(group):
            if not isinstance(actor, dict):
                continue
            actor_id = str(actor.get("id") or "").strip()
            if not actor_id:
                continue
            if not coerce_bool(actor.get("enabled"), default=True):
                continue

            profile_scope = str(actor.get("profile_scope") or "").strip().lower()
            profile_owner = str(actor.get("profile_owner") or "").strip()
            caller_id = profile_owner if profile_scope == "user" and profile_owner else ""
            try:
                launch_spec = resolve_actor_launch_spec(
                    group,
                    actor_id,
                    command=list(actor.get("command") or []) if isinstance(actor.get("command"), list) else [],
                    env=dict(actor.get("env") or {}) if isinstance(actor.get("env"), dict) else {},
                    runner=str(actor.get("runner") or "pty"),
                    runtime=str(actor.get("runtime") or "codex"),
                    find_scope_url=find_scope_url,
                    effective_runner_kind=effective_runner_kind,
                    normalize_runtime_command=normalize_runtime_command,
                    supported_runtimes=supported_runtimes,
                    caller_id=caller_id,
                    is_admin=False,
                    resolve_linked_actor_before_start=resolve_linked_actor_before_start,
                    merge_actor_env_with_private=merge_actor_env_with_private,
                )
            except Exception as e:
                logger.warning("Autostart skipped for %s/%s: %s", group_id, actor_id, e)
                continue

            effective_runner = str(launch_spec["effective_runner"])
            cwd = launch_spec["cwd"]
            runtime = str(launch_spec["runtime"])
            effective_env = dict(launch_spec["merged_env"])

            ok_mcp = True
            effective_cmd = list(launch_spec["effective_command"])
            runtime_error = runtime_start_preflight_error(runtime, effective_cmd, runner=effective_runner)
            if runtime_error:
                logger.warning("Autostart skipped for %s/%s: %s", group_id, actor_id, runtime_error)
                continue
            if effective_runner != "headless":
                try:
                    ok_mcp = bool(
                        ensure_mcp_installed(
                            runtime,
                            cwd,
                            env={str(k): str(v) for k, v in effective_env.items() if isinstance(k, str)},
                        )
                    )
                except Exception:
                    ok_mcp = False
            if not ok_mcp and runtime in auto_mcp_runtimes:
                logger.warning(
                    "MCP server 'cccc' is not installed for %s/%s (runtime=%s); actor will start but tools may not work.",
                    group_id,
                    actor_id,
                    runtime,
                )

            clear_preamble_sent(group, actor_id)

            try:
                logger.info(
                    "autostart start group=%s actor=%s runtime=%s runner=%s runner_effective=%s",
                    group.group_id,
                    actor_id,
                    runtime,
                    str(launch_spec["runner"]),
                    effective_runner,
                )
                if runtime == "web_model" and effective_runner == "headless":
                    write_headless_state(group.group_id, actor_id)
                elif runtime == "codex" and effective_runner == "headless":
                    codex_app_supervisor.start_actor(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        cwd=cwd,
                        env=dict(inject_actor_context_env(effective_env, group.group_id, actor_id)),
                        model=model_from_runtime_command(launch_spec["effective_command"]),
                    )
                elif runtime == "claude" and effective_runner == "headless":
                    claude_app_supervisor.start_actor(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        cwd=cwd,
                        env=dict(inject_actor_context_env(effective_env, group.group_id, actor_id)),
                        model=model_from_runtime_command(launch_spec["effective_command"]),
                    )
                elif effective_runner == "headless":
                    headless_runner.SUPERVISOR.start_actor(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        cwd=cwd,
                        env=dict(inject_actor_context_env(effective_env, group.group_id, actor_id)),
                    )
                else:
                    session = pty_runner.SUPERVISOR.start_actor(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        cwd=cwd,
                        command=effective_cmd,
                        env=prepare_pty_env(inject_actor_context_env(effective_env, group.group_id, actor_id)),
                        runtime=runtime,
                        max_backlog_bytes=pty_backlog_bytes(),
                    )
                logger.info(
                    "autostart started group=%s actor=%s runtime=%s runner_effective=%s",
                    group.group_id,
                    actor_id,
                    runtime,
                    effective_runner,
                )
            except Exception as e:
                logger.warning("Autostart failed for %s/%s: %s", group_id, actor_id, e)
                continue

            try:
                if runtime == "web_model" and effective_runner == "headless":
                    pass
                elif runtime == "codex" and effective_runner == "headless":
                    pass
                elif runtime == "claude" and effective_runner == "headless":
                    pass
                elif effective_runner == "headless":
                    write_headless_state(group.group_id, actor_id)
                else:
                    write_pty_state(group.group_id, actor_id, session.pid)
            except Exception as e:
                logger.debug("State write failed for %s/%s: %s", group_id, actor_id, e)

            clear_preamble_sent(group, actor_id)
            throttle_reset_actor(group.group_id, actor_id)
            try:
                ContextStorage(group).clear_agent_status_if_present(actor_id)
            except Exception:
                pass

        try:
            if (
                get_group_state(group) == "active"
                and (
                    codex_app_supervisor.group_running(group.group_id)
                    or
                    claude_app_supervisor.group_running(group.group_id)
                    or
                    pty_runner.SUPERVISOR.group_running(group.group_id)
                    or headless_runner.SUPERVISOR.group_running(group.group_id)
                    or web_model_group_running(group.group_id)
                )
            ):
                automation_on_resume(group)
        except Exception:
            pass
