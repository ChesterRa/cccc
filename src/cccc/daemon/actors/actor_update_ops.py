"""Actor update operation handlers for daemon."""

from __future__ import annotations

import copy
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor, is_internal_actor, is_supported_internal_actor, list_actors, update_actor
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_actor_permission
from ...util.conv import coerce_bool
from .actor_runtime_ops import actor_runtime_running, stop_actor_runtime_handles
from .actor_profile_runtime import (
    PROFILE_CONTROLLED_FIELDS,
    actor_profile_id,
    actor_profile_ref,
    apply_profile_link_to_actor,
    clear_actor_link_metadata,
    is_actor_profile_linked,
    resolve_linked_actor_before_start,
)
from .actor_profile_store import ProfileResolver, get_actor_profile_by_ref, normalize_actor_profile_ref
from .web_model_actor_policy import require_no_other_chatgpt_web_model_actor, require_standard_chatgpt_web_model_actor


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


def _normalize_capability_id_list(raw: Any) -> list[str]:
    out: list[str] = []
    if isinstance(raw, list):
        for item in raw:
            value = str(item or "").strip()
            if value and value not in out:
                out.append(value)
    return out[:128]


def handle_actor_update(
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    start_actor_process: Callable[..., Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    throttle_reset_actor: Callable[..., None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[Any], Dict[str, str]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    patch = args.get("patch") if isinstance(args.get("patch"), dict) else {}
    profile_id_arg = str(args.get("profile_id") or "").strip()
    profile_scope_raw = str(args.get("profile_scope") or "").strip().lower()
    profile_scope_arg = profile_scope_raw or "global"
    profile_owner_arg = str(args.get("profile_owner") or "").strip()
    profile_action = str(args.get("profile_action") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    allowed = {
        "role",
        "title",
        "avatar_asset_path",
        "command",
        "env",
        "default_scope_key",
        "submit",
        "capability_autoload",
        "capability_hidden",
        "enabled",
        "runner",
        "runtime",
        "runtime_state_source",
    }
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
    try:
        require_actor_permission(group, by=by, action="actor.update", target_actor_id=actor_id)
    except Exception as error:
        return _error("actor_update_failed", str(error))
    linked_before = is_actor_profile_linked(actor_existing)
    before_group_doc = copy.deepcopy(group.doc)
    try:
        before_private_env = load_actor_private_env(group_id, actor_id)
    except Exception as error:
        return _error("actor_update_failed", str(error))
    runtime_was_running = actor_runtime_running(
        group.group_id,
        actor_existing,
        effective_runner_kind=effective_runner_kind,
    )
    runtime_effect = "none"
    runtime_actor: Dict[str, Any] = dict(actor_existing)

    def _rollback(code: str, message: str) -> DaemonResponse:
        failures: list[str] = []
        if runtime_effect == "started":
            try:
                stop_actor_runtime_handles(
                    group.group_id,
                    actor_id,
                    runtime_actor,
                    effective_runner_kind=effective_runner_kind,
                )
            except Exception as error:
                failures.append(f"runtime: {error}")
        try:
            group.doc = copy.deepcopy(before_group_doc)
            group.save()
        except Exception as error:
            failures.append(f"group: {error}")
        try:
            update_actor_private_env(
                group.group_id,
                actor_id,
                set_vars=dict(before_private_env),
                unset_keys=[],
                clear=True,
            )
        except Exception as error:
            failures.append(f"private_env: {error}")
        if runtime_effect == "stopped" and runtime_was_running:
            restored_actor = find_actor(group, actor_id)
            if not isinstance(restored_actor, dict):
                failures.append("runtime: restored actor is missing")
            else:
                restarted = start_actor_process(
                    group,
                    actor_id,
                    command=list(restored_actor.get("command") or []),
                    env=dict(restored_actor.get("env") or {}),
                    runner=str(restored_actor.get("runner") or "pty"),
                    runtime=str(restored_actor.get("runtime") or "codex"),
                    by="system",
                    launch_only=True,
                )
                if not restarted.get("success"):
                    failures.append(f"runtime: {restarted.get('error') or 'failed to restart'}")
        if failures:
            return _error("rollback_failed", f"{message}; rollback failed: {'; '.join(failures)}")
        return _error(code, message)
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
    applied_profile_ref: Any = None
    profile_converted = False
    if "capability_autoload" in patch:
        patch["capability_autoload"] = _normalize_capability_id_list(patch.get("capability_autoload"))
    if "capability_hidden" in patch:
        patch["capability_hidden"] = _normalize_capability_id_list(patch.get("capability_hidden"))
    actor: Dict[str, Any]
    try:
        current_actor = find_actor(group, actor_id) or {}
        if (
            enabled_patched
            and coerce_bool(patch.get("enabled"), default=False)
            and _is_unsupported_internal_actor(current_actor)
        ):
            return _unsupported_internal_actor_error(group.group_id, actor_id, current_actor)
        if str(patch.get("runtime") or "").strip().lower() == "web_model":
            require_standard_chatgpt_web_model_actor(current_actor)
            require_no_other_chatgpt_web_model_actor(group_id=group.group_id, actor_id=actor_id)
        if profile_action == "convert_to_custom":
            current = find_actor(group, actor_id)
            if not isinstance(current, dict) or not is_actor_profile_linked(current):
                raise ValueError("actor is not linked to a profile")
            current_profile_id = actor_profile_id(current)
            current_profile_ref = actor_profile_ref(current)
            profile = get_actor_profile_by_ref(current_profile_ref) if current_profile_ref is not None else get_actor_profile(current_profile_id)
            if not isinstance(profile, dict):
                raise ValueError(f"profile not found: {current_profile_id}")
            if str(profile.get("runtime") or "").strip().lower() == "web_model":
                require_standard_chatgpt_web_model_actor(current)
                require_no_other_chatgpt_web_model_actor(group_id=group.group_id, actor_id=actor_id)
            apply_profile_link_to_actor(
                group,
                actor_id,
                profile_id=current_profile_id,
                profile_ref=current_profile_ref,
                profile=profile,
                load_actor_profile_secrets=load_actor_profile_secrets,
                update_actor_private_env=update_actor_private_env,
            )
            clear_actor_link_metadata(group, actor_id)
            profile_converted = True

        if profile_id_arg:
            applied_profile_ref = normalize_actor_profile_ref(
                {
                    "profile_id": profile_id_arg,
                    "profile_scope": profile_scope_arg,
                    "profile_owner": profile_owner_arg,
                }
            )
            if applied_profile_ref.profile_scope == "global":
                profile = get_actor_profile(profile_id_arg)
            else:
                resolver = ProfileResolver()
                resolved = resolver.resolve(
                    applied_profile_ref,
                    caller_id=str(args.get("caller_id") or "").strip(),
                    is_admin=coerce_bool(args.get("is_admin"), default=False),
                )
                profile = resolved.model_dump(exclude_none=True) if resolved is not None else None
            if not isinstance(profile, dict):
                raise ValueError(f"profile not found: {profile_id_arg}")
            if str(profile.get("runtime") or "").strip().lower() == "web_model":
                require_standard_chatgpt_web_model_actor(current_actor)
                require_no_other_chatgpt_web_model_actor(group_id=group.group_id, actor_id=actor_id)
            apply_profile_link_to_actor(
                group,
                actor_id,
                profile_id=profile_id_arg,
                profile_ref=applied_profile_ref,
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
        return _rollback("actor_update_failed", str(e))

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
                        caller_id=str(args.get("caller_id") or "").strip(),
                        is_admin=coerce_bool(args.get("is_admin"), default=False),
                    )
                except Exception as error:
                    return _rollback("profile_not_found", str(error))
                runtime_actor = dict(actor)
                started = start_actor_process(
                    group,
                    actor_id,
                    command=list(actor.get("command") or []),
                    env=dict(actor.get("env") or {}),
                    runner=str(actor.get("runner") or "pty"),
                    runtime=str(actor.get("runtime") or "codex"),
                    by=by,
                    caller_id=str(args.get("caller_id") or "").strip(),
                    is_admin=coerce_bool(args.get("is_admin"), default=False),
                    launch_only=True,
                )
                if not started.get("success"):
                    message = str(started.get("error") or "unknown error")
                    if message == "no active scope for group":
                        return _rollback(
                            "missing_project_root",
                            "missing project root for group (no active scope)",
                        )
                    if message.startswith("scope not attached:"):
                        return _rollback("scope_not_attached", message)
                    if message.startswith("project root path does not exist:"):
                        return _rollback("invalid_project_root", "project root path does not exist")
                    if message.startswith("unsupported runtime:"):
                        return _rollback("unsupported_runtime", message)
                    if message == "custom runtime requires a command (PTY runner)":
                        return _rollback("missing_command", message)
                    return _rollback("actor_update_failed", message)
                if not runtime_was_running:
                    runtime_effect = "started"
        else:
            if runtime_was_running:
                runtime_effect = "stopped"
            try:
                stop_actor_runtime_handles(
                    group.group_id,
                    actor_id,
                    actor_existing,
                    effective_runner_kind=effective_runner_kind,
                )
                throttle_reset_actor(group.group_id, actor_id, keep_pending=True)
                any_enabled = any(
                    coerce_bool(item.get("enabled"), default=True)
                    for item in list_actors(group)
                    if isinstance(item, dict) and str(item.get("id") or "").strip()
                )
                if not any_enabled:
                    group.doc["running"] = False
                    group.save()
            except Exception as error:
                return _rollback("actor_update_failed", str(error))

    event_data: Dict[str, Any] = {
        "actor_id": actor_id,
        "patch": patch,
    }
    if applied_profile_id:
        event_data["profile_id"] = applied_profile_id
        if applied_profile_ref is not None:
            event_data["profile_scope"] = applied_profile_ref.profile_scope
            event_data["profile_owner"] = applied_profile_ref.profile_owner
    if profile_converted:
        event_data["profile_action"] = "convert_to_custom"

    try:
        event = append_event(
            group.ledger_path,
            kind="actor.update",
            group_id=group.group_id,
            scope_key="",
            by=by,
            data=event_data,
        )
    except Exception as error:
        return _rollback("actor_update_failed", str(error))
    if enabled_patched:
        maybe_reset_automation_on_foreman_change(group, before_foreman_id=before_foreman)
    return DaemonResponse(ok=True, result={"actor": actor, "event": event})


def try_handle_actor_update_op(
    op: str,
    args: Dict[str, Any],
    *,
    foreman_id: Callable[[Any], str],
    maybe_reset_automation_on_foreman_change: Callable[..., None],
    start_actor_process: Callable[..., Dict[str, Any]],
    effective_runner_kind: Callable[[str], str],
    throttle_reset_actor: Callable[..., None],
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> Optional[DaemonResponse]:
    if op == "actor_update":
        return handle_actor_update(
            args,
            foreman_id=foreman_id,
            maybe_reset_automation_on_foreman_change=maybe_reset_automation_on_foreman_change,
            start_actor_process=start_actor_process,
            effective_runner_kind=effective_runner_kind,
            throttle_reset_actor=throttle_reset_actor,
            get_actor_profile=get_actor_profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            load_actor_private_env=load_actor_private_env,
            update_actor_private_env=update_actor_private_env,
        )
    return None
