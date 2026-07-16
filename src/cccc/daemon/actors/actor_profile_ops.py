"""Actor profile operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ...contracts.v1 import ActorProfileRef
from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor
from ...kernel.group import load_group
from ...kernel.registry import load_registry
from ...util.conv import coerce_bool
from .actor_profile_runtime import actor_profile_ref, apply_profile_link_to_actor, clear_actor_link_metadata
from .actor_profile_store import (
    ProfileResolver,
    ProfileRevisionMismatchError,
    delete_actor_profile_secrets,
    get_actor_profile,
    load_actor_profile_secrets,
    normalize_actor_profile_ref,
    update_actor_profile_secrets,
    validate_actor_profile_id,
)
from .private_env_ops import (
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


def _caller_context(args: Dict[str, Any]) -> tuple[str, bool, bool]:
    explicit = "caller_id" in args or "is_admin" in args
    caller_id = str(args.get("caller_id") or "").strip()
    is_admin = coerce_bool(args.get("is_admin"), default=not explicit)
    return caller_id, is_admin, explicit


def _profile_ref_from_args(args: Dict[str, Any], *, profile_id_key: str = "profile_id") -> ActorProfileRef:
    return normalize_actor_profile_ref(
        {
            "profile_id": args.get(profile_id_key),
            "profile_scope": args.get("profile_scope") or args.get("scope") or "global",
            "profile_owner": args.get("profile_owner") or args.get("owner_id") or "",
        }
    )


def _prefixed_profile_ref_from_args(
    args: Dict[str, Any],
    *,
    prefix: str,
    profile_id_key: Optional[str] = None,
) -> ActorProfileRef:
    key = profile_id_key or f"{prefix}profile_id"
    return normalize_actor_profile_ref(
        {
            "profile_id": args.get(key),
            "profile_scope": args.get(f"{prefix}profile_scope") or args.get(f"{prefix}scope") or "global",
            "profile_owner": args.get(f"{prefix}profile_owner") or args.get(f"{prefix}owner_id") or "",
        }
    )


def _resolve_secret_profile_access(
    args: Dict[str, Any],
    *,
    prefix: str = "",
    profile_id_key: Optional[str] = None,
    missing_code: str = "missing_profile_id",
    missing_message: str = "missing profile_id",
) -> ActorProfileRef | DaemonResponse:
    try:
        ref = (
            _prefixed_profile_ref_from_args(args, prefix=prefix, profile_id_key=profile_id_key)
            if prefix
            else _profile_ref_from_args(args, profile_id_key=profile_id_key or "profile_id")
        )
    except ValueError as e:
        msg = str(e)
        if "missing profile_id" in msg:
            return _error(missing_code, missing_message)
        return _error("invalid_request", msg)

    caller_id, is_admin, _ = _caller_context(args)
    if ref.profile_scope == "global" and not is_admin:
        return _error("permission_denied", "global profile secrets require admin access")
    if ref.profile_scope == "user" and not is_admin and ref.profile_owner != caller_id:
        return _error("permission_denied", "cannot access another user's profile secrets")

    resolver = ProfileResolver()
    profile = resolver.resolve(ref, caller_id=caller_id, is_admin=is_admin)
    if profile is None:
        return _error("profile_not_found", f"profile not found: {ref.profile_id}")
    return ref


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
            ref = actor_profile_ref(actor)
            if not aid or ref is None:
                continue
            usage_key = f"{ref.profile_scope}:{ref.profile_owner}:{ref.profile_id}"
            usage.setdefault(usage_key, []).append(
                {
                    "group_id": group.group_id,
                    "group_title": group_title,
                    "actor_id": aid,
                    "actor_title": actor_title,
                    "profile_scope": ref.profile_scope,
                    "profile_owner": ref.profile_owner,
                }
            )
    return usage


def _profile_usage_key(ref: ActorProfileRef | Dict[str, Any] | str) -> str:
    normalized = normalize_actor_profile_ref(ref)
    return f"{normalized.profile_scope}:{normalized.profile_owner}:{normalized.profile_id}"


def handle_actor_profile_list(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if by != "user" and not by:
        return _error("permission_denied", "invalid caller")
    try:
        caller_id, is_admin, explicit = _caller_context(args)
        view = str(args.get("view") or "global").strip().lower() or "global"
        if explicit and view == "all" and not is_admin:
            return _error("permission_denied", "admin access required for view=all")
        resolver = ProfileResolver()
        usage = _profile_usage_map()
        profiles = []
        for model in resolver.list_profiles(view, caller_id=caller_id, is_admin=is_admin):
            item = model.model_dump(exclude_none=True)
            out = dict(item)
            out["usage_count"] = len(usage.get(_profile_usage_key(item), []))
            profiles.append(out)
        return DaemonResponse(ok=True, result={"profiles": profiles})
    except Exception as e:
        return _error("actor_profile_list_failed", str(e))


def handle_actor_profile_get(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if by != "user" and not by:
        return _error("permission_denied", "invalid caller")
    if not str(args.get("profile_id") or "").strip():
        return _error("missing_profile_id", "missing profile_id")
    try:
        caller_id, is_admin, _ = _caller_context(args)
        ref = _profile_ref_from_args(args)
        if ref.profile_scope == "user" and not is_admin and ref.profile_owner != caller_id:
            return _error("permission_denied", "cannot access another user's profile")
        resolver = ProfileResolver()
        profile = resolver.resolve(ref, caller_id=caller_id, is_admin=is_admin)
        if profile is None:
            return _error("profile_not_found", f"profile not found: {ref.profile_id}")
        usage = _profile_usage_map().get(_profile_usage_key(ref), [])
        return DaemonResponse(ok=True, result={"profile": profile.model_dump(exclude_none=True), "usage": usage})
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
        caller_id, is_admin, _ = _caller_context(args)
        payload = dict(profile)
        env_raw = payload.get("env")
        if env_raw is not None:
            if not isinstance(env_raw, dict):
                return _error("invalid_request", "profile.env must be an object")
            has_non_empty_env = any(
                isinstance(key, str) and str(key).strip() and (value is not None and str(value).strip())
                for key, value in env_raw.items()
            )
            if has_non_empty_env:
                return _error(
                    "invalid_request",
                    "profile.env is deprecated; use actor_profile_secret_update to manage profile secrets",
                )

        # Unified model: runtime fields in profile + all variables in profile secrets.
        payload["env"] = {}
        resolver = ProfileResolver()
        saved = resolver.save_profile(
            payload,
            caller_id=caller_id,
            is_admin=is_admin,
            expected_revision=expected_revision,
        )
        if not saved:
            return _error("permission_denied", "profile write denied")
        target_ref = normalize_actor_profile_ref(payload)
        updated = resolver.resolve(target_ref, caller_id=caller_id, is_admin=True)
        if updated is None:
            return _error("actor_profile_upsert_failed", "saved profile could not be reloaded")
        return DaemonResponse(ok=True, result={"profile": updated.model_dump(exclude_none=True)})
    except ProfileRevisionMismatchError as e:
        return _error("profile_revision_mismatch", str(e))
    except Exception as e:
        return _error("actor_profile_upsert_failed", str(e))


def handle_actor_profile_delete(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if not _is_user_writer(by):
        return _error("permission_denied", "only user can delete actor profiles")
    force_detach = coerce_bool(args.get("force_detach"), default=False)
    if not str(args.get("profile_id") or "").strip():
        return _error("missing_profile_id", "missing profile_id")
    try:
        caller_id, is_admin, _ = _caller_context(args)
        ref = _profile_ref_from_args(args)
        resolver = ProfileResolver()
        profile = resolver.resolve(ref, caller_id=caller_id, is_admin=True)
        if profile is None:
            return _error("profile_not_found", f"profile not found: {ref.profile_id}")
        if not is_admin:
            if profile.scope == "global":
                return _error("permission_denied", "profile delete denied")
            if profile.owner_id != caller_id:
                return _error("permission_denied", "profile delete denied")
        usage = _profile_usage_map().get(_profile_usage_key(ref), [])
        if usage and not force_detach:
            return _error(
                "profile_in_use",
                "profile is in use by linked actors",
                details={"profile_id": ref.profile_id, "usage": usage},
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
                actor_ref = actor_profile_ref(actor)
                if actor_ref is None or _profile_usage_key(actor_ref) != _profile_usage_key(ref):
                    continue
                apply_profile_link_to_actor(
                    group,
                    actor_id,
                    profile_id=ref.profile_id,
                    profile_ref=ref,
                    profile=profile.model_dump(exclude_none=True),
                    load_actor_profile_secrets=load_actor_profile_secrets,
                    update_actor_private_env=update_actor_private_env,
                )
                clear_actor_link_metadata(group, actor_id)
                detached.append({"group_id": group_id, "actor_id": actor_id})
        deleted = resolver.delete_profile(ref, caller_id=caller_id, is_admin=is_admin)
        if not deleted:
            return _error("permission_denied", "profile delete denied")
        delete_actor_profile_secrets(
            {
                "profile_id": ref.profile_id,
                "profile_scope": profile.scope,
                "profile_owner": profile.owner_id,
            }
        )
        return DaemonResponse(
            ok=True,
            result={
                "deleted": True,
                "profile_id": ref.profile_id,
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
    resolved = _resolve_secret_profile_access(args)
    if isinstance(resolved, DaemonResponse):
        return resolved
    try:
        private_env = load_actor_profile_secrets(resolved)
        keys = sorted(private_env.keys())
        masked_values = {key: mask_private_env_value(value) for key, value in private_env.items()}
        return DaemonResponse(
            ok=True,
            result={
                "profile_id": resolved.profile_id,
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
    resolved = _resolve_secret_profile_access(args)
    if isinstance(resolved, DaemonResponse):
        return resolved

    clear = bool(args.get("clear") is True)
    set_raw = args.get("set")
    unset_raw = args.get("unset")

    set_vars: Dict[str, str] = {}
    unset_keys: List[str] = []

    try:
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
            resolved,
            set_vars=set_vars,
            unset_keys=unset_keys,
            clear=clear,
        )
        if len(updated) > PRIVATE_ENV_MAX_KEYS:
            update_actor_profile_secrets(resolved, set_vars={}, unset_keys=list(updated.keys()), clear=True)
            return _error("too_many_keys", "too many profile secret keys configured")
        keys = sorted(updated.keys())
        return DaemonResponse(ok=True, result={"profile_id": resolved.profile_id, "keys": keys})
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
        resolved = _resolve_secret_profile_access(args)
        if isinstance(resolved, DaemonResponse):
            return resolved

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
            resolved,
            set_vars=merged,
            unset_keys=[],
            clear=True,
        )
        keys = sorted(updated.keys())
        return DaemonResponse(
            ok=True,
            result={"profile_id": resolved.profile_id, "group_id": group_id, "actor_id": actor_id, "keys": keys},
        )
    except Exception as e:
        return _error("actor_profile_secret_copy_from_actor_failed", str(e))


def handle_actor_profile_secret_copy_from_profile(args: Dict[str, Any]) -> DaemonResponse:
    by = str(args.get("by") or "user").strip()
    if not _is_user_writer(by):
        return _error("permission_denied", "only user can copy profile secrets from another profile")

    target_ref = _resolve_secret_profile_access(args)
    if isinstance(target_ref, DaemonResponse):
        return target_ref
    source_ref = _resolve_secret_profile_access(
        args,
        prefix="source_",
        profile_id_key="source_profile_id",
        missing_code="missing_source_profile_id",
        missing_message="missing source_profile_id",
    )
    if isinstance(source_ref, DaemonResponse):
        return source_ref

    try:
        source_private = load_actor_profile_secrets(source_ref)
        if len(source_private) > PRIVATE_ENV_MAX_KEYS:
            return _error("too_many_keys", "too many profile secret keys configured on source profile")

        updated = update_actor_profile_secrets(
            target_ref,
            set_vars=source_private,
            unset_keys=[],
            clear=True,
        )
        keys = sorted(updated.keys())
        return DaemonResponse(
            ok=True,
            result={
                "profile_id": target_ref.profile_id,
                "source_profile_id": source_ref.profile_id,
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
