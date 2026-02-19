"""Actor list/private-env operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_actor, get_effective_role, list_actors
from ...kernel.group import load_group
from ..private_env_ops import mask_private_env_value
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...util.conv import coerce_bool


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_actor_list(
    args: Dict[str, Any],
    *,
    effective_runner_kind: Callable[[str], str],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    include_unread = coerce_bool(args.get("include_unread"), default=False)
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    actors = list_actors(group)
    for actor in actors:
        aid = str(actor.get("id") or "")
        if not aid:
            continue
        actor["role"] = get_effective_role(group, aid)
        runner_kind = str(actor.get("runner") or "pty").strip()
        effective_runner = effective_runner_kind(runner_kind)
        if effective_runner == "headless":
            actor["running"] = headless_runner.SUPERVISOR.actor_running(group_id, aid)
        else:
            actor["running"] = pty_runner.SUPERVISOR.actor_running(group_id, aid)
        if effective_runner != runner_kind:
            actor["runner_effective"] = effective_runner
    if include_unread:
        from ...kernel.inbox import batch_unread_counts

        actor_ids = [str(a.get("id") or "") for a in actors if a.get("id")]
        counts = batch_unread_counts(group, actor_ids=actor_ids)
        for actor in actors:
            aid = str(actor.get("id") or "")
            if aid:
                actor["unread_count"] = counts.get(aid, 0)
    return DaemonResponse(ok=True, result={"actors": actors})


def handle_actor_env_private_keys(
    args: Dict[str, Any],
    *,
    load_actor_private_env: Callable[[str, str], Dict[str, str]],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if by and by != "user":
        return _error("permission_denied", "only user can access private env metadata")
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if find_actor(group, actor_id) is None:
        return _error("actor_not_found", f"actor not found: {actor_id}")
    private_env = load_actor_private_env(group_id, actor_id)
    keys = sorted(private_env.keys())
    masked_values = {k: mask_private_env_value(v) for k, v in private_env.items()}
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group_id,
            "actor_id": actor_id,
            "keys": keys,
            "masked_values": masked_values,
        },
    )


def handle_actor_env_private_update(
    args: Dict[str, Any],
    *,
    validate_private_env_key: Callable[[Any], str],
    coerce_private_env_value: Callable[[Any], str],
    update_actor_private_env: Callable[..., Dict[str, str]],
    private_env_max_keys: int,
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if by and by != "user":
        return _error("permission_denied", "only user can update private env")
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not actor_id:
        return _error("missing_actor_id", "missing actor_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if find_actor(group, actor_id) is None:
        return _error("actor_not_found", f"actor not found: {actor_id}")

    clear = coerce_bool(args.get("clear"), default=False)
    set_raw = args.get("set")
    unset_raw = args.get("unset")

    set_vars: Dict[str, str] = {}
    unset_keys: list[str] = []

    try:
        if set_raw is not None:
            if not isinstance(set_raw, dict):
                raise ValueError("set must be an object")
            for key, value in set_raw.items():
                set_key = validate_private_env_key(key)
                set_value = coerce_private_env_value(value)
                set_vars[set_key] = set_value

        if unset_raw is not None:
            if not isinstance(unset_raw, list):
                raise ValueError("unset must be a list")
            for item in unset_raw:
                unset_keys.append(validate_private_env_key(item))
    except ValueError as e:
        return _error("invalid_request", str(e))

    if len(set_vars) > private_env_max_keys:
        return _error("too_many_keys", "too many env keys to set in one request")
    if len(unset_keys) > private_env_max_keys:
        return _error("too_many_keys", "too many env keys to unset in one request")

    try:
        updated = update_actor_private_env(
            group_id,
            actor_id,
            set_vars=set_vars,
            unset_keys=unset_keys,
            clear=clear,
        )
    except Exception:
        return _error("actor_env_private_update_failed", "failed to update private env")

    if len(updated) > private_env_max_keys:
        try:
            update_actor_private_env(group_id, actor_id, set_vars={}, unset_keys=list(updated.keys()), clear=True)
        except Exception:
            pass
        return _error("too_many_keys", "too many private env keys configured")

    keys = sorted(updated.keys())
    return DaemonResponse(ok=True, result={"group_id": group_id, "actor_id": actor_id, "keys": keys})


def try_handle_actor_aux_op(
    op: str,
    args: Dict[str, Any],
    *,
    effective_runner_kind: Callable[[str], str],
    load_actor_private_env: Optional[Callable[[str, str], Dict[str, str]]] = None,
    validate_private_env_key: Optional[Callable[[Any], str]] = None,
    coerce_private_env_value: Optional[Callable[[Any], str]] = None,
    update_actor_private_env: Optional[Callable[..., Dict[str, str]]] = None,
    private_env_max_keys: Optional[int] = None,
) -> Optional[DaemonResponse]:
    if op == "actor_list":
        return handle_actor_list(args, effective_runner_kind=effective_runner_kind)

    if op == "actor_env_private_keys":
        if load_actor_private_env is None:
            return _error("internal_error", "actor private env callbacks not configured")
        return handle_actor_env_private_keys(args, load_actor_private_env=load_actor_private_env)

    if op == "actor_env_private_update":
        if (
            validate_private_env_key is None
            or coerce_private_env_value is None
            or update_actor_private_env is None
            or private_env_max_keys is None
        ):
            return _error("internal_error", "actor private env callbacks not configured")
        return handle_actor_env_private_update(
            args,
            validate_private_env_key=validate_private_env_key,
            coerce_private_env_value=coerce_private_env_value,
            update_actor_private_env=update_actor_private_env,
            private_env_max_keys=private_env_max_keys,
        )

    return None
