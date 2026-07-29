from __future__ import annotations

import os
import subprocess
import threading
import uuid
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Callable, Iterable, Mapping

from ...kernel.runtime_hooks.projection import (
    launch_identity_path,
    read_launch_identity,
)
from ...kernel.runtime_hooks.committed_io import write_json_committed
from ...kernel.runtime_hooks.store import begin_launch
from .claude import (
    MIN_CLAUDE_VERSION,
    configure_claude_launch,
    is_direct_claude_command,
    parse_claude_version,
)
from .codex import configure_codex_launch, is_direct_codex_command
from .input_observer import (
    HookSessionCapability,
    observe_pty_input,
    reset_pty_input,
)
from .provider_command import normalize_cli_command

ClaudeVersionProbe = Callable[[list[str], Path, Mapping[str, str]], tuple[int, int, int] | None]
_LAUNCH_LOCKS: dict[tuple[int, str, str], tuple[threading.Lock, int]] = {}
_LAUNCH_LOCKS_GUARD = threading.Lock()


@contextmanager
def serialize_actor_launch(
    supervisor: Any, group_id: str, actor_id: str
):
    key = (id(supervisor), str(group_id), str(actor_id))
    with _LAUNCH_LOCKS_GUARD:
        lock, references = _LAUNCH_LOCKS.get(
            key, (threading.Lock(), 0)
        )
        _LAUNCH_LOCKS[key] = (lock, references + 1)
    lock.acquire()
    try:
        yield
    finally:
        lock.release()
        with _LAUNCH_LOCKS_GUARD:
            current = _LAUNCH_LOCKS.get(key)
            if current is not None:
                current_lock, references = current
                if references == 1:
                    _LAUNCH_LOCKS.pop(key, None)
                else:
                    _LAUNCH_LOCKS[key] = (
                        current_lock,
                        references - 1,
                    )


def start_actor_with_hooks(
    *,
    supervisor: Any,
    home: Path,
    group_id: str,
    actor_id: str,
    cwd: Path,
    command: Iterable[str],
    env: Mapping[str, str],
    runtime: str,
    max_backlog_bytes: int,
    cccc_executable: Path | None = None,
    cccc_command: list[str] | None = None,
    claude_version_probe: ClaudeVersionProbe | None = None,
) -> Any:
    with serialize_actor_launch(supervisor, group_id, actor_id):
        return _start_actor_with_hooks_serialized(
            supervisor=supervisor,
            home=home,
            group_id=group_id,
            actor_id=actor_id,
            cwd=cwd,
            command=command,
            env=env,
            runtime=runtime,
            max_backlog_bytes=max_backlog_bytes,
            cccc_executable=cccc_executable,
            cccc_command=cccc_command,
            claude_version_probe=claude_version_probe,
        )


def _start_actor_with_hooks_serialized(
    *,
    supervisor: Any,
    home: Path,
    group_id: str,
    actor_id: str,
    cwd: Path,
    command: Iterable[str],
    env: Mapping[str, str],
    runtime: str,
    max_backlog_bytes: int,
    cccc_executable: Path | None = None,
    cccc_command: list[str] | None = None,
    claude_version_probe: ClaudeVersionProbe | None = None,
) -> Any:
    cli_command = normalize_cli_command(cccc_command, cccc_executable)
    argv = [str(item) for item in command]
    launch_env = {str(key): str(value) for key, value in env.items()}
    original_argv = list(argv)
    original_env = dict(launch_env)
    runtime_name = str(runtime or "").strip().lower()
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

    raw_token = uuid.uuid4()
    token = str(getattr(raw_token, "hex", raw_token)).replace("-", "")
    _invalidate_previous_launch(
        home, runtime_name, group_id, actor_id, token
    )
    attempted = False
    hook_state_active = False
    hook_input_active = False
    unavailable_event = "HookUnavailableExecutable"
    try:
        if runtime_name == "codex" and is_direct_codex_command(argv):
            attempted = True
            argv, launch_env = configure_codex_launch(
                home=home,
                group_id=group_id,
                actor_id=actor_id,
                command=argv,
                env=launch_env,
                cccc_command=cli_command,
                launch_token=token,
            )
            hook_state_active = True
            hook_input_active = True
        elif runtime_name == "claude" and is_direct_claude_command(argv):
            attempted = True
            unavailable_event = "HookUnavailableVersion"
            probe = claude_version_probe or _probe_claude_version
            version = probe(argv, cwd, launch_env)
            if version is None or version < MIN_CLAUDE_VERSION:
                begin_launch(
                    home,
                    runtime_name,
                    group_id,
                    actor_id,
                    token,
                    event="HookUnavailableVersion",
                )
            else:
                unavailable_event = "HookUnavailableSettings"
                argv, launch_env = configure_claude_launch(
                    home=home,
                    group_id=group_id,
                    actor_id=actor_id,
                    command=argv,
                    env=launch_env,
                    cwd=cwd,
                    cccc_command=cli_command,
                    launch_token=token,
                    version=version,
                )
                hook_state_active = True
                hook_input_active = True
    except Exception:
        if attempted:
            begin_launch(
                home,
                runtime_name,
                group_id,
                actor_id,
                token,
                event=unavailable_event,
            )
            hook_state_active = True
        argv = original_argv
        launch_env = original_env

    try:
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
            hook_enabled=hook_state_active,
            pid=int(getattr(session, "pid", 0) or 0),
        )
        if hook_state_active:
            _bind_session_capability(
                session,
                home=home,
                runtime=runtime_name,
                launch_token=token,
                input_enabled=hook_input_active,
            )
        return session
    except Exception:
        if hook_state_active:
            begin_launch(
                home,
                runtime_name,
                group_id,
                actor_id,
                token,
                event="HookUnavailableSpawn",
            )
        raise


def _probe_claude_version(
    command: list[str], cwd: Path, env: Mapping[str, str]
) -> tuple[int, int, int] | None:
    process_env = os.environ.copy()
    process_env.update({str(key): str(value) for key, value in env.items()})
    completed = subprocess.run(
        [command[0], "--version"],
        cwd=str(cwd),
        env=process_env,
        capture_output=True,
        text=True,
        timeout=3,
        check=False,
    )
    if completed.returncode != 0:
        return None
    return parse_claude_version(completed.stdout)


def _bind_session_capability(
    session: Any,
    *,
    home: Path,
    runtime: str,
    launch_token: str,
    input_enabled: bool,
) -> None:
    capability = HookSessionCapability(
        runtime=runtime,
        launch_token=launch_token,
        pid=int(getattr(session, "pid", 0) or 0),
        runtime_state_source="terminal",
        input_enabled=input_enabled,
    )
    observer = (
        lambda current, data: observe_pty_input(
            home, capability, current, data
        )
    ) if input_enabled else None
    try:
        session.bind_input_observer(observer, capability)
    except Exception:
        setattr(session, "_input_observer", observer)
        setattr(session, "_runtime_hook_capability", capability)


def _invalidate_previous_launch(
    home: Path,
    runtime: str,
    group_id: str,
    actor_id: str,
    launch_token: str,
) -> None:
    reset_pty_input(group_id, actor_id)
    if runtime in {"codex", "claude"}:
        begin_launch(
            home,
            runtime,
            group_id,
            actor_id,
            launch_token,
            event="HookUnavailableCommand",
            observer=lambda _state: _write_launch_identity(
                home,
                group_id,
                actor_id,
                runtime,
                launch_token,
                hook_enabled=False,
                pid=0,
            ),
        )
    else:
        launch_identity_path(home, group_id, actor_id).unlink(
            missing_ok=True
        )


def _write_launch_identity(
    home: Path,
    group_id: str,
    actor_id: str,
    runtime: str,
    launch_token: str,
    *,
    hook_enabled: bool,
    pid: int,
) -> None:
    write_json_committed(
        launch_identity_path(home, group_id, actor_id),
        {
            "v": 1,
            "group_id": group_id,
            "actor_id": actor_id,
            "runtime": runtime,
            "launch_token": launch_token,
            "hook_enabled": bool(hook_enabled),
            "pid": int(pid),
        },
    )
