"""Actor list/private-env operation handlers for daemon."""

from __future__ import annotations

from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ..actor_runtime_cache import get_group_runtime
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ...kernel.actors import find_actor, get_effective_role, list_actors
from ...kernel.context import ContextStorage
from ...kernel.group import load_group
from ...kernel.query_projections import get_actor_list_projection
from ...kernel.working_state import DEFAULT_PTY_TERMINAL_SIGNAL_TAIL_BYTES, derive_effective_working_state
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ..context.context_ops import _agent_state_to_dict
from .private_env_ops import mask_private_env_value
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
    include_internal = coerce_bool(args.get("include_internal"), default=False)
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if include_internal:
        actors = []
        for actor in list_actors(group):
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid:
                continue
            item = dict(actor)
            item["role"] = get_effective_role(group, aid)
            actors.append(item)
    else:
        actors = get_actor_list_projection(group)
    runtime_snapshot = get_group_runtime(group_id)
    storage = ContextStorage(group)
    agent_rows = [_agent_state_to_dict(agent) for agent in storage.load_agents().agents]
    agent_state_by_id = {
        str(item.get("id") or "").strip(): item
        for item in agent_rows
        if isinstance(item, dict) and str(item.get("id") or "").strip()
    } if isinstance(agent_rows, list) else {}
    for actor in actors:
        aid = str(actor.get("id") or "")
        if not aid:
            continue
        runner_kind = str(actor.get("runner") or "pty").strip()
        snap = runtime_snapshot.get(aid) if isinstance(runtime_snapshot.get(aid), dict) else {}
        if snap:
            actor["running"] = bool(snap.get("running"))
            actor["idle_seconds"] = snap.get("idle_seconds")
            snap_runner_effective = str(snap.get("runner_effective") or runner_kind or "pty")
            if snap_runner_effective == "headless" or snap_runner_effective != runner_kind:
                actor["runner_effective"] = snap_runner_effective
            else:
                actor.pop("runner_effective", None)
            for key in (
                "effective_working_state",
                "effective_working_reason",
                "effective_working_updated_at",
                "effective_active_task_id",
            ):
                if key in snap:
                    actor[key] = snap.get(key)
            continue
        effective_runner = effective_runner_kind(runner_kind)
        runtime = str(actor.get("runtime") or "").strip()
        headless_state = None
        running = False
        idle_seconds = None
        pty_terminal_text = ""
        if runtime.lower() == "codex" and effective_runner == "headless":
            state = codex_app_supervisor.get_state(group_id=group_id, actor_id=aid)
            headless_state = state.model_dump() if hasattr(state, "model_dump") else (dict(state) if isinstance(state, dict) else None)
            running = bool(state is not None and codex_app_supervisor.actor_running(group_id, aid))
        elif runtime.lower() == "claude" and effective_runner == "headless":
            state = claude_app_supervisor.get_state(group_id=group_id, actor_id=aid)
            headless_state = dict(state) if isinstance(state, dict) else None
            running = bool(state is not None and claude_app_supervisor.actor_running(group_id, aid))
        elif effective_runner == "headless":
            state = headless_runner.SUPERVISOR.get_state(group_id=group_id, actor_id=aid)
            headless_state = state.model_dump() if hasattr(state, "model_dump") else (dict(state) if isinstance(state, dict) else None)
            running = bool(state is not None and headless_runner.SUPERVISOR.actor_running(group_id, aid))
        else:
            running = bool(pty_runner.SUPERVISOR.actor_running(group_id, aid))
            idle_seconds = pty_runner.SUPERVISOR.idle_seconds(group_id=group_id, actor_id=aid) if running else None
            if running:
                try:
                    pty_terminal_text = pty_runner.SUPERVISOR.tail_output(
                        group_id=group_id,
                        actor_id=aid,
                        max_bytes=DEFAULT_PTY_TERMINAL_SIGNAL_TAIL_BYTES,
                    ).decode("utf-8", errors="replace")
                except Exception:
                    pty_terminal_text = ""
        actor["running"] = running
        actor["idle_seconds"] = idle_seconds
        if effective_runner == "headless" or effective_runner != runner_kind:
            actor["runner_effective"] = effective_runner
        else:
            actor.pop("runner_effective", None)
        actor.update(
            derive_effective_working_state(
                running=running,
                effective_runner=effective_runner,
                runtime=runtime,
                idle_seconds=idle_seconds,
                pty_terminal_text=pty_terminal_text,
                agent_state=agent_state_by_id.get(aid),
                headless_state=headless_state,
            )
        )
    if include_unread:
        from ...kernel.inbox import get_indexed_unread_counts

        counts = get_indexed_unread_counts(group, actors=actors)
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
    actor = find_actor(group, actor_id)
    if actor is None:
        return _error("actor_not_found", f"actor not found: {actor_id}")
    if str(actor.get("profile_id") or "").strip():
        return _error(
            "actor_profile_linked_readonly",
            "linked actor private env is profile-controlled (convert to custom first)",
        )
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
    actor = find_actor(group, actor_id)
    if actor is None:
        return _error("actor_not_found", f"actor not found: {actor_id}")
    if str(actor.get("profile_id") or "").strip():
        return _error(
            "actor_profile_linked_readonly",
            "linked actor private env is profile-controlled (convert to custom first)",
        )

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
