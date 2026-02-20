"""Actor profile operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor
from ...kernel.group import load_group
from ...kernel.registry import load_registry
from ...util.conv import coerce_bool
from ..actor_profile_runtime import apply_profile_link_to_actor, clear_actor_link_metadata
from ..actor_profile_store import (
    ProfileRevisionMismatchError,
    delete_actor_profile,
    delete_actor_profile_secrets,
    get_actor_profile,
    list_actor_profiles,
    load_actor_profile_secrets,
    update_actor_profile_secrets,
    upsert_actor_profile,
    validate_actor_profile_id,
)
from ..private_env_ops import (
    PRIVATE_ENV_MAX_KEYS,
    coerce_private_env_value,
    load_actor_private_env,
    mask_private_env_value,
    update_actor_private_env,
    validate_private_env_key,
)


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _is_user_writer(by: str) -> bool:
    who = str(by or "").strip()
    return not who or who == "user"


def _profile_usage_map() -> Dict[str, List[Dict[str, str]]]:
    usage: Dict[str, List[Dict[str, str]]] = {}
    try:
        reg = load_registry()
    except Exception:
        return usage
    groups = reg.groups if isinstance(reg.groups, dict) else {}
    for gid in sorted(groups.keys()):
        group = load_group(str(gid))
        if group is None:
            continue
        group_title = str(group.doc.get("title") or "").strip()
        actors = group.doc.get("actors") if isinstance(group.doc.get("actors"), list) else []
        for actor in actors:
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            actor_title = str(actor.get("title") or "").strip()
            pid = str(actor.get("profile_id") or "").strip()
            if not aid or not pid:
                continue
            usage.setdefault(pid, []).append(
                {
                    "group_id": group.group_id,
                    "group_title": group_title,
                    "actor_id": aid,
                    "actor_title": actor_title,
                }
            )
    return usage


def handle_actor_profile_list(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if by != "user" and not by:
        return _error("permission_denied", "invalid caller")
    try:
        usage = _profile_usage_map()
        profiles = []
        for item in list_actor_profiles():
            pid = str(item.get("id") or "").strip()
            out = dict(item)
            out["usage_count"] = len(usage.get(pid, []))
            profiles.append(out)
        return DaemonResponse(ok=True, result={"profiles": profiles})
    except Exception as e:
        return _error("actor_profile_list_failed", str(e))


def handle_actor_profile_get(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if by != "user" and not by:
        return _error("permission_denied", "invalid caller")
    profile_id = str(args.get("profile_id") or "").strip()
    if not profile_id:
        return _error("missing_profile_id", "missing profile_id")
    try:
        pid = validate_actor_profile_id(profile_id)
        profile = get_actor_profile(pid)
        if not isinstance(profile, dict):
            return _error("profile_not_found", f"profile not found: {pid}")
        usage = _profile_usage_map().get(pid, [])
        return DaemonResponse(ok=True, result={"profile": profile, "usage": usage})
    except Exception as e:
        return _error("actor_profile_get_failed", str(e))


def handle_actor_profile_upsert(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if not _is_user_writer(by):
        return _error("permission_denied", "only user can modify actor profiles")
    profile = args.get("profile")
    if not isinstance(profile, dict):
        return _error("invalid_request", "profile must be an object")
    expected_revision_raw = args.get("expected_revision")
    expected_revision: Optional[int] = None
    if expected_revision_raw is not None:
        try:
            expected_revision = int(expected_revision_raw)
        except Exception:
            return _error("invalid_request", "expected_revision must be an integer")
    try:
        payload = dict(profile)
        legacy_env_raw = payload.get("env")
        legacy_env: Dict[str, str] = {}
        if legacy_env_raw is not None:
            if not isinstance(legacy_env_raw, dict):
                return _error("invalid_request", "profile.env must be an object")
            for key, value in legacy_env_raw.items():
                k = validate_private_env_key(key)
                v = coerce_private_env_value(value)
                legacy_env[k] = v

        # Unified model: actor profile variables are secret env.
        # Keep env field empty for storage/backward schema compatibility.
        payload["env"] = {}

        profile_id_hint = str(payload.get("id") or "").strip()
        if profile_id_hint and legacy_env:
            pid = validate_actor_profile_id(profile_id_hint)
            current = load_actor_profile_secrets(pid)
            preview = dict(current)
            preview.update(legacy_env)
            if len(preview) > PRIVATE_ENV_MAX_KEYS:
                return _error("too_many_keys", "too many profile secret keys configured")
        elif legacy_env and len(legacy_env) > PRIVATE_ENV_MAX_KEYS:
            return _error("too_many_keys", "too many profile secret keys configured")

        updated = upsert_actor_profile(payload, expected_revision=expected_revision)
        if legacy_env:
            pid = str(updated.get("id") or "").strip()
            if pid:
                merged = update_actor_profile_secrets(
                    pid,
                    set_vars=legacy_env,
                    unset_keys=[],
                    clear=False,
                )
                if len(merged) > PRIVATE_ENV_MAX_KEYS:
                    return _error("too_many_keys", "too many profile secret keys configured")
        return DaemonResponse(ok=True, result={"profile": updated})
    except ProfileRevisionMismatchError as e:
        return _error("profile_revision_mismatch", str(e))
    except Exception as e:
        return _error("actor_profile_upsert_failed", str(e))


def handle_actor_profile_delete(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if not _is_user_writer(by):
        return _error("permission_denied", "only user can delete actor profiles")
    profile_id = str(args.get("profile_id") or "").strip()
    force_detach = coerce_bool(args.get("force_detach"), default=False)
    if not profile_id:
        return _error("missing_profile_id", "missing profile_id")
    try:
        pid = validate_actor_profile_id(profile_id)
        profile = get_actor_profile(pid)
        if not isinstance(profile, dict):
            return _error("profile_not_found", f"profile not found: {pid}")
        usage = _profile_usage_map().get(pid, [])
        if usage and not force_detach:
            return _error(
                "profile_in_use",
                "profile is in use by linked actors",
                details={"profile_id": pid, "usage": usage},
            )
        detached: List[Dict[str, str]] = []
        if usage and force_detach:
            for item in usage:
                group_id = str(item.get("group_id") or "").strip()
                actor_id = str(item.get("actor_id") or "").strip()
                if not group_id or not actor_id:
                    continue
                group = load_group(group_id)
                if group is None:
                    continue
                actor = find_actor(group, actor_id)
                if not isinstance(actor, dict):
                    continue
                if str(actor.get("profile_id") or "").strip() != pid:
                    continue
                apply_profile_link_to_actor(
                    group,
                    actor_id,
                    profile_id=pid,
                    profile=profile,
                    load_actor_profile_secrets=load_actor_profile_secrets,
                    update_actor_private_env=update_actor_private_env,
                )
                clear_actor_link_metadata(group, actor_id)
                detached.append({"group_id": group_id, "actor_id": actor_id})
        delete_actor_profile(pid)
        delete_actor_profile_secrets(pid)
        return DaemonResponse(
            ok=True,
            result={
                "deleted": True,
                "profile_id": pid,
                "detached_count": len(detached),
                "detached": detached,
            },
        )
    except Exception as e:
        return _error("actor_profile_delete_failed", str(e))


def handle_actor_profile_secret_keys(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if by != "user" and not by:
        return _error("permission_denied", "invalid caller")
    profile_id = str(args.get("profile_id") or "").strip()
    if not profile_id:
        return _error("missing_profile_id", "missing profile_id")
    try:
        pid = validate_actor_profile_id(profile_id)
        if get_actor_profile(pid) is None:
            return _error("profile_not_found", f"profile not found: {pid}")
        private_env = load_actor_profile_secrets(pid)
        keys = sorted(private_env.keys())
        masked_values = {key: mask_private_env_value(value) for key, value in private_env.items()}
        return DaemonResponse(
            ok=True,
            result={
                "profile_id": pid,
                "keys": keys,
                "masked_values": masked_values,
            },
        )
    except Exception as e:
        return _error("actor_profile_secret_keys_failed", str(e))


def handle_actor_profile_secret_update(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if not _is_user_writer(by):
        return _error("permission_denied", "only user can update profile secrets")
    profile_id = str(args.get("profile_id") or "").strip()
    if not profile_id:
        return _error("missing_profile_id", "missing profile_id")

    clear = bool(args.get("clear") is True)
    set_raw = args.get("set")
    unset_raw = args.get("unset")

    set_vars: Dict[str, str] = {}
    unset_keys: List[str] = []

    try:
        pid = validate_actor_profile_id(profile_id)
        if get_actor_profile(pid) is None:
            return _error("profile_not_found", f"profile not found: {pid}")
        if set_raw is not None:
            if not isinstance(set_raw, dict):
                raise ValueError("set must be an object")
            for key, value in set_raw.items():
                k = validate_private_env_key(key)
                v = coerce_private_env_value(value)
                set_vars[k] = v
        if unset_raw is not None:
            if not isinstance(unset_raw, list):
                raise ValueError("unset must be a list")
            for item in unset_raw:
                unset_keys.append(validate_private_env_key(item))
    except ValueError as e:
        return _error("invalid_request", str(e))
    except Exception as e:
        return _error("actor_profile_secret_update_failed", str(e))

    if len(set_vars) > PRIVATE_ENV_MAX_KEYS:
        return _error("too_many_keys", "too many env keys to set in one request")
    if len(unset_keys) > PRIVATE_ENV_MAX_KEYS:
        return _error("too_many_keys", "too many env keys to unset in one request")

    try:
        updated = update_actor_profile_secrets(
            pid,
            set_vars=set_vars,
            unset_keys=unset_keys,
            clear=clear,
        )
        if len(updated) > PRIVATE_ENV_MAX_KEYS:
            update_actor_profile_secrets(pid, set_vars={}, unset_keys=list(updated.keys()), clear=True)
            return _error("too_many_keys", "too many profile secret keys configured")
        keys = sorted(updated.keys())
        return DaemonResponse(ok=True, result={"profile_id": pid, "keys": keys})
    except Exception as e:
        return _error("actor_profile_secret_update_failed", str(e))


def handle_actor_profile_secret_copy_from_actor(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if not _is_user_writer(by):
        return _error("permission_denied", "only user can copy profile secrets from actor")

    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    profile_id = str(args.get("profile_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    if not profile_id:
        return _error("missing_profile_id", "missing profile_id")

    try:
        pid = validate_actor_profile_id(profile_id)
        if get_actor_profile(pid) is None:
            return _error("profile_not_found", f"profile not found: {pid}")

        group = load_group(group_id)
        if group is None:
            return _error("group_not_found", f"group not found: {group_id}")
        actor = find_actor(group, actor_id)
        if not isinstance(actor, dict):
            return _error("actor_not_found", f"actor not found: {actor_id}")

        actor_public_env_raw = actor.get("env")
        actor_public_env: Dict[str, str] = {}
        if isinstance(actor_public_env_raw, dict):
            for key, value in actor_public_env_raw.items():
                if not isinstance(key, str):
                    continue
                k = key.strip()
                if not k:
                    continue
                actor_public_env[k] = str(value)

        actor_private = load_actor_private_env(group_id, actor_id)
        merged: Dict[str, str] = {}
        merged.update(actor_public_env)
        merged.update(actor_private)
        if len(merged) > PRIVATE_ENV_MAX_KEYS:
            return _error("too_many_keys", "too many private env keys configured on actor")

        updated = update_actor_profile_secrets(
            pid,
            set_vars=merged,
            unset_keys=[],
            clear=True,
        )
        keys = sorted(updated.keys())
        return DaemonResponse(ok=True, result={"profile_id": pid, "group_id": group_id, "actor_id": actor_id, "keys": keys})
    except Exception as e:
        return _error("actor_profile_secret_copy_from_actor_failed", str(e))


def handle_actor_profile_secret_copy_from_profile(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if not _is_user_writer(by):
        return _error("permission_denied", "only user can copy profile secrets from another profile")

    profile_id = str(args.get("profile_id") or "").strip()
    source_profile_id = str(args.get("source_profile_id") or "").strip()
    if not profile_id:
        return _error("missing_profile_id", "missing profile_id")
    if not source_profile_id:
        return _error("missing_source_profile_id", "missing source_profile_id")

    try:
        target_pid = validate_actor_profile_id(profile_id)
        source_pid = validate_actor_profile_id(source_profile_id)
        if get_actor_profile(target_pid) is None:
            return _error("profile_not_found", f"profile not found: {target_pid}")
        if get_actor_profile(source_pid) is None:
            return _error("source_profile_not_found", f"source profile not found: {source_pid}")

        source_private = load_actor_profile_secrets(source_pid)
        if len(source_private) > PRIVATE_ENV_MAX_KEYS:
            return _error("too_many_keys", "too many profile secret keys configured on source profile")

        updated = update_actor_profile_secrets(
            target_pid,
            set_vars=source_private,
            unset_keys=[],
            clear=True,
        )
        keys = sorted(updated.keys())
        return DaemonResponse(
            ok=True,
            result={
                "profile_id": target_pid,
                "source_profile_id": source_pid,
                "keys": keys,
            },
        )
    except Exception as e:
        return _error("actor_profile_secret_copy_from_profile_failed", str(e))


def try_handle_actor_profile_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "actor_profile_list":
        return handle_actor_profile_list(args)
    if op == "actor_profile_get":
        return handle_actor_profile_get(args)
    if op == "actor_profile_upsert":
        return handle_actor_profile_upsert(args)
    if op == "actor_profile_delete":
        return handle_actor_profile_delete(args)
    if op == "actor_profile_secret_keys":
        return handle_actor_profile_secret_keys(args)
    if op == "actor_profile_secret_update":
        return handle_actor_profile_secret_update(args)
    if op == "actor_profile_secret_copy_from_actor":
        return handle_actor_profile_secret_copy_from_actor(args)
    if op == "actor_profile_secret_copy_from_profile":
        return handle_actor_profile_secret_copy_from_profile(args)
    return None
