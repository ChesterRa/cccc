from __future__ import annotations

import uuid
from pathlib import Path
from typing import Any, Iterable, Mapping

from ...kernel.runtime import get_cccc_mcp_stdio_command
from ...paths import ensure_home
from ...runners import pty as pty_runner
from .launch import (
    _invalidate_previous_launch,
    _write_launch_identity,
    serialize_actor_launch,
    start_actor_with_hooks,
)
from .provider_command import cli_command_from_mcp


def start_pty_actor_with_hooks(
    *,
    group_id: str,
    actor_id: str,
    cwd: Path,
    command: Iterable[str],
    env: Mapping[str, str],
    runtime: str,
    max_backlog_bytes: int,
) -> Any:
    return start_actor_with_hooks(
        supervisor=pty_runner.SUPERVISOR,
        home=ensure_home(),
        group_id=group_id,
        actor_id=actor_id,
        cwd=cwd,
        command=command,
        env=env,
        runtime=runtime,
        max_backlog_bytes=max_backlog_bytes,
        cccc_command=cli_command_from_mcp(
            get_cccc_mcp_stdio_command()
        ),
    )


def start_unhooked_pty_actor(
    *,
    supervisor: Any,
    home: Path,
    group_id: str,
    actor_id: str,
    cwd: Path,
    command: Iterable[str],
    env: Mapping[str, str],
    runtime: str,
    runtime_state_source: str,
    max_backlog_bytes: int,
) -> Any:
    with serialize_actor_launch(supervisor, group_id, actor_id):
        return _start_unhooked_pty_actor_serialized(
            supervisor=supervisor,
            home=home,
            group_id=group_id,
            actor_id=actor_id,
            cwd=cwd,
            command=command,
            env=env,
            runtime=runtime,
            runtime_state_source=runtime_state_source,
            max_backlog_bytes=max_backlog_bytes,
        )


def _start_unhooked_pty_actor_serialized(
    *,
    supervisor: Any,
    home: Path,
    group_id: str,
    actor_id: str,
    cwd: Path,
    command: Iterable[str],
    env: Mapping[str, str],
    runtime: str,
    runtime_state_source: str,
    max_backlog_bytes: int,
) -> Any:
    argv = [str(item) for item in command]
    launch_env = {str(key): str(value) for key, value in env.items()}
    if supervisor.actor_running(group_id=group_id, actor_id=actor_id):
        return supervisor.start_actor(
            group_id=group_id,
            actor_id=actor_id,
            cwd=cwd,
            command=argv,
            env=launch_env,
            runtime=runtime,
            max_backlog_bytes=max_backlog_bytes,
        )
    runtime_name = str(runtime or "").strip().lower()
    token = uuid.uuid4().hex
    _invalidate_previous_launch(
        home, runtime_name, group_id, actor_id, token
    )
    session = supervisor.start_actor(
        group_id=group_id,
        actor_id=actor_id,
        cwd=cwd,
        command=argv,
        env=launch_env,
        runtime=runtime,
        max_backlog_bytes=max_backlog_bytes,
    )
    _write_launch_identity(
        home,
        group_id,
        actor_id,
        runtime_name,
        token,
        hook_enabled=False,
        pid=int(getattr(session, "pid", 0) or 0),
    )
    try:
        session.bind_input_observer(None, None)
        setattr(
            session,
            "_runtime_state_source",
            str(runtime_state_source or ""),
        )
    except Exception:
        pass
    return session
