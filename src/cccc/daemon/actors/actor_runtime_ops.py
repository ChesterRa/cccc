"""Actor runtime startup helpers shared by daemon actor lifecycle flows."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, TypedDict

from ...kernel.actors import find_actor
from ...kernel.context import ContextStorage
from ...kernel.ledger import append_event
from ...kernel.runtime import runtime_start_preflight_error
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner
from ...runners.platform_support import pty_support_error_message
from ..pet.review_scheduler import request_pet_review


class ActorLaunchConfig(TypedDict):
    actor: Dict[str, Any]
    runtime: str
    runner: str
    effective_runner: str
    command: List[str]
    public_env: Dict[str, str]
    private_env: Dict[str, str]
    merged_env: Dict[str, Any]
    default_scope_key: str
    submit: str


class ActorLaunchSpec(ActorLaunchConfig):
    scope_key: str
    cwd: Path
    effective_command: List[str]


def model_from_runtime_command(command: List[str]) -> str:
    """Return an explicitly configured model from a runtime command."""
    items = [str(item or "").strip() for item in list(command or [])]
    for idx, item in enumerate(items):
        if not item:
            continue
        if item in {"-m", "--model"}:
            if idx + 1 < len(items):
                return str(items[idx + 1] or "").strip()
            return ""
        if item.startswith("--model="):
            return item.split("=", 1)[1].strip()
    return ""


def _coerce_string_env(raw: Any) -> Dict[str, str]:
    if not isinstance(raw, dict):
        return {}
    out: Dict[str, str] = {}
    for key, value in raw.items():
        if isinstance(key, str) and isinstance(value, str):
            out[str(key)] = str(value)
    return out


def _coerce_command(raw: Any, fallback: List[str]) -> List[str]:
    source = raw if isinstance(raw, list) else fallback
    return [str(item) for item in source if isinstance(item, str) and str(item).strip()]


def resolve_actor_launch_config(
    group: Any,
    actor_id: str,
    *,
    command: List[str],
    env: Dict[str, str],
    runner: str,
    runtime: str,
    effective_runner_kind: Callable[[str], str],
    caller_id: str = "",
    is_admin: bool = False,
    resolve_linked_actor_before_start: Optional[Callable[[Any, str], Dict[str, Any]]] = None,
    load_actor_private_env: Optional[Callable[[str, str], Dict[str, str]]] = None,
    merge_actor_env_with_private: Optional[Callable[[str, str, Dict[str, Any]], Dict[str, Any]]] = None,
) -> ActorLaunchConfig:
    actor = find_actor(group, actor_id)
    if actor is None:
        raise ValueError(f"actor not found: {actor_id}")
    if callable(resolve_linked_actor_before_start):
        actor = resolve_linked_actor_before_start(
            group,
            actor_id,
            caller_id=str(caller_id or "").strip(),
            is_admin=bool(is_admin),
        )

    resolved_command = _coerce_command(actor.get("command"), list(command or []))
    public_env = _coerce_string_env(actor.get("env")) if isinstance(actor.get("env"), dict) else _coerce_string_env(env)
    resolved_runner = str(actor.get("runner") or runner or "pty").strip() or "pty"
    resolved_runtime = str(actor.get("runtime") or runtime or "codex").strip() or "codex"
    if resolved_runtime == "web_model":
        resolved_runner = "headless"
        resolved_command = []
    effective_runner = effective_runner_kind(resolved_runner)
    if resolved_runtime == "custom" and effective_runner != "headless" and not resolved_command:
        raise ValueError("custom runtime requires a command (PTY runner)")

    private_env = (
        _coerce_string_env(load_actor_private_env(group.group_id, actor_id))
        if callable(load_actor_private_env)
        else {}
    )
    if callable(merge_actor_env_with_private):
        merged_env = dict(merge_actor_env_with_private(group.group_id, actor_id, public_env))
    elif private_env:
        merged_env = dict(public_env)
        merged_env.update(private_env)
    else:
        merged_env = dict(public_env)

    return {
        "actor": dict(actor),
        "runtime": resolved_runtime,
        "runner": resolved_runner,
        "effective_runner": effective_runner,
        "command": resolved_command,
        "public_env": public_env,
        "private_env": private_env,
        "merged_env": merged_env,
        "default_scope_key": str(actor.get("default_scope_key") or "").strip(),
        "submit": str(actor.get("submit") or "enter").strip() or "enter",
    }


def resolve_actor_launch_spec(
    group: Any,
    actor_id: str,
    *,
    command: List[str],
    env: Dict[str, str],
    runner: str,
    runtime: str,
    find_scope_url: Callable[[Any, str], str],
    effective_runner_kind: Callable[[str], str],
    normalize_runtime_command: Callable[[str, List[str]], List[str]],
    supported_runtimes: tuple[str, ...] | list[str],
    caller_id: str = "",
    is_admin: bool = False,
    resolve_linked_actor_before_start: Optional[Callable[[Any, str], Dict[str, Any]]] = None,
    load_actor_private_env: Optional[Callable[[str, str], Dict[str, str]]] = None,
    merge_actor_env_with_private: Optional[Callable[[str, str, Dict[str, Any]], Dict[str, Any]]] = None,
) -> ActorLaunchSpec:
    launch_config = resolve_actor_launch_config(
        group,
        actor_id,
        command=command,
        env=env,
        runner=runner,
        runtime=runtime,
        effective_runner_kind=effective_runner_kind,
        caller_id=caller_id,
        is_admin=is_admin,
        resolve_linked_actor_before_start=resolve_linked_actor_before_start,
        load_actor_private_env=load_actor_private_env,
        merge_actor_env_with_private=merge_actor_env_with_private,
    )

    group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if not group_scope_key:
        raise ValueError("no active scope for group")

    scope_key = str(launch_config["default_scope_key"] or group_scope_key).strip()
    url = find_scope_url(group, scope_key)
    if not url:
        raise ValueError(f"scope not attached: {scope_key}")
    cwd = Path(url).expanduser().resolve()
    if not cwd.exists():
        raise ValueError(f"project root path does not exist: {cwd}")

    if launch_config["runtime"] not in supported_runtimes:
        raise ValueError(f"unsupported runtime: {launch_config['runtime']}")

    effective_command = normalize_runtime_command(launch_config["runtime"], list(launch_config["command"] or []))
    return {
        **launch_config,
        "scope_key": scope_key,
        "cwd": cwd,
        "effective_command": effective_command,
    }


def start_actor_process(
    group: Any,
    actor_id: str,
    *,
    command: List[str],
    env: Dict[str, str],
    runner: str,
    runtime: str,
    by: str,
    caller_id: str = "",
    is_admin: bool = False,
    find_scope_url: Callable[[Any, str], str],
    effective_runner_kind: Callable[[str], str],
    merge_actor_env_with_private: Callable[[str, str, Dict[str, Any]], Dict[str, Any]],
    normalize_runtime_command: Callable[[str, List[str]], List[str]],
    ensure_mcp_installed: Callable[..., bool],
    inject_actor_context_env: Callable[[Dict[str, Any], str, str], Dict[str, Any]],
    prepare_pty_env: Callable[[Dict[str, Any]], Dict[str, str]],
    pty_backlog_bytes: Callable[[], int],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[[str, str, int], None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[[str, str], None],
    supported_runtimes: tuple[str, ...],
    resolve_linked_actor_before_start: Optional[Callable[[Any, str], Dict[str, Any]]] = None,
    load_actor_private_env: Optional[Callable[[str, str], Dict[str, str]]] = None,
) -> Dict[str, Any]:
    try:
        launch_spec = resolve_actor_launch_spec(
            group,
            actor_id,
            command=command,
            env=env,
            runner=runner,
            runtime=runtime,
            find_scope_url=find_scope_url,
            effective_runner_kind=effective_runner_kind,
            normalize_runtime_command=normalize_runtime_command,
            supported_runtimes=supported_runtimes,
            caller_id=caller_id,
            is_admin=is_admin,
            resolve_linked_actor_before_start=resolve_linked_actor_before_start,
            load_actor_private_env=load_actor_private_env,
            merge_actor_env_with_private=merge_actor_env_with_private,
        )
    except Exception as e:
        return {"success": False, "error": str(e)}

    actor = launch_spec["actor"]
    effective_runner = launch_spec["effective_runner"]
    effective_env = dict(launch_spec["merged_env"])
    effective_cmd = launch_spec["effective_command"]
    cwd = launch_spec["cwd"]
    runtime = launch_spec["runtime"]
    runner = launch_spec["runner"]

    if effective_runner != "headless":
        if not bool(getattr(pty_runner, "PTY_SUPPORTED", False)):
            error_message = pty_support_error_message() or "PTY runner is not supported in this environment."
            return {"success": False, "error": error_message}
        try:
            mcp_ready = bool(
                ensure_mcp_installed(
                    runtime,
                    cwd,
                    env={str(k): str(v) for k, v in effective_env.items() if isinstance(k, str)},
                )
            )
        except Exception as e:
            return {"success": False, "error": f"failed to install MCP: {e}"}
        if not mcp_ready:
            return {"success": False, "error": f"failed to install MCP for runtime: {runtime}"}

    runtime_error = runtime_start_preflight_error(runtime, effective_cmd, runner=effective_runner)
    if runtime_error:
        return {"success": False, "error": runtime_error}

    try:
        if runtime == "web_model" and effective_runner == "headless":
            try:
                write_headless_state(group.group_id, actor_id)
            except Exception:
                pass
        elif runtime == "codex" and effective_runner == "headless":
            codex_app_supervisor.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                env=dict(inject_actor_context_env(effective_env, group.group_id, actor_id)),
                model=model_from_runtime_command(effective_cmd),
            )
        elif runtime == "claude" and effective_runner == "headless":
            claude_app_supervisor.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                env=dict(inject_actor_context_env(effective_env, group.group_id, actor_id)),
                model=model_from_runtime_command(effective_cmd),
            )
        elif effective_runner == "headless":
            headless_runner.SUPERVISOR.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                env=dict(inject_actor_context_env(effective_env, group.group_id, actor_id)),
            )
            try:
                write_headless_state(group.group_id, actor_id)
            except Exception:
                pass
        else:
            session = pty_runner.SUPERVISOR.start_actor(
                group_id=group.group_id,
                actor_id=actor_id,
                cwd=cwd,
                command=effective_cmd,
                env=prepare_pty_env(inject_actor_context_env(effective_env, group.group_id, actor_id)),
                runtime=runtime,
                max_backlog_bytes=pty_backlog_bytes(),
            )
            try:
                write_pty_state(group.group_id, actor_id, session.pid)
            except Exception:
                pass
    except Exception as e:
        return {"success": False, "error": f"failed to start session: {e}"}

    clear_preamble_sent(group, actor_id)
    throttle_reset_actor(group.group_id, actor_id)
    try:
        ContextStorage(group).clear_agent_status_if_present(actor_id)
    except Exception:
        pass

    try:
        if str(group.doc.get("state") or "").strip() == "stopped":
            group.doc["state"] = "active"
        group.doc["running"] = True
        group.save()
    except Exception:
        pass

    start_data: Dict[str, Any] = {"actor_id": actor_id, "runner": runner}
    if effective_runner != runner:
        start_data["runner_effective"] = effective_runner
    start_event = append_event(
        group.ledger_path,
        kind="actor.start",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data=start_data,
    )

    from ...kernel.events import publish_event
    publish_event("actor.start", {"group_id": group.group_id, "actor_id": actor_id})
    try:
        request_pet_review(
            group.group_id,
            reason="actor_start",
            source_event_id=str(start_event.get("id") or "").strip(),
            immediate=True,
        )
    except Exception:
        pass

    return {
        "success": True,
        "actor": actor,
        "event": start_event,
        "effective_runner": effective_runner,
        "error": None,
    }
