"""Actor autostart bootstrap helpers."""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any, Callable

from ..kernel.actors import list_actors
from ..kernel.group import load_group
from ..util.conv import coerce_bool
from ..runners import headless as headless_runner
from ..runners import pty as pty_runner

logger = logging.getLogger("cccc.daemon.server")


def autostart_running_groups(
    home: Path,
    *,
    effective_runner_kind: Callable[[str], str],
    find_scope_url: Callable[[Any, str], str],
    supported_runtimes: tuple[str, ...],
    ensure_mcp_installed: Callable[[str, Path], bool],
    auto_mcp_runtimes: tuple[str, ...],
    pty_supported: Callable[[], bool],
    merge_actor_env_with_private: Callable[[str, str, dict[str, Any]], dict[str, Any]],
    inject_actor_context_env: Callable[[dict[str, Any], str, str], dict[str, Any]],
    prepare_pty_env: Callable[[dict[str, Any]], dict[str, str]],
    normalize_runtime_command: Callable[[str, list[str]], list[str]],
    pty_backlog_bytes: Callable[[], int],
    write_headless_state: Callable[[str, str], None],
    write_pty_state: Callable[[str, str, int], None],
    clear_preamble_sent: Callable[[Any, str], None],
    throttle_reset_actor: Callable[[str, str], None],
    automation_on_resume: Callable[[Any], None],
    get_group_state: Callable[[Any], str],
) -> None:
    base = home / "groups"
    if not base.exists():
        return

    for group_yaml in base.glob("*/group.yaml"):
        group_id = group_yaml.parent.name
        group = load_group(group_id)
        if group is None:
            continue
        if not coerce_bool(group.doc.get("running"), default=False):
            continue

        group_scope_key = str(group.doc.get("active_scope_key") or "").strip()
        if not group_scope_key:
            group.doc["running"] = False
            try:
                group.save()
            except Exception:
                pass
            continue

        for actor in list_actors(group):
            if not isinstance(actor, dict):
                continue
            actor_id = str(actor.get("id") or "").strip()
            if not actor_id:
                continue
            if not coerce_bool(actor.get("enabled"), default=True):
                continue

            runner_kind = str(actor.get("runner") or "pty").strip()
            effective_runner = effective_runner_kind(runner_kind)
            scope_key = str(actor.get("default_scope_key") or group_scope_key).strip()
            url = find_scope_url(group, scope_key)
            if not url:
                continue
            cwd = Path(url).expanduser().resolve()
            if not cwd.exists():
                continue

            command = actor.get("command") if isinstance(actor.get("command"), list) else []
            env = actor.get("env") if isinstance(actor.get("env"), dict) else {}
            runtime = str(actor.get("runtime") or "codex").strip()
            if runtime not in supported_runtimes:
                continue
            if runtime == "custom" and effective_runner != "headless" and not command:
                continue

            ok_mcp = True
            try:
                ok_mcp = bool(ensure_mcp_installed(runtime, cwd))
            except Exception:
                ok_mcp = False
            if not ok_mcp and runtime in auto_mcp_runtimes:
                logger.warning(
                    "MCP server 'cccc' is not installed for %s/%s (runtime=%s); actor will start but tools may not work.",
                    group_id,
                    actor_id,
                    runtime,
                )

            clear_preamble_sent(group, actor_id)

            try:
                if effective_runner == "headless":
                    if runner_kind != "headless" and not pty_supported():
                        logger.warning(
                            "pty runner is not supported on this platform; autostarting %s/%s as headless",
                            group_id,
                            actor_id,
                        )
                    effective_env = merge_actor_env_with_private(group.group_id, actor_id, env)
                    headless_runner.SUPERVISOR.start_actor(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        cwd=cwd,
                        env=dict(inject_actor_context_env(effective_env, group.group_id, actor_id)),
                    )
                else:
                    effective_env = merge_actor_env_with_private(group.group_id, actor_id, env)
                    effective_cmd = normalize_runtime_command(runtime, list(command or []))
                    session = pty_runner.SUPERVISOR.start_actor(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        cwd=cwd,
                        command=effective_cmd,
                        env=prepare_pty_env(inject_actor_context_env(effective_env, group.group_id, actor_id)),
                        max_backlog_bytes=pty_backlog_bytes(),
                    )
            except Exception as e:
                logger.warning("Autostart failed for %s/%s: %s", group_id, actor_id, e)
                continue

            try:
                if effective_runner == "headless":
                    write_headless_state(group.group_id, actor_id)
                else:
                    write_pty_state(group.group_id, actor_id, session.pid)
            except Exception as e:
                logger.debug("State write failed for %s/%s: %s", group_id, actor_id, e)

            clear_preamble_sent(group, actor_id)
            throttle_reset_actor(group.group_id, actor_id)

        try:
            if (
                get_group_state(group) == "active"
                and (
                    pty_runner.SUPERVISOR.group_running(group.group_id)
                    or headless_runner.SUPERVISOR.group_running(group.group_id)
                )
            ):
                automation_on_resume(group)
        except Exception:
            pass
