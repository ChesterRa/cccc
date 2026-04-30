"""Actor lifecycle operation handlers for daemon."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Callable, Dict, Optional, Sequence

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor, is_internal_actor, list_actors, update_actor
from ...kernel.context import ContextStorage
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_actor_permission
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool
from .actor_runtime_ops import model_from_runtime_command, resolve_actor_launch_spec
from .actor_profile_runtime import ActorProfileAccessDeniedError, resolve_linked_actor_before_start
from ..pet.review_scheduler import request_pet_review


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_actor_start(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    start_actor_process: Callable[..., Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    before_foreman = foreman_id(group)
    caller_context_explicit = "caller_id" in args or "is_admin" in args
    caller_id = str(args.get("caller_id") or "").strip()
    is_admin = coerce_bool(args.get("is_admin"), default=not caller_context_explicit)
    try:
        require_actor_permission(group, by=by, action="actor.start", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, {"enabled": True})
        actor = resolve_linked_actor_before_start(
            group,
            actor_id,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            update_actor_private_env=update_actor_private_env,
            caller_id=caller_id,
            is_admin=is_admin,
        )
    except Exception as e:
        msg = str(e)
        if "profile not found:" in msg:
            return _error("profile_not_found", msg)
        if isinstance(e, ActorProfileAccessDeniedError):
            return _error("permission_denied", msg)
        return _error("actor_start_failed", msg)

    cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
    env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
    runner_kind = str(actor.get("runner") or "pty").strip()
    runtime = str(actor.get("runtime") or "codex").strip()
    start_result = start_actor_process(
        group,
        actor_id,
        command=list(cmd or []),
        env=dict(env or {}),
        runner=runner_kind,
        runtime=runtime,
        by=by,
        caller_id=caller_id,
        is_admin=is_admin,
    )
    if not start_result["success"]:
        return _error("actor_start_failed", start_result.get("error") or "unknown error")

    maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    result: Dict[str, Any] = {"actor": actor, "event": start_result["event"]}
    if start_result.get("effective_runner") != runner_kind:
        result["runner_effective"] = start_result.get("effective_runner")
    return DaemonResponse(ok=True, result=result)


def handle_actor_stop(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    effective_runner_kind: Callable[[str], str],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    before_foreman = foreman_id(group)
    try:
        require_actor_permission(group, by=by, action="actor.stop", target_actor_id=actor_id)
        current_actor = find_actor(group, actor_id)
        if isinstance(current_actor, dict) and is_internal_actor(current_actor):
            actor = dict(current_actor)
        else:
            actor = update_actor(group, actor_id, {"enabled": False})
        runner_kind = str(actor.get("runner") or "pty").strip()
        runner_effective = effective_runner_kind(runner_kind)
        runtime = str(actor.get("runtime") or "codex").strip() or "codex"
        if runtime == "web_model" and runner_effective == "headless":
            remove_headless_state(group.group_id, actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
        elif runtime == "codex" and runner_effective == "headless":
            codex_app_supervisor.stop_actor(group_id=group.group_id, actor_id=actor_id)
            remove_headless_state(group.group_id, actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
        elif runtime == "claude" and runner_effective == "headless":
            claude_app_supervisor.stop_actor(group_id=group.group_id, actor_id=actor_id)
            remove_headless_state(group.group_id, actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
        elif runner_effective == "headless":
            headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
            remove_headless_state(group.group_id, actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
        else:
            pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
            remove_headless_state(group.group_id, actor_id)
    except Exception as e:
        return _error("actor_stop_failed", str(e))

    try:
        any_enabled = any(
            coerce_bool(item.get("enabled"), default=True)
            for item in list_actors(group)
            if isinstance(item, dict) and str(item.get("id") or "").strip()
        )
        if not any_enabled:
            group.doc["running"] = False
            group.save()
    except Exception:
        pass

    maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    event = append_event(
        group.ledger_path,
        kind="actor.stop",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"actor_id": actor_id},
    )

    from ...kernel.events import publish_event
    publish_event("actor.stop", {"group_id": group.group_id, "actor_id": actor_id})
    try:
        request_pet_review(
            group.group_id,
            reason="actor_stop",
            source_event_id=str(event.get("id") or "").strip(),
            immediate=True,
        )
    except Exception:
        pass

    return DaemonResponse(ok=True, result={"actor": actor, "event": event})


def handle_actor_restart(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    effective_runner_kind: Callable[[str], str],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[..., None],
    find_scope_url: Callable[[Any, str], str],
    merge_actor_env_with_private: Callable[[str, str, Dict[str, Any]], Dict[str, Any]],
    inject_actor_context_env: Callable[..., Dict[str, Any]],
    normalize_runtime_command: Callable[[str, list[str]], list[str]],
    prepare_pty_env: Callable[[Dict[str, Any]], Dict[str, Any]],
    pty_backlog_bytes: Callable[[], int],
    ensure_mcp_installed: Callable[..., bool],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[..., None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    supported_runtimes: Sequence[str],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    before_foreman = foreman_id(group)
    caller_context_explicit = "caller_id" in args or "is_admin" in args
    caller_id = str(args.get("caller_id") or "").strip()
    is_admin = coerce_bool(args.get("is_admin"), default=not caller_context_explicit)
    try:
        require_actor_permission(group, by=by, action="actor.restart", target_actor_id=actor_id)
        actor = update_actor(group, actor_id, {"enabled": True})
        actor = resolve_linked_actor_before_start(
            group,
            actor_id,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            update_actor_private_env=update_actor_private_env,
            caller_id=caller_id,
            is_admin=is_admin,
        )
        runner_kind = str(actor.get("runner") or "pty").strip()
        runtime = str(actor.get("runtime") or "codex").strip() or "codex"
        if runtime == "web_model" and effective_runner_kind(runner_kind) == "headless":
            remove_headless_state(group.group_id, actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
        elif runtime == "codex" and effective_runner_kind(runner_kind) == "headless":
            codex_app_supervisor.stop_actor(group_id=group.group_id, actor_id=actor_id)
            remove_headless_state(group.group_id, actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
        elif runtime == "claude" and effective_runner_kind(runner_kind) == "headless":
            claude_app_supervisor.stop_actor(group_id=group.group_id, actor_id=actor_id)
            remove_headless_state(group.group_id, actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
        elif effective_runner_kind(runner_kind) == "headless":
            headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
            remove_headless_state(group.group_id, actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
        else:
            pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
            remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
            remove_headless_state(group.group_id, actor_id)
        clear_preamble_sent(group, actor_id)
        throttle_reset_actor(group.group_id, actor_id, keep_pending=True)
    except Exception as e:
        msg = str(e)
        if "profile not found:" in msg:
            return _error("profile_not_found", msg)
        if isinstance(e, ActorProfileAccessDeniedError):
            return _error("permission_denied", msg)
        return _error("actor_restart_failed", msg)

    runner_effective = effective_runner_kind(str(actor.get("runner") or "pty"))
    if coerce_bool(group.doc.get("running"), default=False):
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
                supported_runtimes=list(supported_runtimes),
                caller_id=caller_id,
                is_admin=is_admin,
                merge_actor_env_with_private=merge_actor_env_with_private,
            )
        except ValueError as e:
            msg = str(e)
            if msg == "no active scope for group":
                return _error(
                    "missing_project_root",
                    "missing project root for group (no active scope)",
                    details={"hint": "Attach a project root first (e.g. cccc attach <path> --group <id>)"},
                )
            if msg.startswith("scope not attached:"):
                scope_key = msg.partition(":")[2].strip()
                return _error(
                    "scope_not_attached",
                    msg,
                    details={
                        "group_id": group.group_id,
                        "actor_id": actor_id,
                        "scope_key": scope_key,
                        "hint": "Attach this scope to the group (cccc attach <path> --group <id>)",
                    },
                )
            if msg.startswith("project root path does not exist:"):
                return _error(
                    "invalid_project_root",
                    "project root path does not exist",
                    details={
                        "group_id": group.group_id,
                        "actor_id": actor_id,
                        "scope_key": str(actor.get("default_scope_key") or group.doc.get("active_scope_key") or "").strip(),
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
                        "group_id": group.group_id,
                        "actor_id": actor_id,
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
                        "group_id": group.group_id,
                        "actor_id": actor_id,
                        "runtime": str(actor.get("runtime") or "codex").strip() or "codex",
                        "hint": "Set actor.command (or switch runner to headless).",
                    },
                )
            return _error("actor_restart_failed", msg)
        cwd = launch_spec["cwd"]
        runner_kind = str(launch_spec["runner"])
        runner_effective = str(launch_spec["effective_runner"])
        runtime = str(launch_spec["runtime"])
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
                return _error("actor_restart_failed", f"failed to install MCP: {e}")
            if not mcp_ready:
                return _error("actor_restart_failed", f"failed to install MCP for runtime: {runtime}")

        if runtime == "web_model" and runner_effective == "headless":
            try:
                write_headless_state(group.group_id, actor_id)
            except Exception:
                pass
        elif runtime == "codex" and runner_effective == "headless":
            codex_app_supervisor.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                env=dict(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=actor_id)),
                model=model_from_runtime_command(launch_spec["effective_command"]),
            )
        elif runtime == "claude" and runner_effective == "headless":
            claude_app_supervisor.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                env=dict(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=actor_id)),
                model=model_from_runtime_command(launch_spec["effective_command"]),
            )
        elif runner_effective == "headless":
            headless_runner.SUPERVISOR.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                env=dict(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=actor_id)),
            )
            try:
                write_headless_state(group.group_id, actor_id)
            except Exception:
                pass
        else:
            session = pty_runner.SUPERVISOR.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                command=launch_spec["effective_command"],
                env=prepare_pty_env(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=actor_id)),
                runtime=runtime,
                max_backlog_bytes=pty_backlog_bytes(),
            )
            try:
                write_pty_state(group.group_id, actor_id, pid=session.pid)
            except Exception:
                pass
        try:
            ContextStorage(group).clear_agent_status_if_present(actor_id)
        except Exception:
            pass
        try:
            if str(group.doc.get("state") or "").strip() == "stopped":
                group.doc["state"] = "active"
            group.doc["running"] = True
            group.save()
        except Exception:
            pass

    maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    event = append_event(
        group.ledger_path,
        kind="actor.restart",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={
            "actor_id": actor_id,
            "runner": str(actor.get("runner") or "pty"),
            "runner_effective": runner_effective,
        },
    )

    from ...kernel.events import publish_event
    publish_event("actor.restart", {"group_id": group.group_id, "actor_id": actor_id})
    try:
        request_pet_review(
            group.group_id,
            reason="actor_restart",
            source_event_id=str(event.get("id") or "").strip(),
            immediate=True,
        )
    except Exception:
        pass

    return DaemonResponse(ok=True, result={"actor": actor, "event": event})


def try_handle_actor_lifecycle_op(
    op: str,
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    start_actor_process: Callable[..., Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[..., None],
    find_scope_url: Callable[[Any, str], str],
    merge_actor_env_with_private: Callable[[str, str, Dict[str, Any]], Dict[str, Any]],
    inject_actor_context_env: Callable[..., Dict[str, Any]],
    normalize_runtime_command: Callable[[str, list[str]], list[str]],
    prepare_pty_env: Callable[[Dict[str, Any]], Dict[str, Any]],
    pty_backlog_bytes: Callable[[], int],
    ensure_mcp_installed: Callable[..., bool],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[..., None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    supported_runtimes: Sequence[str],
) -> Optional[DaemonResponse]:
    if op == "actor_start":
        return handle_actor_start(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            start_actor_process=start_actor_process,
            effective_runner_kind=effective_runner_kind,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            update_actor_private_env=update_actor_private_env,
        )
    if op == "actor_stop":
        return handle_actor_stop(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            effective_runner_kind=effective_runner_kind,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
        )
    if op == "actor_restart":
        return handle_actor_restart(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            effective_runner_kind=effective_runner_kind,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
            clear_preamble_sent=clear_preamble_sent,
            throttle_reset_actor=throttle_reset_actor,
            find_scope_url=find_scope_url,
            merge_actor_env_with_private=merge_actor_env_with_private,
            inject_actor_context_env=inject_actor_context_env,
            normalize_runtime_command=normalize_runtime_command,
            prepare_pty_env=prepare_pty_env,
            pty_backlog_bytes=pty_backlog_bytes,
            ensure_mcp_installed=ensure_mcp_installed,
            write_headless_state=write_headless_state,
            write_pty_state=write_pty_state,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            update_actor_private_env=update_actor_private_env,
            supported_runtimes=supported_runtimes,
        )
    return None
