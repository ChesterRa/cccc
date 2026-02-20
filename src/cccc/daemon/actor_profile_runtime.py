from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ..kernel.actors import find_actor, update_actor

PROFILE_CONTROLLED_FIELDS = ("runtime", "runner", "command", "submit", "env")


class ActorProfileNotFoundError(RuntimeError):
    pass


def actor_profile_id(actor: Dict[str, Any]) -> str:
    return str(actor.get("profile_id") or "").strip()


def is_actor_profile_linked(actor: Dict[str, Any]) -> bool:
    return bool(actor_profile_id(actor))


def _profile_patch(profile: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "runtime": str(profile.get("runtime") or "codex"),
        "runner": str(profile.get("runner") or "pty"),
        "command": list(profile.get("command") or []),
        "submit": str(profile.get("submit") or "enter"),
        # Unified model: profile variables are secret env; keep actor.env empty.
        "env": {},
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


def _set_actor_link_metadata(group: Any, actor_id: str, *, profile_id: str, revision: int) -> Dict[str, Any]:
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
    profile: Dict[str, Any],
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> Dict[str, Any]:
    """Apply linked profile config + secrets to actor and keep revision metadata in sync."""
    item = find_actor(group, actor_id)
    if item is None:
        raise ValueError(f"actor not found: {actor_id}")

    if not _same_profile_config(item, profile):
        item = update_actor(group, actor_id, _profile_patch(profile))

    revision = int(profile.get("revision") or 0)
    item = _set_actor_link_metadata(group, actor_id, profile_id=profile_id, revision=revision)

    profile_private = load_actor_profile_secrets(profile_id)
    legacy_public_env_raw = profile.get("env")
    legacy_public_env: Dict[str, str] = {}
    if isinstance(legacy_public_env_raw, dict):
        for key, value in legacy_public_env_raw.items():
            if not isinstance(key, str):
                continue
            k = key.strip()
            if not k or value is None:
                continue
            legacy_public_env[k] = str(value)
    merged_private: Dict[str, str] = {}
    merged_private.update(legacy_public_env)
    merged_private.update(profile_private)
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
    load_actor_profile_secrets: Callable[[str], Dict[str, str]],
    update_actor_private_env: Callable[..., Dict[str, str]],
) -> Dict[str, Any]:
    item = find_actor(group, actor_id)
    if item is None:
        raise ValueError(f"actor not found: {actor_id}")
    profile_id = actor_profile_id(item)
    if not profile_id:
        return dict(item)

    profile = get_actor_profile(profile_id)
    if not isinstance(profile, dict):
        raise ActorProfileNotFoundError(f"profile not found: {profile_id}")

    return apply_profile_link_to_actor(
        group,
        actor_id,
        profile_id=profile_id,
        profile=profile,
        load_actor_profile_secrets=load_actor_profile_secrets,
        update_actor_private_env=update_actor_private_env,
    )
