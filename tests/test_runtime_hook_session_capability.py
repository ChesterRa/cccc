from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from cccc.daemon.codex_app_sessions import CodexAppSession
from cccc.daemon.runtime_hooks.launch import start_actor_with_hooks
from cccc.daemon.runtime_hooks.pty_launch import (
    start_unhooked_pty_actor,
)
from cccc.kernel.runtime_hooks.activity import read_events
from cccc.kernel.runtime_hooks.projection import (
    read_launch_identity,
    runtime_hook_working_projection,
)
from cccc.kernel.runtime_hooks.store import (
    read_state,
    record_hook_event,
)


class _Session:
    def __init__(
        self, pid: int, group_id: str, actor_id: str
    ) -> None:
        self.pid = pid
        self.group_id = group_id
        self.actor_id = actor_id
        self._input_observer = None
        self._runtime_hook_capability = None

    def bind_input_observer(self, observer, capability) -> None:
        self._input_observer = observer
        self._runtime_hook_capability = capability

    def emit(self, data: bytes) -> None:
        if self._input_observer is not None:
            self._input_observer(self, data)


class _Supervisor:
    def __init__(self) -> None:
        self.running = False
        self.next_pid = 100
        self.session: _Session | None = None

    def actor_running(self, *, group_id: str, actor_id: str) -> bool:
        return self.running

    def start_actor(self, **kwargs: object) -> _Session:
        if self.running and self.session is not None:
            return self.session
        self.next_pid += 1
        self.session = _Session(
            self.next_pid,
            str(kwargs["group_id"]),
            str(kwargs["actor_id"]),
        )
        self.running = True
        return self.session


def _start_eligible(
    supervisor: _Supervisor, home: Path
) -> _Session:
    return start_actor_with_hooks(
        supervisor=supervisor,
        home=home,
        group_id="g1",
        actor_id="peer",
        cwd=home,
        command=["codex"],
        env={},
        runtime="codex",
        max_backlog_bytes=1024,
        cccc_command=["/bin/cccc"],
    )


def _start_turn(home: Path) -> str:
    state = read_state(home, "codex", "g1", "peer")
    assert state is not None
    token = state.launch_token
    for payload in (
        {"hook_event_name": "SessionStart", "session_id": "session-1"},
        {
            "hook_event_name": "UserPromptSubmit",
            "session_id": "session-1",
            "turn_id": "turn-1",
        },
        {
            "hook_event_name": "PreToolUse",
            "session_id": "session-1",
            "turn_id": "turn-1",
            "tool_use_id": "op-1",
            "tool_name": "Bash",
        },
    ):
        record_hook_event(
            home, "codex", "g1", "peer", token, payload
        )
    return token


def test_remote_tui_revoke_blocks_old_and_current_input_capabilities(
    tmp_path: Path,
) -> None:
    supervisor = _Supervisor()
    eligible = _start_eligible(supervisor, tmp_path)
    old_token = _start_turn(tmp_path)
    assert eligible._runtime_hook_capability.launch_token == old_token
    assert runtime_hook_working_projection(
        tmp_path,
        running=True,
        effective_runner="pty",
        runtime="codex",
        group_id="g1",
        actor_id="peer",
        session_capability=eligible._runtime_hook_capability,
    ) is not None
    before_activity = read_events(tmp_path, "g1")

    supervisor.running = False
    remote = start_unhooked_pty_actor(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=["codex", "app-server"],
        env={},
        runtime="codex",
        runtime_state_source="app_server",
        max_backlog_bytes=1024,
    )
    identity = read_launch_identity(tmp_path, "g1", "peer")
    state = read_state(tmp_path, "codex", "g1", "peer")
    assert identity is not None and identity["hook_enabled"] is False
    assert state is not None and state.launch_token != old_token
    assert remote._runtime_hook_capability is None
    assert runtime_hook_working_projection(
        tmp_path,
        running=True,
        effective_runner="pty",
        runtime="codex",
        group_id="g1",
        actor_id="peer",
        session_capability=None,
    ) is None

    remote.emit(b"\x03")
    eligible.emit(b"\x03")
    assert read_state(tmp_path, "codex", "g1", "peer") == state
    assert read_events(tmp_path, "g1") == before_activity


def test_remote_tui_to_eligible_restart_binds_only_new_session(
    tmp_path: Path,
) -> None:
    supervisor = _Supervisor()
    remote = start_unhooked_pty_actor(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=["codex", "app-server"],
        env={},
        runtime="codex",
        runtime_state_source="app_server",
        max_backlog_bytes=1024,
    )
    supervisor.running = False
    eligible = _start_eligible(supervisor, tmp_path)
    token = _start_turn(tmp_path)
    remote.emit(b"\x03")
    assert read_state(
        tmp_path, "codex", "g1", "peer"
    ).event == "PreToolUse"  # type: ignore[union-attr]
    eligible.emit(b"\x03")
    state = read_state(tmp_path, "codex", "g1", "peer")
    assert state is not None and state.launch_token == token
    assert state.event == "UserInterrupt"


def test_codex_remote_tui_uses_explicit_unhooked_facade(
    tmp_path: Path,
) -> None:
    app_session = CodexAppSession(
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        env={},
        listen_url="ws://127.0.0.1:1234",
        start_remote_tui=True,
        remote_tui_base_command=["codex"],
    )
    pty_session = SimpleNamespace(pid=123)
    with patch(
        "cccc.daemon.codex_app_sessions.start_unhooked_pty_actor",
        return_value=pty_session,
    ) as start:
        assert app_session._start_remote_tui(env={}) is pty_session
    assert start.call_args.kwargs["runtime_state_source"] == "app_server"
