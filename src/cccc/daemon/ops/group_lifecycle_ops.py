"""Group lifecycle operation handlers for daemon."""

from __future__ import annotations

from pathlib import Path
from typing import Any, Callable, Dict, Optional, Sequence

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import list_actors, update_actor
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.permissions import require_group_permission
from ...runners import headless as headless_runner
from ...runners import pty as pty_runner


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_group_start(
    args: Dict[str, Any],
    *,
    pty_supported: Callable[[], bool],
    effective_runner_kind: Callable[[str], str],
    find_scope_url: Callable[[Any, str], str],
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
    reset_automation_timers_if_active: Callable[[Any], None],
    supported_runtimes: Sequence[str],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if not group_scope_key:
        return _error(
            "missing_project_root",
            "missing project root for group (no active scope)",
            details={"hint": "Attach a project root first (e.g. cccc attach <path> --group <id>)"},
        )
    try:
        require_group_permission(group, by=by, action="group.start")
        actors = list_actors(group)
        start_specs: list[tuple[str, Path, list[str], dict[str, str], Dict[str, Any], str]] = []
        for actor in actors:
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid:
                continue

            scope_key = str(actor.get("default_scope_key") or group_scope_key).strip()
            url = find_scope_url(group, scope_key)
            if not url:
                return _error(
                    "scope_not_attached",
                    f"scope not attached: {scope_key}",
                    details={
                        "group_id": group.group_id,
                        "actor_id": aid,
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
                        "actor_id": aid,
                        "scope_key": scope_key,
                        "path": str(cwd),
                        "hint": "Re-attach a valid project root (cccc attach <path> --group <id>)",
                    },
                )
            cmd = actor.get("command") if isinstance(actor.get("command"), list) else []
            env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
            runner_kind = str(actor.get("runner") or "pty").strip()
            start_specs.append((aid, cwd, list(cmd or []), dict(env or {}), dict(actor), runner_kind))

        started: list[str] = []
        forced_headless: list[str] = []
        for aid, cwd, cmd, env, actor, runner_kind in start_specs:
            try:
                update_actor(group, aid, {"enabled": True})
            except Exception:
                pass

            runner_effective = effective_runner_kind(runner_kind)
            if runner_effective == "headless" and runner_kind != "headless" and not pty_supported():
                forced_headless.append(aid)

            runtime = str(actor.get("runtime") or "codex").strip()
            if runtime not in supported_runtimes:
                return _error(
                    "unsupported_runtime",
                    f"unsupported runtime: {runtime}",
                    details={
                        "group_id": group.group_id,
                        "actor_id": aid,
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
                        "actor_id": aid,
                        "runtime": runtime,
                        "hint": "Set actor.command (or switch runner to headless).",
                    },
                )
            ensure_mcp_installed(runtime, cwd)

            effective_env = merge_actor_env_with_private(group.group_id, aid, env)
            if runner_effective == "headless":
                headless_runner.SUPERVISOR.start_actor(
                    group_id=group.group_id,
                    actor_id=aid,
                    cwd=cwd,
                    env=dict(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=aid)),
                )
                try:
                    write_headless_state(group.group_id, aid)
                except Exception:
                    pass
            else:
                effective_cmd = normalize_runtime_command(runtime, list(cmd or []))
                session = pty_runner.SUPERVISOR.start_actor(
                    group_id=group.group_id,
                    actor_id=aid,
                    cwd=cwd,
                    command=effective_cmd,
                    env=prepare_pty_env(inject_actor_context_env(effective_env, group_id=group.group_id, actor_id=aid)),
                    max_backlog_bytes=pty_backlog_bytes(),
                )
                try:
                    write_pty_state(group.group_id, aid, pid=session.pid)
                except Exception:
                    pass

            clear_preamble_sent(group, aid)
            throttle_reset_actor(group.group_id, aid, keep_pending=True)
            started.append(aid)
    except Exception as e:
        return _error("group_start_failed", str(e))

    if started:
        try:
            group.doc["running"] = True
            group.save()
        except Exception:
            pass
        reset_automation_timers_if_active(group)

    data: Dict[str, Any] = {"started": started}
    if forced_headless:
        data["forced_headless"] = forced_headless
    event = append_event(group.ledger_path, kind="group.start", group_id=group.group_id, scope_key="", by=by, data=data)
    result: Dict[str, Any] = {"group_id": group.group_id, "started": started, "event": event}
    if forced_headless:
        result["forced_headless"] = forced_headless
    return DaemonResponse(ok=True, result=result)


def handle_group_stop(
    args: Dict[str, Any],
    *,
    pty_state_dir_for_group: Callable[[str], Path],
    headless_state_dir_for_group: Callable[[str], Path],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    try:
        require_group_permission(group, by=by, action="group.stop")
        actors = list_actors(group)
        stopped: list[str] = []
        for actor in actors:
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid:
                continue
            try:
                update_actor(group, aid, {"enabled": False})
                stopped.append(aid)
            except Exception:
                pass

        pty_runner.SUPERVISOR.stop_group(group_id=group.group_id)
        headless_runner.SUPERVISOR.stop_group(group_id=group.group_id)

        try:
            pdir = pty_state_dir_for_group(group.group_id)
            for file_path in pdir.glob("*.json"):
                try:
                    file_path.unlink()
                except Exception:
                    pass
        except Exception:
            pass
        try:
            hdir = headless_state_dir_for_group(group.group_id)
            for file_path in hdir.glob("*.json"):
                try:
                    file_path.unlink()
                except Exception:
                    pass
        except Exception:
            pass
    except Exception as e:
        return _error("group_stop_failed", str(e))
    try:
        group.doc["running"] = False
        group.save()
    except Exception:
        pass
    event = append_event(group.ledger_path, kind="group.stop", group_id=group.group_id, scope_key="", by=by, data={"stopped": stopped})
    return DaemonResponse(ok=True, result={"group_id": group.group_id, "stopped": stopped, "event": event})


def try_handle_group_lifecycle_op(
    op: str,
    args: Dict[str, Any],
    *,
    pty_supported: Callable[[], bool],
    effective_runner_kind: Callable[[str], str],
    find_scope_url: Callable[[Any, str], str],
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
    reset_automation_timers_if_active: Callable[[Any], None],
    supported_runtimes: Sequence[str],
    pty_state_dir_for_group: Callable[[str], Path],
    headless_state_dir_for_group: Callable[[str], Path],
) -> Optional[DaemonResponse]:
    if op == "group_start":
        return handle_group_start(
            args,
            pty_supported=pty_supported,
            effective_runner_kind=effective_runner_kind,
            find_scope_url=find_scope_url,
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
            reset_automation_timers_if_active=reset_automation_timers_if_active,
            supported_runtimes=supported_runtimes,
        )
    if op == "group_stop":
        return handle_group_stop(
            args,
            pty_state_dir_for_group=pty_state_dir_for_group,
            headless_state_dir_for_group=headless_state_dir_for_group,
        )
    return None
