"""Group lifecycle operation handlers for daemon."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any, Callable, Dict, Optional, Sequence

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import list_actors, update_actor
from ...kernel.context import ContextStorage
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_group_permission
from ...kernel.runtime import runtime_start_preflight_error
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool
from ..actors.actor_profile_runtime import resolve_linked_actor_before_start
from ..actors.actor_runtime_ops import model_from_runtime_command, resolve_actor_launch_spec
from ..assistants.voice_secretary_runtime_ops import (
    capture_voice_secretary_actor_state,
    restore_voice_secretary_actor_state,
    sync_voice_secretary_actor_from_foreman,
)
from ..pet.pet_runtime_ops import capture_pet_actor_state, restore_pet_actor_state, sync_pet_actor_from_foreman
from ..pet.review_scheduler import cancel_pet_review, request_pet_review
from ..pet.profile_refresh import maybe_request_pet_profile_refresh

logger = logging.getLogger(__name__)


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_group_start(
    args: Dict[str, Any],
    *,
    effective_runner_kind: Callable[[str], str],
    find_scope_url: Callable[[Any, str], str],
    ensure_mcp_installed: Callable[..., Any],
    merge_actor_env_with_private: Callable[[str, str, Dict[str, Any]], Dict[str, Any]],
    inject_actor_context_env: Callable[..., Dict[str, Any]],
    normalize_runtime_command: Callable[[str, list[str]], list[str]],
    prepare_pty_env: Callable[[Dict[str, Any]], Dict[str, Any]],
    pty_backlog_bytes: Callable[[], int],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[..., None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[..., None],
    reset_automation_timers_if_active: Callable[[Any], None],
    supported_runtimes: Sequence[str],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if not group_scope_key:
        return _error(
            "missing_project_root",
            "missing project root for group (no active scope)",
            details={"hint": "Attach a project root first (e.g. cccc attach <path> --group <id>)"},
        )
    try:
        require_group_permission(group, by=by, action="group.start")
        resolve_before_start = lambda grp, aid, caller_id="", is_admin=False: resolve_linked_actor_before_start(
            grp,
            aid,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            update_actor_private_env=update_actor_private_env,
            caller_id=caller_id,
            is_admin=is_admin,
        )
        pet_state_before = capture_pet_actor_state(group, load_actor_private_env=load_actor_private_env)
        try:
            sync_pet_actor_from_foreman(
                group,
                effective_runner_kind=effective_runner_kind,
                load_actor_private_env=load_actor_private_env,
                update_actor_private_env=update_actor_private_env,
                delete_actor_private_env=delete_actor_private_env,
                resolve_linked_actor_before_start=resolve_before_start,
                caller_id=str(args.get("caller_id") or "").strip(),
                is_admin=coerce_bool(args.get("is_admin"), default=False),
            )
        except Exception as e:
            logger.warning("Pet actor sync skipped for %s during group start: %s", group.group_id, e)
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
                resolve_linked_actor_before_start=resolve_before_start,
                caller_id=str(args.get("caller_id") or "").strip(),
                is_admin=coerce_bool(args.get("is_admin"), default=False),
            )
        except Exception as e:
            logger.warning("Voice Secretary actor sync skipped for %s during group start: %s", group.group_id, e)
            try:
                restore_voice_secretary_actor_state(
                    group,
                    None if str(e).strip() == "voice secretary requires a foreman actor" else voice_state_before,
                    update_actor_private_env=update_actor_private_env,
                    delete_actor_private_env=delete_actor_private_env,
                )
            except Exception:
                pass
        actors = list_actors(group)
        start_specs: list[Dict[str, Any]] = []
        for actor in actors:
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid:
                continue

            try:
                launch_spec = resolve_actor_launch_spec(
                    group,
                    aid,
                    command=list(actor.get("command") or []) if isinstance(actor.get("command"), list) else [],
                    env=dict(actor.get("env") or {}) if isinstance(actor.get("env"), dict) else {},
                    runner=str(actor.get("runner") or "pty"),
                    runtime=str(actor.get("runtime") or "codex"),
                    find_scope_url=find_scope_url,
                    effective_runner_kind=effective_runner_kind,
                    normalize_runtime_command=normalize_runtime_command,
                    supported_runtimes=list(supported_runtimes),
                    caller_id=str(args.get("caller_id") or "").strip(),
                    is_admin=coerce_bool(args.get("is_admin"), default=False),
                    resolve_linked_actor_before_start=resolve_before_start,
                    merge_actor_env_with_private=merge_actor_env_with_private,
                )
            except ValueError as e:
                msg = str(e)
                actor_ref = {"group_id": group.group_id, "actor_id": aid}
                if msg.startswith("scope not attached:"):
                    scope_key = msg.partition(":")[2].strip()
                    return _error(
                        "scope_not_attached",
                        msg,
                        details={
                            **actor_ref,
                            "scope_key": scope_key,
                            "hint": "Attach this scope to the group (cccc attach <path> --group <id>)",
                        },
                    )
                if msg.startswith("project root path does not exist:"):
                    return _error(
                        "invalid_project_root",
                        "project root path does not exist",
                        details={
                            **actor_ref,
                            "scope_key": str(actor.get("default_scope_key") or group_scope_key).strip(),
                            "path": msg.partition(":")[2].strip(),
                            "hint": "Re-attach a valid project root (cccc attach <path> --group <id>)",
                        },
                    )
                if msg.startswith("unsupported runtime:"):
                    runtime = msg.partition(":")[2].strip()
                    return _error(
                        "unsupported_runtime",
                        msg,
                        details={
                            **actor_ref,
                            "runtime": runtime,
                            "supported": list(supported_runtimes),
                            "hint": "Change the actor runtime to a supported one.",
                        },
                    )
                if msg == "custom runtime requires a command (PTY runner)":
                    return _error(
                        "missing_command",
                        msg,
                        details={
                            **actor_ref,
                            "runtime": str(actor.get("runtime") or "codex").strip() or "codex",
                            "hint": "Set actor.command (or switch runner to headless).",
                        },
                    )
                raise
            start_specs.append(launch_spec)

        started: list[str] = []
        for launch_spec in start_specs:
            aid = str(((launch_spec.get("actor") or {}).get("id") or "")).strip()
            cwd = launch_spec["cwd"]
            runner_kind = str(launch_spec["runner"])
            runtime = str(launch_spec["runtime"])
            runner_effective = str(launch_spec["effective_runner"])
            update_actor(group, aid, {"enabled": True})
            effective_env = dict(launch_spec["merged_env"])
            if runner_effective != "headless":
                try:
                    mcp_ready = bool(
                        ensure_mcp_installed(
                            runtime,
                            cwd,
                            env={str(k): str(v) for k, v in effective_env.items() if isinstance(k, str)},
                        )
                    )
                except Exception as e:
                    raise RuntimeError(f"failed to install MCP for actor {aid}: {e}") from e
                if not mcp_ready:
                    raise RuntimeError(f"failed to install MCP for actor {aid} (runtime={runtime})")
                runtime_error = runtime_start_preflight_error(runtime, launch_spec["effective_command"], runner=runner_effective)
                if runtime_error:
                    return _error(
                        "runtime_unavailable",
                        runtime_error,
                        details={
                            "group_id": group.group_id,
                            "actor_id": aid,
                            "runtime": runtime,
                        },
                    )

            if runtime == "web_model" and runner_effective == "headless":
                try:
                    write_headless_state(group.group_id, aid)
                except Exception:
                    pass
            elif runtime == "codex" and runner_effective == "headless":
                codex_app_supervisor.start_actor(
                    group_id=group.group_id,
                    actor_id=aid,
                    cwd=cwd,
                    env=dict(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=aid)),
                    model=model_from_runtime_command(launch_spec["effective_command"]),
                )
            elif runtime == "claude" and runner_effective == "headless":
                claude_app_supervisor.start_actor(
                    group_id=group.group_id,
                    actor_id=aid,
                    cwd=cwd,
                    env=dict(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=aid)),
                    model=model_from_runtime_command(launch_spec["effective_command"]),
                )
            elif runner_effective == "headless":
                headless_runner.SUPERVISOR.start_actor(
                    group_id=group.group_id,
                    actor_id=aid,
                    cwd=cwd,
                    env=dict(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=aid)),
                )
                try:
                    write_headless_state(group.group_id, aid)
                except Exception:
                    pass
            else:
                session = pty_runner.SUPERVISOR.start_actor(
                    group_id=group.group_id,
                    actor_id=aid,
                    cwd=cwd,
                    command=launch_spec["effective_command"],
                    env=prepare_pty_env(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=aid)),
                    runtime=runtime,
                    max_backlog_bytes=pty_backlog_bytes(),
                )
                try:
                    write_pty_state(group.group_id, aid, pid=session.pid)
                except Exception:
                    pass

            clear_preamble_sent(group, aid)
            throttle_reset_actor(group.group_id, aid, keep_pending=True)
            try:
                ContextStorage(group).clear_agent_status_if_present(aid)
            except Exception:
                pass
            started.append(aid)
    except Exception as e:
        msg = str(e)
        if "profile not found:" in msg:
            return _error("profile_not_found", msg)
        return _error("group_start_failed", msg)

    if started:
        try:
            if str(group.doc.get("state") or "").strip() == "stopped":
                group.doc["state"] = "active"
            group.doc["running"] = True
            group.save()
        except Exception:
            pass
        reset_automation_timers_if_active(group)

    data: Dict[str, Any] = {"started": started}
    event = append_event(group.ledger_path, kind="group.start", group_id=group.group_id, scope_key="", by=by, data=data)
    try:
        request_pet_review(
            group.group_id,
            reason="group_start",
            source_event_id=str(event.get("id") or "").strip(),
            immediate=True,
        )
    except Exception:
        pass
    try:
        maybe_request_pet_profile_refresh(
            group.group_id,
            source_event_id=str(event.get("id") or "").strip(),
            reason="group_start",
        )
    except Exception:
        pass
    result: Dict[str, Any] = {"group_id": group.group_id, "started": started, "event": event}
    return DaemonResponse(ok=True, result=result)


def handle_group_stop(
    args: Dict[str, Any],
    *,
    pty_state_dir_for_group: Callable[[str], Path],
    headless_state_dir_for_group: Callable[[str], Path],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.stop")
        actors = list_actors(group)
        stopped: list[str] = []
        for actor in actors:
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid:
                continue
            try:
                update_actor(group, aid, {"enabled": False})
                stopped.append(aid)
            except Exception:
                pass

        pty_runner.SUPERVISOR.stop_group(group_id=group.group_id)
        headless_runner.SUPERVISOR.stop_group(group_id=group.group_id)
        codex_app_supervisor.stop_group(group_id=group.group_id)
        claude_app_supervisor.stop_group(group_id=group.group_id)

        try:
            pdir = pty_state_dir_for_group(group.group_id)
            for file_path in pdir.glob("*.json"):
                try:
                    file_path.unlink()
                except Exception:
                    pass
        except Exception:
            pass
        try:
            hdir = headless_state_dir_for_group(group.group_id)
            for file_path in hdir.glob("*.json"):
                try:
                    file_path.unlink()
                except Exception:
                    pass
        except Exception:
            pass
    except Exception as e:
        return _error("group_stop_failed", str(e))
    try:
        group.doc["state"] = "stopped"
        group.doc["running"] = False
        group.save()
    except Exception:
        pass
    cancel_pet_review(group.group_id)
    event = append_event(group.ledger_path, kind="group.stop", group_id=group.group_id, scope_key="", by=by, data={"stopped": stopped})
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "stopped": stopped, "event": event})


def try_handle_group_lifecycle_op(
    op: str,
    args: Dict[str, Any],
    *,
    effective_runner_kind: Callable[[str], str],
    find_scope_url: Callable[[Any, str], str],
    ensure_mcp_installed: Callable[..., Any],
    merge_actor_env_with_private: Callable[[str, str, Dict[str, Any]], Dict[str, Any]],
    inject_actor_context_env: Callable[..., Dict[str, Any]],
    normalize_runtime_command: Callable[[str, list[str]], list[str]],
    prepare_pty_env: Callable[[Dict[str, Any]], Dict[str, Any]],
    pty_backlog_bytes: Callable[[], int],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[..., None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[..., None],
    reset_automation_timers_if_active: Callable[[Any], None],
    supported_runtimes: Sequence[str],
    pty_state_dir_for_group: Callable[[str], Path],
    headless_state_dir_for_group: Callable[[str], Path],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
) -> Optional[DaemonResponse]:
    if op == "group_start":
        return handle_group_start(
            args,
            effective_runner_kind=effective_runner_kind,
            find_scope_url=find_scope_url,
            ensure_mcp_installed=ensure_mcp_installed,
            merge_actor_env_with_private=merge_actor_env_with_private,
            inject_actor_context_env=inject_actor_context_env,
            normalize_runtime_command=normalize_runtime_command,
            prepare_pty_env=prepare_pty_env,
            pty_backlog_bytes=pty_backlog_bytes,
            write_headless_state=write_headless_state,
            write_pty_state=write_pty_state,
            clear_preamble_sent=clear_preamble_sent,
            throttle_reset_actor=throttle_reset_actor,
            reset_automation_timers_if_active=reset_automation_timers_if_active,
            supported_runtimes=supported_runtimes,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            load_actor_private_env=load_actor_private_env,
            update_actor_private_env=update_actor_private_env,
            delete_actor_private_env=delete_actor_private_env,
        )
    if op == "group_stop":
        return handle_group_stop(
            args,
            pty_state_dir_for_group=pty_state_dir_for_group,
            headless_state_dir_for_group=headless_state_dir_for_group,
        )
    return None
