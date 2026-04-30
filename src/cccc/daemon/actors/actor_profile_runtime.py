from __future__ import annotations

import logging
from typing import Any, Callable, Dict, Optional

from ...kernel.actors import find_actor, update_actor
from ...contracts.v1 import ActorProfileRef
from .actor_profile_store import ProfileResolver, normalize_actor_profile_ref

PROFILE_CONTROLLED_FIELDS = ("runtime", "runner", "command", "submit", "env")
_LOG = logging.getLogger("cccc.daemon.actor_profile_runtime")


class ActorProfileNotFoundError(RuntimeError):
    pass


class ActorProfileAccessDeniedError(RuntimeError):
    pass


def actor_profile_id(actor: Dict[str, Any]) -> str:
    return str(actor.get("profile_id") or "").strip()


def actor_profile_ref(actor: Dict[str, Any]) -> Optional[ActorProfileRef]:
    """Return explicit profile ref only when scope is declared on the actor.

    Bare ``profile_id`` without ``profile_scope`` is treated as legacy data
    and must NOT be fabricated into an implicit global ref here — that compat
    belongs exclusively in ``_resolve_profile_for_start()``.
    """
    profile_id = actor_profile_id(actor)
    if not profile_id:
        return None
    raw_scope = str(actor.get("profile_scope") or "").strip()
    if not raw_scope:
        return None
    try:
        return normalize_actor_profile_ref(
            {
                "profile_id": profile_id,
                "profile_scope": raw_scope,
                "profile_owner": actor.get("profile_owner") or "",
            }
        )
    except Exception:
        return None


def actor_profile_ref_is_explicit(actor: Dict[str, Any]) -> bool:
    return actor_profile_ref(actor) is not None


def is_actor_profile_linked(actor: Dict[str, Any]) -> bool:
    return bool(actor_profile_id(actor))


def _resolve_profile_for_start(
    actor: Dict[str, Any],
    *,
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    caller_id: str = "",
    is_admin: bool = False,
) -> Optional[Dict[str, Any]]:
    ref = actor_profile_ref(actor)
    if ref is not None:
        profile = ProfileResolver().resolve(ref, caller_id=caller_id, is_admin=is_admin)
        if profile is None:
            if ref.profile_scope == "user" and not is_admin and ref.profile_owner != str(caller_id or "").strip():
                raise ActorProfileAccessDeniedError(
                    f"profile access denied: {ref.profile_id} (owned by {ref.profile_owner})"
                )
            return None
        return profile.model_dump(exclude_none=True)
    # Legacy compat: actor has bare profile_id but no constructible ref.
    # Treat as global — no caller-based fallback.
    pid = actor_profile_id(actor)
    if pid:
        return get_actor_profile(pid)
    return None


def _profile_patch(profile: Dict[str, Any]) -> Dict[str, Any]:
    runtime = str(profile.get("runtime") or "codex")
    runner = "headless" if runtime == "web_model" else str(profile.get("runner") or "pty")
    return {
        "runtime": runtime,
        "runner": runner,
        "command": [] if runtime == "web_model" else list(profile.get("command") or []),
        "submit": str(profile.get("submit") or "enter"),
        # Unified model: profile variables are secret env; keep actor.env empty.
        "env": {},
    }


def _profile_secret_ref(profile: Dict[str, Any], *, fallback_profile_id: str) -> Dict[str, str]:
    return {
        "profile_id": str(profile.get("id") or fallback_profile_id or "").strip(),
        "profile_scope": str(profile.get("scope") or "global").strip() or "global",
        "profile_owner": str(profile.get("owner_id") or "").strip(),
    }


def _same_profile_config(actor: Dict[str, Any], profile: Dict[str, Any]) -> bool:
    patch = _profile_patch(profile)
    actor_command = actor.get("command") if isinstance(actor.get("command"), list) else []
    actor_env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
    return (
        str(actor.get("runtime") or "codex") == str(patch["runtime"])
        and str(actor.get("runner") or "pty") == str(patch["runner"])
        and list(actor_command) == list(patch["command"])
        and str(actor.get("submit") or "enter") == str(patch["submit"])
        and dict(actor_env) == dict(patch["env"])
    )


def _set_actor_link_metadata(
    group: Any,
    actor_id: str,
    *,
    profile_id: str,
    revision: int,
    profile_ref: Optional[ActorProfileRef] = None,
) -> Dict[str, Any]:
    item = find_actor(group, actor_id)
    if item is None:
        raise ValueError(f"actor not found: {actor_id}")
    changed = False
    if str(item.get("profile_id") or "").strip() != profile_id:
        item["profile_id"] = profile_id
        changed = True
    if int(item.get("profile_revision_applied") or 0) != int(revision):
        item["profile_revision_applied"] = int(revision)
        changed = True
    if profile_ref is not None:
        desired_scope = str(profile_ref.profile_scope or "global").strip() or "global"
        desired_owner = str(profile_ref.profile_owner or "").strip()
        if str(item.get("profile_scope") or "").strip() != desired_scope:
            item["profile_scope"] = desired_scope
            changed = True
        if desired_scope == "user":
            if str(item.get("profile_owner") or "").strip() != desired_owner:
                item["profile_owner"] = desired_owner
                changed = True
        elif "profile_owner" in item:
            item.pop("profile_owner", None)
            changed = True
    else:
        if "profile_scope" in item:
            item.pop("profile_scope", None)
            changed = True
        if "profile_owner" in item:
            item.pop("profile_owner", None)
            changed = True
    if profile_ref is not None and profile_ref.profile_scope == "global" and "profile_owner" in item and str(item.get("profile_owner") or "").strip():
        item["profile_owner"] = ""
        changed = True
    if changed:
        group.save()
    return dict(item)


def clear_actor_link_metadata(group: Any, actor_id: str) -> Dict[str, Any]:
    item = find_actor(group, actor_id)
    if item is None:
        raise ValueError(f"actor not found: {actor_id}")
    changed = False
    if "profile_id" in item:
        item.pop("profile_id", None)
        changed = True
    if "profile_scope" in item:
        item.pop("profile_scope", None)
        changed = True
    if "profile_owner" in item:
        item.pop("profile_owner", None)
        changed = True
    if "profile_revision_applied" in item:
        item.pop("profile_revision_applied", None)
        changed = True
    if changed:
        group.save()
    return dict(item)


def apply_profile_link_to_actor(
    group: Any,
    actor_id: str,
    *,
    profile_id: str,
    profile_ref: ActorProfileRef | Dict[str, Any] | str | None = None,
    profile: Dict[str, Any],
    load_actor_profile_secrets: Callable[[Any], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> Dict[str, Any]:
    """Apply linked profile config + secrets to actor and keep revision metadata in sync."""
    item = find_actor(group, actor_id)
    if item is None:
        raise ValueError(f"actor not found: {actor_id}")

    if not _same_profile_config(item, profile):
        item = update_actor(group, actor_id, _profile_patch(profile))

    revision = int(profile.get("revision") or 0)
    explicit_ref = normalize_actor_profile_ref(profile_ref) if profile_ref is not None else None
    effective_ref = explicit_ref or normalize_actor_profile_ref(_profile_secret_ref(profile, fallback_profile_id=profile_id))
    item = _set_actor_link_metadata(
        group,
        actor_id,
        profile_id=profile_id,
        revision=revision,
        profile_ref=effective_ref,
    )

    merged_private = load_actor_profile_secrets(effective_ref)
    update_actor_private_env(
        group.group_id,
        actor_id,
        set_vars=merged_private,
        unset_keys=[],
        clear=True,
    )
    return item


def resolve_linked_actor_before_start(
    group: Any,
    actor_id: str,
    *,
    get_actor_profile: Callable[[str], Optional[Dict[str, Any]]],
    load_actor_profile_secrets: Callable[[Any], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
    caller_id: str = "",
    is_admin: bool = False,
) -> Dict[str, Any]:
    item = find_actor(group, actor_id)
    if item is None:
        raise ValueError(f"actor not found: {actor_id}")
    profile_id = actor_profile_id(item)
    profile: Optional[Dict[str, Any]] = None
    if profile_id:
        profile = _resolve_profile_for_start(
            item,
            get_actor_profile=get_actor_profile,
            caller_id=caller_id,
            is_admin=is_admin,
        )
        if not isinstance(profile, dict):
            raise ActorProfileNotFoundError(f"profile not found: {profile_id}")

        explicit_ref = actor_profile_ref(item)
        item = apply_profile_link_to_actor(
            group,
            actor_id,
            profile_id=profile_id,
            profile_ref=explicit_ref,
            profile=profile,
            load_actor_profile_secrets=load_actor_profile_secrets,
            update_actor_private_env=update_actor_private_env,
        )

    # Apply capability autoload at startup:
    # 0) role-based defaults (foreman gets management packs automatically)
    # 1) profile capability defaults (if linked)
    # 2) actor-specific autoload list
    try:
        from ..ops.capability_ops import (
            apply_actor_capability_autoload,
            apply_actor_profile_capability_defaults,
        )
        from ...kernel.actors import get_effective_role

        # Role-based default capabilities — foreman always gets management packs
        _ROLE_CAPABILITY_DEFAULTS: Dict[str, list] = {
            "foreman": [
                "pack:group-runtime",   # actor/group/runtime lifecycle
                "pack:diagnostics",     # terminal transcript + debug
            ],
        }
        role = get_effective_role(group, actor_id)
        role_defaults = _ROLE_CAPABILITY_DEFAULTS.get(role, [])
        if role_defaults:
            apply_actor_capability_autoload(
                group_id=group.group_id,
                actor_id=actor_id,
                autoload_capabilities=role_defaults,
                scope="actor",
                reason=f"role_default:{role}",
            )

        if isinstance(profile, dict):
            defaults_raw = profile.get("capability_defaults")
            if isinstance(defaults_raw, dict):
                apply_actor_profile_capability_defaults(
                    group_id=group.group_id,
                    actor_id=actor_id,
                    profile_id=profile_id,
                    capability_defaults=defaults_raw,
                )
        apply_actor_capability_autoload(
            group_id=group.group_id,
            actor_id=actor_id,
            autoload_capabilities=item.get("capability_autoload"),
            scope="actor",
            reason=f"actor_autoload:{actor_id}",
        )
    except Exception as e:
        _LOG.warning(
            "actor capability autoload apply failed group=%s actor=%s profile=%s err=%s",
            group.group_id,
            actor_id,
            profile_id,
            str(e),
        )

    return item
