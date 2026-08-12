"""Actor lifecycle operation handlers for daemon."""

from __future__ import annotations

import copy
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor, is_internal_actor, is_supported_internal_actor, list_actors, update_actor
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_actor_permission
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ..runtime_session_ops import (
    read_runtime_session,
    remove_runtime_session,
    runtime_session_path,
    write_runtime_session,
)
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool
from .actor_runtime_ops import actor_runtime_running, stop_actor_runtime_handles
from .actor_profile_runtime import ActorProfileAccessDeniedError, resolve_linked_actor_before_start

_NEW_SESSION_RUNTIMES = frozenset({"antigravity", "claude", "codex", "grok"})


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _is_unsupported_internal_actor(actor: Any) -> bool:
    return isinstance(actor, dict) and is_internal_actor(actor) and not is_supported_internal_actor(actor)


def _unsupported_internal_actor_error(group_id: str, actor_id: str, actor: Dict[str, Any]) -> DaemonResponse:
    return _error(
        "unsupported_internal_actor",
        "unsupported internal actor cannot be started",
        details={
            "group_id": group_id,
            "actor_id": actor_id,
            "internal_kind": str(actor.get("internal_kind") or "").strip(),
        },
    )


def _restore_actor_persistent_state(
    group: Any,
    actor_id: str,
    *,
    group_doc: Dict[str, Any],
    private_env: Dict[str, str],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> list[str]:
    failures: list[str] = []
    try:
        group.doc = copy.deepcopy(group_doc)
        group.save()
    except Exception as error:
        failures.append(f"group: {error}")
    try:
        update_actor_private_env(
            group.group_id,
            actor_id,
            set_vars=dict(private_env),
            unset_keys=[],
            clear=True,
        )
    except Exception as error:
        failures.append(f"private_env: {error}")
    return failures


def _error_after_rollback(
    code: str,
    message: str,
    failures: list[str],
    *,
    details: Optional[Dict[str, Any]] = None,
) -> DaemonResponse:
    if not failures:
        return _error(code, message, details=details)
    return _error("rollback_failed", f"{message}; rollback failed: {'; '.join(failures)}")


def _classify_start_failure(message: str, *, default_code: str) -> tuple[str, str]:
    if message == "no active scope for group":
        return "missing_project_root", "missing project root for group (no active scope)"
    if message.startswith("scope not attached:"):
        return "scope_not_attached", message
    if message.startswith("project root path does not exist:"):
        return "invalid_project_root", "project root path does not exist"
    if message.startswith("unsupported runtime:"):
        return "unsupported_runtime", message
    if message == "custom runtime requires a command (PTY runner)":
        return "missing_command", message
    return default_code, message


def _stop_actor_runtime_handles(
    group_id: str,
    actor_id: str,
    *,
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
) -> None:
    codex_app_supervisor.stop_actor(group_id=group_id, actor_id=actor_id)
    claude_app_supervisor.stop_actor(group_id=group_id, actor_id=actor_id)
    headless_runner.SUPERVISOR.stop_actor(group_id=group_id, actor_id=actor_id)
    pty_runner.SUPERVISOR.stop_actor(group_id=group_id, actor_id=actor_id)
    remove_headless_state(group_id, actor_id)
    remove_pty_state_if_pid(group_id, actor_id, pid=0)


def handle_actor_start(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    start_actor_process: Callable[..., Dict[str, Any]],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
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
    previous_actor = find_actor(group, actor_id)
    try:
        require_actor_permission(group, by=by, action="actor.start", target_actor_id=actor_id)
    except Exception as error:
        return _error("actor_start_failed", str(error))
    before_group_doc = copy.deepcopy(group.doc)
    try:
        before_private_env = load_actor_private_env(group_id, actor_id)
    except Exception as error:
        return _error("actor_start_failed", str(error))

    def _rollback(
        code: str,
        message: str,
        *,
        details: Optional[Dict[str, Any]] = None,
    ) -> DaemonResponse:
        failures = _restore_actor_persistent_state(
            group,
            actor_id,
            group_doc=before_group_doc,
            private_env=before_private_env,
            update_actor_private_env=update_actor_private_env,
        )
        return _error_after_rollback(code, message, failures, details=details)

    try:
        if _is_unsupported_internal_actor(previous_actor):
            return _unsupported_internal_actor_error(group.group_id, actor_id, previous_actor)
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
            return _rollback("profile_not_found", msg)
        if isinstance(e, ActorProfileAccessDeniedError):
            return _rollback("permission_denied", msg)
        return _rollback("actor_start_failed", msg)

    cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
    env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
    runner_kind = str(actor.get("runner") or "pty").strip()
    runtime = str(actor.get("runtime") or "codex").strip()
    try:
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
    except Exception as e:
        return _rollback("actor_start_failed", str(e))
    if not start_result["success"]:
        message = str(start_result.get("error") or "unknown error")
        if message == "no active scope for group":
            return _rollback(
                "missing_project_root",
                "missing project root for group (no active scope)",
            )
        if message.startswith("scope not attached:"):
            return _rollback("scope_not_attached", message)
        if message.startswith("project root path does not exist:"):
            return _rollback("invalid_project_root", "project root path does not exist")
        return _rollback("actor_start_failed", message)

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
    start_actor_process: Callable[..., Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
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
    before_group_doc: Optional[Dict[str, Any]] = None
    before_private_env: Dict[str, str] = {}
    runtime_was_running = False
    actor: Dict[str, Any] = {}
    try:
        require_actor_permission(group, by=by, action="actor.stop", target_actor_id=actor_id)
        current_actor = find_actor(group, actor_id)
        if not isinstance(current_actor, dict):
            raise ValueError(f"actor not found: {actor_id}")
        before_group_doc = copy.deepcopy(group.doc)
        before_private_env = load_actor_private_env(group_id, actor_id)
        runtime_was_running = actor_runtime_running(
            group.group_id,
            current_actor,
            effective_runner_kind=effective_runner_kind,
        )
        if isinstance(current_actor, dict) and is_internal_actor(current_actor):
            actor = dict(current_actor)
        else:
            actor = update_actor(group, actor_id, {"enabled": False})
        _stop_actor_runtime_handles(
            group.group_id,
            actor_id,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
        )
        any_enabled = any(
            coerce_bool(item.get("enabled"), default=True)
            for item in list_actors(group)
            if isinstance(item, dict) and str(item.get("id") or "").strip()
        )
        if not any_enabled:
            group.doc["running"] = False
            group.save()
        event = append_event(
            group.ledger_path,
            kind="actor.stop",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data={"actor_id": actor_id},
        )
    except Exception as error:
        if before_group_doc is None:
            return _error("actor_stop_failed", str(error))
        failures = _restore_actor_persistent_state(
            group,
            actor_id,
            group_doc=before_group_doc,
            private_env=before_private_env,
            update_actor_private_env=update_actor_private_env,
        )
        if runtime_was_running:
            restored_actor = find_actor(group, actor_id)
            if not isinstance(restored_actor, dict):
                failures.append("runtime: restored actor is missing")
            else:
                restart = start_actor_process(
                    group,
                    actor_id,
                    command=list(restored_actor.get("command") or []),
                    env=dict(restored_actor.get("env") or {}),
                    runner=str(restored_actor.get("runner") or "pty"),
                    runtime=str(restored_actor.get("runtime") or "codex"),
                    by="system",
                    launch_only=True,
                )
                if not restart.get("success"):
                    failures.append(f"runtime: {restart.get('error') or 'failed to restart'}")
        return _error_after_rollback("actor_stop_failed", str(error), failures)

    maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    from ...kernel.events import publish_event
    publish_event("actor.stop", {"group_id": group.group_id, "actor_id": actor_id})
    return DaemonResponse(ok=True, result={"actor": actor, "event": event})


def handle_actor_restart(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    start_actor_process: Callable[..., Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
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
        require_actor_permission(group, by=by, action="actor.restart", target_actor_id=actor_id)
    except Exception as error:
        return _error("actor_restart_failed", str(error))
    current_actor = find_actor(group, actor_id)
    if not isinstance(current_actor, dict):
        return _error("actor_restart_failed", f"actor not found: {actor_id}")
    if _is_unsupported_internal_actor(current_actor):
        return _unsupported_internal_actor_error(group.group_id, actor_id, current_actor)
    before_group_doc = copy.deepcopy(group.doc)
    try:
        before_private_env = load_actor_private_env(group_id, actor_id)
    except Exception as error:
        return _error("actor_restart_failed", str(error))
    runtime_was_running = actor_runtime_running(
        group.group_id,
        current_actor,
        effective_runner_kind=effective_runner_kind,
    )
    replacement_started = False
    actor = dict(current_actor)
    runner_effective = effective_runner_kind(str(actor.get("runner") or "pty"))
    failure_code = "actor_restart_failed"
    try:
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
        _stop_actor_runtime_handles(
            group.group_id,
            actor_id,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
        )
        if coerce_bool(group.doc.get("running"), default=False):
            started = start_actor_process(
                group,
                actor_id,
                command=list(actor.get("command") or []),
                env=dict(actor.get("env") or {}),
                runner=str(actor.get("runner") or "pty"),
                runtime=str(actor.get("runtime") or "codex"),
                by=by,
                caller_id=caller_id,
                is_admin=is_admin,
                launch_only=True,
                launch_reason="actor_restart",
            )
            if not started.get("success"):
                message = str(started.get("error") or "unknown error")
                failure_code, normalized = _classify_start_failure(
                    message,
                    default_code="actor_restart_failed",
                )
                raise RuntimeError(normalized)
            replacement_started = True
            runner_effective = str(started.get("effective_runner") or runner_effective)
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
    except Exception as error:
        failures: list[str] = []
        if replacement_started:
            try:
                stop_actor_runtime_handles(
                    group.group_id,
                    actor_id,
                    actor,
                    effective_runner_kind=effective_runner_kind,
                )
            except Exception as rollback_error:
                failures.append(f"replacement_runtime: {rollback_error}")
        failures.extend(
            _restore_actor_persistent_state(
                group,
                actor_id,
                group_doc=before_group_doc,
                private_env=before_private_env,
                update_actor_private_env=update_actor_private_env,
            )
        )
        if runtime_was_running:
            restored_actor = find_actor(group, actor_id)
            if not isinstance(restored_actor, dict):
                failures.append("runtime: restored actor is missing")
            else:
                restored = start_actor_process(
                    group,
                    actor_id,
                    command=list(restored_actor.get("command") or []),
                    env=dict(restored_actor.get("env") or {}),
                    runner=str(restored_actor.get("runner") or "pty"),
                    runtime=str(restored_actor.get("runtime") or "codex"),
                    by="system",
                    launch_only=True,
                )
                if not restored.get("success"):
                    failures.append(f"runtime: {restored.get('error') or 'failed to restore'}")
        return _error_after_rollback(failure_code, str(error), failures)

    maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    from ...kernel.events import publish_event

    publish_event("actor.restart", {"group_id": group.group_id, "actor_id": actor_id})
    return DaemonResponse(ok=True, result={"actor": actor, "event": event})


def handle_actor_new_session(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    start_actor_process: Callable[..., Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
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
        require_actor_permission(group, by=by, action="actor.restart", target_actor_id=actor_id)
    except Exception as error:
        return _error("actor_new_session_failed", str(error))
    current_actor = find_actor(group, actor_id)
    if not isinstance(current_actor, dict):
        return _error("actor_new_session_failed", f"actor not found: {actor_id}")
    if _is_unsupported_internal_actor(current_actor):
        return _unsupported_internal_actor_error(group.group_id, actor_id, current_actor)
    before_group_doc = copy.deepcopy(group.doc)
    try:
        before_private_env = load_actor_private_env(group_id, actor_id)
    except Exception as error:
        return _error("actor_new_session_failed", str(error))
    session_path = runtime_session_path(group_id, actor_id)
    session_existed = session_path.is_file()
    before_runtime_session = read_runtime_session(group_id, actor_id) if session_existed else {}
    runtime_was_running = actor_runtime_running(
        group.group_id,
        current_actor,
        effective_runner_kind=effective_runner_kind,
    )
    replacement_started = False
    provider_rotated = False
    actor = dict(current_actor)
    runtime = str(actor.get("runtime") or "codex").strip().lower() or "codex"
    runner_effective = effective_runner_kind(str(actor.get("runner") or "pty"))
    failure_code = "actor_new_session_failed"
    try:
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
        runtime = str(actor.get("runtime") or "codex").strip().lower() or "codex"
        if runtime not in _NEW_SESSION_RUNTIMES:
            failure_code = "unsupported_runtime"
            raise ValueError(
                "new session is supported only for antigravity, claude, codex, and grok actors"
            )
        if runtime == "antigravity" and pty_runner.SUPERVISOR.actor_running(
            group.group_id,
            actor_id,
        ):
            from ..messaging.delivery import pty_submit_text

            if not pty_submit_text(
                group,
                actor_id=actor_id,
                text="/clear",
                wait_for_submit=True,
            ):
                raise RuntimeError("failed to start a fresh Antigravity conversation")
            provider_rotated = True
            rotation = "in_place"
        else:
            _stop_actor_runtime_handles(
                group.group_id,
                actor_id,
                remove_headless_state=remove_headless_state,
                remove_pty_state_if_pid=remove_pty_state_if_pid,
            )
            remove_runtime_session(group.group_id, actor_id)
            started = start_actor_process(
                group,
                actor_id,
                command=list(actor.get("command") or []),
                env=dict(actor.get("env") or {}),
                runner=str(actor.get("runner") or "pty"),
                runtime=runtime,
                by=by,
                caller_id=caller_id,
                is_admin=is_admin,
                launch_only=True,
                launch_reason="actor_new_session",
            )
            if not started.get("success"):
                message = str(started.get("error") or "unknown error")
                failure_code, normalized = _classify_start_failure(
                    message,
                    default_code="actor_new_session_failed",
                )
                raise RuntimeError(normalized)
            replacement_started = True
            runner_effective = str(started.get("effective_runner") or runner_effective)
            if str(group.doc.get("state") or "").strip() == "stopped":
                group.doc["state"] = "active"
            group.doc["running"] = True
            group.save()
            rotation = ""
        event_data: Dict[str, Any] = {
            "actor_id": actor_id,
            "runner": str(actor.get("runner") or "pty"),
            "runner_effective": runner_effective,
            "runtime": runtime,
        }
        if rotation:
            event_data["rotation"] = rotation
        event = append_event(
            group.ledger_path,
            kind="actor.new_session",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data=event_data,
        )
    except Exception as error:
        failures: list[str] = []
        if replacement_started:
            try:
                stop_actor_runtime_handles(
                    group.group_id,
                    actor_id,
                    actor,
                    effective_runner_kind=effective_runner_kind,
                )
            except Exception as rollback_error:
                failures.append(f"replacement_runtime: {rollback_error}")
        failures.extend(
            _restore_actor_persistent_state(
                group,
                actor_id,
                group_doc=before_group_doc,
                private_env=before_private_env,
                update_actor_private_env=update_actor_private_env,
            )
        )
        try:
            if session_existed:
                write_runtime_session(group_id, actor_id, before_runtime_session)
            else:
                remove_runtime_session(group_id, actor_id)
        except Exception as rollback_error:
            failures.append(f"runtime_session: {rollback_error}")
        if provider_rotated:
            failures.append("provider_session: in-place rotation cannot be reversed")
        elif runtime_was_running:
            restored_actor = find_actor(group, actor_id)
            if not isinstance(restored_actor, dict):
                failures.append("runtime: restored actor is missing")
            else:
                restored = start_actor_process(
                    group,
                    actor_id,
                    command=list(restored_actor.get("command") or []),
                    env=dict(restored_actor.get("env") or {}),
                    runner=str(restored_actor.get("runner") or "pty"),
                    runtime=str(restored_actor.get("runtime") or "codex"),
                    by="system",
                    launch_only=True,
                    launch_reason="actor_new_session_rollback",
                )
                if not restored.get("success"):
                    failures.append(f"runtime: {restored.get('error') or 'failed to restore'}")
        return _error_after_rollback(failure_code, str(error), failures)

    maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    from ...kernel.events import publish_event

    publish_event("actor.new_session", {"group_id": group.group_id, "actor_id": actor_id})
    result: Dict[str, Any] = {
        "actor": actor,
        "event": event,
        "new_session": True,
    }
    if rotation:
        result["rotation"] = rotation
    if runner_effective != str(actor.get("runner") or "pty"):
        result["runner_effective"] = runner_effective
    return DaemonResponse(ok=True, result=result)


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
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> Optional[DaemonResponse]:
    if op == "actor_start":
        return handle_actor_start(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            start_actor_process=start_actor_process,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            load_actor_private_env=load_actor_private_env,
            update_actor_private_env=update_actor_private_env,
        )
    if op == "actor_stop":
        return handle_actor_stop(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            start_actor_process=start_actor_process,
            effective_runner_kind=effective_runner_kind,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
            load_actor_private_env=load_actor_private_env,
            update_actor_private_env=update_actor_private_env,
        )
    if op == "actor_restart":
        return handle_actor_restart(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            start_actor_process=start_actor_process,
            effective_runner_kind=effective_runner_kind,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            load_actor_private_env=load_actor_private_env,
            update_actor_private_env=update_actor_private_env,
        )
    if op == "actor_new_session":
        return handle_actor_new_session(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            start_actor_process=start_actor_process,
            effective_runner_kind=effective_runner_kind,
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            load_actor_private_env=load_actor_private_env,
            update_actor_private_env=update_actor_private_env,
        )
    return None
