"""Actor add operation handler for daemon."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional, Sequence

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import add_actor, find_actor, generate_actor_id, get_effective_role, remove_actor
from ...kernel.group import load_group
from ...kernel.inbox import set_cursor
from ...kernel.ledger import append_event
from ...kernel.permissions import require_actor_permission
from ...kernel.runtime import get_runtime_command_with_flags
from ..actor_profile_runtime import apply_profile_link_to_actor


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_actor_add(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    start_actor_process: Callable[..., Dict[str, Any]],
    pty_supported: Callable[[], bool],
    effective_runner_kind: Callable[[str], str],
    validate_private_env_key: Callable[[Any], str],
    coerce_private_env_value: Callable[[Any], str],
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
    private_env_max_keys: int,
    supported_runtimes: Sequence[str],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    warn_forced_headless: Optional[Callable[[str, str], None]] = None,
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    title = str(args.get("title") or "").strip()
    submit = str(args.get("submit") or "").strip()
    requested_runner = str(args.get("runner") or "").strip()
    runtime = str(args.get("runtime") or "codex").strip()
    by = str(args.get("by") or "user").strip()
    command_raw = args.get("command")
    env_raw = args.get("env")
    env_private_raw = args.get("env_private")
    default_scope_key = str(args.get("default_scope_key") or "").strip()
    profile_id = str(args.get("profile_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    before_foreman = foreman_id(group)

    try:
        require_actor_permission(group, by=by, action="actor.add")
        env_private_set: Dict[str, str] = {}
        if env_private_raw is not None:
            if profile_id:
                raise ValueError("env_private is not allowed when profile_id is used")
            if by != "user":
                raise ValueError("env_private is only allowed for by=user")
            if not isinstance(env_private_raw, dict):
                raise ValueError("env_private must be an object")
            for key, value in env_private_raw.items():
                private_key = validate_private_env_key(key)
                private_value = coerce_private_env_value(value)
                env_private_set[private_key] = private_value
            if len(env_private_set) > private_env_max_keys:
                raise ValueError("too many env_private keys")

        linked_profile: Optional[Dict[str, Any]] = None
        linked_profile_id = ""
        linked_profile_legacy_env: Dict[str, str] = {}
        if profile_id:
            linked_profile_id = profile_id
            linked_profile = get_actor_profile(linked_profile_id)
            if not isinstance(linked_profile, dict):
                raise ValueError(f"profile not found: {linked_profile_id}")
            runtime = str(linked_profile.get("runtime") or "codex").strip() or "codex"
            requested_runner = str(linked_profile.get("runner") or "pty").strip() or "pty"
            submit = str(linked_profile.get("submit") or submit or "enter").strip() or "enter"
            linked_profile_secrets = load_actor_profile_secrets(linked_profile_id)
            profile_env_raw = linked_profile.get("env")
            if isinstance(profile_env_raw, dict):
                linked_profile_legacy_env = {
                    str(key): str(value)
                    for key, value in profile_env_raw.items()
                    if isinstance(key, str) and value is not None and str(key).strip()
                }
            merged_profile_vars: Dict[str, str] = {}
            merged_profile_vars.update(linked_profile_legacy_env)
            merged_profile_vars.update(linked_profile_secrets)
            if len(merged_profile_vars) > private_env_max_keys:
                raise ValueError("too many profile private env keys configured")

        runner = requested_runner or ("pty" if pty_supported() else "headless")
        if runner not in ("pty", "headless"):
            raise ValueError("invalid runner (must be 'pty' or 'headless')")
        if runtime not in supported_runtimes:
            raise ValueError("invalid runtime")
        forced_headless = False
        if runner == "pty" and not pty_supported():
            runner = "headless"
            forced_headless = True

        foreman_cfg: Optional[Dict[str, Any]] = None
        if by and by != "user":
            try:
                if get_effective_role(group, by) == "foreman":
                    foreman_cfg = find_actor(group, by)
            except Exception:
                foreman_cfg = None

        if not actor_id:
            actor_id = generate_actor_id(group, runtime=runtime)

        command: list[str] = []
        if isinstance(linked_profile, dict):
            profile_command = linked_profile.get("command")
            if isinstance(profile_command, list):
                command = [str(item) for item in profile_command if isinstance(item, str) and str(item).strip()]
        elif isinstance(command_raw, list) and all(isinstance(item, str) for item in command_raw):
            command = [str(item) for item in command_raw if str(item).strip()]

        env: Dict[str, str] = {}
        if isinstance(linked_profile, dict):
            env = {}
        elif isinstance(env_raw, dict) and all(isinstance(key, str) and isinstance(value, str) for key, value in env_raw.items()):
            env = {str(key): str(value) for key, value in env_raw.items()}

        if isinstance(foreman_cfg, dict) and foreman_cfg.get("id") == by and not linked_profile_id:
            foreman_runtime = str(foreman_cfg.get("runtime") or "").strip()
            foreman_runner = str(foreman_cfg.get("runner") or "pty").strip() or "pty"
            foreman_runner_effective = effective_runner_kind(foreman_runner)
            runner_effective = effective_runner_kind(runner)
            foreman_command_raw = foreman_cfg.get("command") if isinstance(foreman_cfg.get("command"), list) else []
            foreman_command = [str(item) for item in foreman_command_raw if isinstance(item, str) and str(item).strip()]
            foreman_env_raw = foreman_cfg.get("env") if isinstance(foreman_cfg.get("env"), dict) else {}
            foreman_env = {
                str(key): str(value)
                for key, value in foreman_env_raw.items()
                if isinstance(key, str) and isinstance(value, str)
            }

            if not foreman_runtime:
                raise ValueError("foreman config missing runtime")
            if runtime != foreman_runtime:
                raise ValueError(f"foreman can only add actors with the same runtime as itself (expected: {foreman_runtime})")
            if runner_effective != foreman_runner_effective:
                raise ValueError(
                    f"foreman can only add actors with the same runner as itself (expected: {foreman_runner_effective})"
                )

            if not command:
                command = list(foreman_command) if foreman_command else get_runtime_command_with_flags(runtime)
            elif command != foreman_command:
                raise ValueError(
                    "foreman can only add actors by strict-cloning command (runtime/runner/command/env must match foreman)"
                )

            if not env:
                env = dict(foreman_env)
            if env != foreman_env:
                raise ValueError("foreman can only add actors by strict-cloning env (runtime/runner/command/env must match foreman)")
        else:
            if not command:
                command = get_runtime_command_with_flags(runtime)

        if runtime == "custom" and runner != "headless" and not command:
            raise ValueError("custom runtime requires a command (PTY runner)")

        actor = add_actor(
            group,
            actor_id=actor_id,
            title=title,
            command=command,
            env=env,
            default_scope_key=default_scope_key,
            submit=submit or "enter",
            runner=runner,  # type: ignore
            runtime=runtime,  # type: ignore
        )

        effective_actor_id = str(actor.get("id") or actor_id).strip() or actor_id

        if linked_profile_id and isinstance(linked_profile, dict):
            actor = apply_profile_link_to_actor(
                group,
                effective_actor_id,
                profile_id=linked_profile_id,
                profile=linked_profile,
                load_actor_profile_secrets=load_actor_profile_secrets,
                update_actor_private_env=update_actor_private_env,
            )

        if env_private_raw is not None:
            try:
                update_actor_private_env(
                    group.group_id,
                    effective_actor_id,
                    set_vars=env_private_set,
                    unset_keys=[],
                    clear=True,
                )
            except Exception:
                try:
                    remove_actor(group, effective_actor_id)
                except Exception:
                    pass
                try:
                    delete_actor_private_env(group.group_id, effective_actor_id)
                except Exception:
                    pass
                raise RuntimeError("failed to store env_private")
    except Exception as e:
        return _error("actor_add_failed", str(e))

    if forced_headless and callable(warn_forced_headless):
        try:
            warn_forced_headless(group.group_id, str(actor.get("id") or actor_id))
        except Exception:
            pass

    event = append_event(
        group.ledger_path,
        kind="actor.add",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"actor": actor},
    )
    try:
        set_cursor(group, actor_id, event_id=str(event.get("id") or ""), ts=str(event.get("ts") or ""))
    except Exception:
        pass

    maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    start_actor_id = str(actor.get("id") or actor_id).strip() or actor_id
    start_runtime = str(actor.get("runtime") or runtime).strip() or runtime
    start_runner = str(actor.get("runner") or runner).strip() or runner
    start_command = actor.get("command") if isinstance(actor.get("command"), list) else command
    start_env = actor.get("env") if isinstance(actor.get("env"), dict) else env
    start_result = start_actor_process(
        group,
        start_actor_id,
        command=list(start_command or []),
        env=dict(start_env or {}),
        runner=start_runner,
        runtime=start_runtime,
        by=by,
    )

    result: Dict[str, Any] = {"actor": actor, "event": event}
    if start_result["success"]:
        result["start_event"] = start_result["event"]
        result["running"] = True
        if start_result.get("effective_runner") != start_runner:
            result["runner_effective"] = start_result.get("effective_runner")
    else:
        result["start_error"] = start_result.get("error")
        result["running"] = False
    if forced_headless:
        result["runner_effective"] = "headless"
    return DaemonResponse(ok=True, result=result)


def try_handle_actor_add_op(
    op: str,
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    start_actor_process: Callable[..., Dict[str, Any]],
    pty_supported: Callable[[], bool],
    effective_runner_kind: Callable[[str], str],
    validate_private_env_key: Callable[[Any], str],
    coerce_private_env_value: Callable[[Any], str],
    update_actor_private_env: Callable[..., Dict[str, str]],
    delete_actor_private_env: Callable[[str, str], None],
    private_env_max_keys: int,
    supported_runtimes: Sequence[str],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    warn_forced_headless: Optional[Callable[[str, str], None]] = None,
) -> Optional[DaemonResponse]:
    if op == "actor_add":
        return handle_actor_add(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            start_actor_process=start_actor_process,
            pty_supported=pty_supported,
            effective_runner_kind=effective_runner_kind,
            validate_private_env_key=validate_private_env_key,
            coerce_private_env_value=coerce_private_env_value,
            update_actor_private_env=update_actor_private_env,
            delete_actor_private_env=delete_actor_private_env,
            private_env_max_keys=private_env_max_keys,
            supported_runtimes=supported_runtimes,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            warn_forced_headless=warn_forced_headless,
        )
    return None
