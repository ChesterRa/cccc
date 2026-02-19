"""Actor runtime startup helper shared by actor lifecycle operations."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Callable, Dict, List

from ...kernel.actors import find_actor
from ...kernel.ledger import append_event
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner


def start_actor_process(
    group: Any,
    actor_id: str,
    *,
    command: List[str],
    env: Dict[str, str],
    runner: str,
    runtime: str,
    by: str,
    find_scope_url: Callable[[Any, str], str],
    effective_runner_kind: Callable[[str], str],
    merge_actor_env_with_private: Callable[[str, str, Dict[str, Any]], Dict[str, Any]],
    normalize_runtime_command: Callable[[str, List[str]], List[str]],
    ensure_mcp_installed: Callable[[str, Path], bool],
    inject_actor_context_env: Callable[[Dict[str, Any], str, str], Dict[str, Any]],
    prepare_pty_env: Callable[[Dict[str, Any]], Dict[str, str]],
    pty_backlog_bytes: Callable[[], int],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[[str, str, int], None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[[str, str], None],
    supported_runtimes: tuple[str, ...],
) -> Dict[str, Any]:
    group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if not group_scope_key:
        return {"success": False, "error": "no active scope for group"}

    actor = find_actor(group, actor_id)
    if actor is None:
        return {"success": False, "error": f"actor not found: {actor_id}"}

    scope_key = str(actor.get("default_scope_key") or group_scope_key).strip()
    url = find_scope_url(group, scope_key)
    if not url:
        return {"success": False, "error": f"scope not attached: {scope_key}"}

    cwd = Path(url).expanduser().resolve()
    if not cwd.exists():
        return {"success": False, "error": f"project root path does not exist: {cwd}"}

    if runtime not in supported_runtimes:
        return {"success": False, "error": f"unsupported runtime: {runtime}"}

    effective_runner = effective_runner_kind(runner)
    effective_env = merge_actor_env_with_private(group.group_id, actor_id, env)

    if runtime == "custom" and effective_runner != "headless" and not command:
        return {"success": False, "error": "custom runtime requires a command (PTY runner)"}

    effective_cmd = normalize_runtime_command(runtime, list(command or []))

    try:
        ensure_mcp_installed(runtime, cwd)
    except Exception as e:
        return {"success": False, "error": f"failed to install MCP: {e}"}

    try:
        if effective_runner == "headless":
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

    return {
        "success": True,
        "event": start_event,
        "effective_runner": effective_runner,
        "error": None,
    }
