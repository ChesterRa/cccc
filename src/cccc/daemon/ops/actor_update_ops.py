"""Actor update operation handlers for daemon."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Callable, Dict, Optional, Sequence

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor, list_actors, update_actor
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_actor_permission
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool
from ..actor_profile_runtime import (
    PROFILE_CONTROLLED_FIELDS,
    actor_profile_id,
    apply_profile_link_to_actor,
    clear_actor_link_metadata,
    is_actor_profile_linked,
    resolve_linked_actor_before_start,
)


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_actor_update(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    find_scope_url: Callable[[Any, str], str],
    effective_runner_kind: Callable[[str], str],
    ensure_mcp_installed: Callable[[str, Path], Any],
    merge_actor_env_with_private: Callable[[str, str, Dict[str, Any]], Dict[str, Any]],
    inject_actor_context_env: Callable[..., Dict[str, Any]],
    normalize_runtime_command: Callable[[str, list[str]], list[str]],
    prepare_pty_env: Callable[[Dict[str, Any]], Dict[str, Any]],
    pty_backlog_bytes: Callable[[], int],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[..., None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[..., None],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    supported_runtimes: Sequence[str],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
    profile_id_arg = str(args.get("profile_id") or "").strip()
    profile_action = str(args.get("profile_action") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    allowed = {"role", "title", "command", "env", "default_scope_key", "submit", "enabled", "runner", "runtime"}
    unknown = set(patch.keys()) - allowed
    if unknown:
        return _error("invalid_patch", "invalid patch keys", details={"unknown_keys": sorted(unknown)})
    if profile_action and profile_action not in ("convert_to_custom",):
        return _error("invalid_request", "invalid profile_action")
    if profile_action and profile_id_arg:
        return _error("invalid_request", "profile_action and profile_id are mutually exclusive")
    if not patch and not profile_id_arg and not profile_action:
        return _error("invalid_patch", "empty patch")
    actor_existing = find_actor(group, actor_id)
    if not isinstance(actor_existing, dict):
        return _error("actor_not_found", f"actor not found: {actor_id}")
    linked_before = is_actor_profile_linked(actor_existing)
    controlled_patch_keys = sorted([key for key in PROFILE_CONTROLLED_FIELDS if key in patch])
    if linked_before and controlled_patch_keys:
        return _error(
            "actor_profile_linked_readonly",
            "linked actor runtime fields are read-only (convert to custom first)",
            details={"keys": controlled_patch_keys},
        )
    if profile_action == "convert_to_custom" and controlled_patch_keys:
        return _error(
            "invalid_request",
            "cannot combine convert_to_custom with runtime field patch",
            details={"keys": controlled_patch_keys},
        )
    if profile_id_arg and controlled_patch_keys:
        return _error(
            "invalid_request",
            "cannot patch runtime fields while attaching profile",
            details={"keys": controlled_patch_keys},
        )
    enabled_patched = "enabled" in patch
    before_foreman = foreman_id(group) if enabled_patched else ""
    applied_profile_id = ""
    profile_converted = False
    actor: Dict[str, Any]
    try:
        require_actor_permission(group, by=by, action="actor.update", target_actor_id=actor_id)
        if profile_action == "convert_to_custom":
            current = find_actor(group, actor_id)
            if not isinstance(current, dict) or not is_actor_profile_linked(current):
                raise ValueError("actor is not linked to a profile")
            current_profile_id = actor_profile_id(current)
            profile = get_actor_profile(current_profile_id)
            if not isinstance(profile, dict):
                raise ValueError(f"profile not found: {current_profile_id}")
            apply_profile_link_to_actor(
                group,
                actor_id,
                profile_id=current_profile_id,
                profile=profile,
                load_actor_profile_secrets=load_actor_profile_secrets,
                update_actor_private_env=update_actor_private_env,
            )
            clear_actor_link_metadata(group, actor_id)
            profile_converted = True

        if profile_id_arg:
            profile = get_actor_profile(profile_id_arg)
            if not isinstance(profile, dict):
                raise ValueError(f"profile not found: {profile_id_arg}")
            apply_profile_link_to_actor(
                group,
                actor_id,
                profile_id=profile_id_arg,
                profile=profile,
                load_actor_profile_secrets=load_actor_profile_secrets,
                update_actor_private_env=update_actor_private_env,
            )
            applied_profile_id = profile_id_arg

        actor = find_actor(group, actor_id) or {}
        if patch:
            actor = update_actor(group, actor_id, patch)
        else:
            actor = dict(actor)
    except Exception as e:
        return _error("actor_update_failed", str(e))

    if enabled_patched:
        if coerce_bool(actor.get("enabled"), default=False):
            if coerce_bool(group.doc.get("running"), default=False):
                try:
                    actor = resolve_linked_actor_before_start(
                        group,
                        actor_id,
                        get_actor_profile=get_actor_profile,
                        load_actor_profile_secrets=load_actor_profile_secrets,
                        update_actor_private_env=update_actor_private_env,
                    )
                except Exception as e:
                    return _error("profile_not_found", str(e))

                group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
                if not group_scope_key:
                    return _error(
                        "missing_project_root",
                        "missing project root for group (no active scope)",
                        details={"hint": "Attach a project root first (e.g. cccc attach <path> --group <id>)"},
                    )
                scope_key = str(actor.get("default_scope_key") or group_scope_key).strip()
                url = find_scope_url(group, scope_key)
                if not url:
                    return _error(
                        "scope_not_attached",
                        f"scope not attached: {scope_key}",
                        details={
                            "group_id": group.group_id,
                            "actor_id": actor_id,
                            "scope_key": scope_key,
                            "hint": "Attach this scope to the group (cccc attach <path> --group <id>)",
                        },
                    )
                cwd = Path(url).expanduser().resolve()
                if not cwd.exists():
                    return _error(
                        "invalid_project_root",
                        "project root path does not exist",
                        details={
                            "group_id": group.group_id,
                            "actor_id": actor_id,
                            "scope_key": scope_key,
                            "path": str(cwd),
                            "hint": "Re-attach a valid project root (cccc attach <path> --group <id>)",
                        },
                    )
                cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
                env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
                runner_kind = str(actor.get("runner") or "pty").strip() or "pty"
                runner_effective = effective_runner_kind(runner_kind)
                runtime = str(actor.get("runtime") or "codex").strip() or "codex"
                if runtime not in supported_runtimes:
                    return _error(
                        "unsupported_runtime",
                        f"unsupported runtime: {runtime}",
                        details={
                            "group_id": group.group_id,
                            "actor_id": actor_id,
                            "runtime": runtime,
                            "supported": list(supported_runtimes),
                            "hint": "Change the actor runtime to a supported one.",
                        },
                    )
                if runtime == "custom" and runner_effective != "headless" and not cmd:
                    return _error(
                        "missing_command",
                        "custom runtime requires a command (PTY runner)",
                        details={
                            "group_id": group.group_id,
                            "actor_id": actor_id,
                            "runtime": runtime,
                            "hint": "Set actor.command (or switch runner to headless).",
                        },
                    )
                ensure_mcp_installed(runtime, cwd)

                if runner_effective == "headless":
                    effective_env = merge_actor_env_with_private(group.group_id, actor_id, env)
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
                    effective_env = merge_actor_env_with_private(group.group_id, actor_id, env)
                    effective_cmd = normalize_runtime_command(runtime, list(cmd or []))
                    session = pty_runner.SUPERVISOR.start_actor(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        cwd=cwd,
                        command=effective_cmd,
                        env=prepare_pty_env(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=actor_id)),
                        max_backlog_bytes=pty_backlog_bytes(),
                    )
                    try:
                        write_pty_state(group.group_id, actor_id, pid=session.pid)
                    except Exception:
                        pass

                clear_preamble_sent(group, actor_id)
                throttle_reset_actor(group.group_id, actor_id, keep_pending=True)
        else:
            runner_kind = str(actor.get("runner") or "pty").strip() or "pty"
            runner_effective = effective_runner_kind(runner_kind)
            if runner_effective == "headless":
                headless_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
                remove_headless_state(group.group_id, actor_id)
                remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
            else:
                pty_runner.SUPERVISOR.stop_actor(group_id=group.group_id, actor_id=actor_id)
                remove_pty_state_if_pid(group.group_id, actor_id, pid=0)
                remove_headless_state(group.group_id, actor_id)
            throttle_reset_actor(group.group_id, actor_id, keep_pending=True)
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

    if enabled_patched:
        maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    event_data: Dict[str, Any] = {
        "actor_id": actor_id,
        "patch": patch,
    }
    if applied_profile_id:
        event_data["profile_id"] = applied_profile_id
    if profile_converted:
        event_data["profile_action"] = "convert_to_custom"

    event = append_event(
        group.ledger_path,
        kind="actor.update",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data=event_data,
    )
    return DaemonResponse(ok=True, result={"actor": actor, "event": event})


def try_handle_actor_update_op(
    op: str,
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    find_scope_url: Callable[[Any, str], str],
    effective_runner_kind: Callable[[str], str],
    ensure_mcp_installed: Callable[[str, Path], Any],
    merge_actor_env_with_private: Callable[[str, str, Dict[str, Any]], Dict[str, Any]],
    inject_actor_context_env: Callable[..., Dict[str, Any]],
    normalize_runtime_command: Callable[[str, list[str]], list[str]],
    prepare_pty_env: Callable[[Dict[str, Any]], Dict[str, Any]],
    pty_backlog_bytes: Callable[[], int],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[..., None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[..., None],
    remove_headless_state: Callable[[str, str], None],
    remove_pty_state_if_pid: Callable[..., None],
    supported_runtimes: Sequence[str],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> Optional[DaemonResponse]:
    if op == "actor_update":
        return handle_actor_update(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            find_scope_url=find_scope_url,
            effective_runner_kind=effective_runner_kind,
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
            remove_headless_state=remove_headless_state,
            remove_pty_state_if_pid=remove_pty_state_if_pid,
            supported_runtimes=supported_runtimes,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            update_actor_private_env=update_actor_private_env,
        )
    return None
