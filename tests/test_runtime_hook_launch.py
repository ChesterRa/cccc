from __future__ import annotations

import json
import threading
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from types import SimpleNamespace

import pytest

from cccc.daemon.runtime_hooks.claude import (
    append_claude_settings,
    parse_claude_version,
)
from cccc.daemon.runtime_hooks.codex import configure_codex_launch
from cccc.daemon.runtime_hooks.launch import (
    _probe_claude_version,
    read_launch_identity,
    start_actor_with_hooks,
)
from cccc.kernel.runtime_hooks.store import (
    begin_launch,
    read_state,
    record_hook_event,
)
from cccc.kernel.runtime_hooks.projection import (
    runtime_hook_working_projection,
)
from cccc.kernel.working_state import derive_effective_working_state


def test_codex_direct_command_gets_session_only_hooks(tmp_path: Path) -> None:
    command, env = configure_codex_launch(
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        command=["codex", "--search"],
        env={"PATH": "/usr/bin"},
        cccc_executable=Path("/opt/cccc bin/cccc"),
        launch_token="token",
    )
    assert command[:2] == ["codex", "--search"]
    assert any(item.startswith("hooks.UserPromptSubmit=") for item in command)
    assert any(item.startswith("hooks.PostToolUse=") for item in command)
    assert any(item.startswith("hooks.Stop=") for item in command)
    assert not any(item.startswith("hooks.PostToolUseFailure=") for item in command)
    assert not any(item.startswith("hooks.StopFailure=") for item in command)
    assert any(item.startswith("hooks.state=") for item in command)
    assert env["CCCC_HOOK_LAUNCH_TOKEN"] == "token"


def test_wrapper_and_app_server_codex_are_not_eligible(tmp_path: Path) -> None:
    for command in (["wrapper", "codex"], ["codex", "app-server"]):
        configured, env = configure_codex_launch(
            home=tmp_path,
            group_id="g1",
            actor_id="peer",
            command=command,
            env={},
            cccc_executable=Path("/bin/cccc"),
            launch_token="token",
        )
        assert configured == command
        assert "CCCC_HOOK_LAUNCH_TOKEN" not in env


def test_claude_settings_merge_preserves_existing_hooks_and_prompt_tail(tmp_path: Path) -> None:
    command = [
        "claude",
        "--settings",
        '{"language":"zh","hooks":{"Stop":[{"matcher":"existing"}]}}',
        "--",
        "--settings",
        "prompt text",
    ]
    configured = append_claude_settings(command, cwd=tmp_path, cccc_executable=Path("/bin/cccc"))
    assert configured[:2] == ["claude", "--settings"]
    settings = json.loads(configured[2])
    assert settings["language"] == "zh"
    assert settings["hooks"]["Stop"][0]["matcher"] == "existing"
    assert configured[3:] == ["--", "--settings", "prompt text"]
    assert parse_claude_version("2.1.141 (Claude Code)") == (2, 1, 141)


def test_input_observer_counts_submit_once_and_ignores_paste_payload(tmp_path: Path) -> None:
    supervisor = _FakeSupervisor()
    session = start_actor_with_hooks(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=["claude"],
        env={},
        runtime="claude",
        max_backlog_bytes=1024,
        cccc_command=["/bin/cccc"],
        claude_version_probe=lambda *_args: (2, 1, 999),
    )
    token = read_state(tmp_path, "claude", "g1", "peer").launch_token  # type: ignore[union-attr]
    record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        token,
        {"hook_event_name": "SessionStart", "session_id": "session-1"},
    )
    session._input_observer(  # type: ignore[attr-defined]
        session, b"\x1b[200~hello\nworld\x1b[201~"
    )
    assert read_state(tmp_path, "claude", "g1", "peer").turn_generation == 0  # type: ignore[union-attr]
    session._input_observer(session, b"\r")  # type: ignore[attr-defined]
    assert read_state(tmp_path, "claude", "g1", "peer").turn_generation == 1  # type: ignore[union-attr]
    session._input_observer(  # type: ignore[attr-defined]
        session, b"second prompt\r"
    )
    assert read_state(tmp_path, "claude", "g1", "peer").turn_generation == 2  # type: ignore[union-attr]
    session._input_observer(session, b"\x03")  # type: ignore[attr-defined]
    state = read_state(tmp_path, "claude", "g1", "peer")
    assert state is not None and state.status == "idle"


class _FakeSupervisor:
    def __init__(self) -> None:
        self.running = False
        self.commands: list[list[str]] = []
        self.session = SimpleNamespace(pid=123)

    def actor_running(self, *, group_id: str, actor_id: str) -> bool:
        return self.running

    def start_actor(self, **kwargs: object) -> object:
        if self.running:
            return self.session
        self.session.group_id = str(kwargs["group_id"])
        self.session.actor_id = str(kwargs["actor_id"])
        self.commands.append(list(kwargs["command"]))  # type: ignore[arg-type]
        self.running = True
        return self.session


def test_actual_process_launch_rotates_token_but_running_actor_does_not(
    tmp_path: Path,
) -> None:
    supervisor = _FakeSupervisor()
    common = {
        "supervisor": supervisor,
        "home": tmp_path,
        "group_id": "g1",
        "actor_id": "peer",
        "cwd": tmp_path,
        "command": ["codex", "--search"],
        "env": {},
        "runtime": "codex",
        "max_backlog_bytes": 1024,
        "cccc_executable": Path("/bin/cccc"),
    }
    first = start_actor_with_hooks(**common)
    first_token = read_state(tmp_path, "codex", "g1", "peer").launch_token  # type: ignore[union-attr]
    assert start_actor_with_hooks(**common) is first
    assert read_state(tmp_path, "codex", "g1", "peer").launch_token == first_token  # type: ignore[union-attr]

    supervisor.running = False
    start_actor_with_hooks(**common)
    second_token = read_state(tmp_path, "codex", "g1", "peer").launch_token  # type: ignore[union-attr]
    assert second_token != first_token
    assert len(supervisor.commands) == 2


def test_concurrent_launches_keep_process_identity_and_capability_aligned(
    tmp_path: Path,
) -> None:
    class ConcurrentSupervisor(_FakeSupervisor):
        def __init__(self) -> None:
            super().__init__()
            self.start_entered = threading.Event()
            self.finish_start = threading.Event()
            self.second_running_check = threading.Event()
            self.running_checks = 0
            self.start_lock = threading.Lock()
            self.process_token = ""

        def actor_running(self, *, group_id: str, actor_id: str) -> bool:
            self.running_checks += 1
            if self.running_checks == 2:
                self.second_running_check.set()
            return self.running

        def start_actor(self, **kwargs: object) -> object:
            with self.start_lock:
                if self.running:
                    return self.session
                self.start_entered.set()
                assert self.finish_start.wait(timeout=2)
                env = kwargs["env"]
                assert isinstance(env, dict)
                self.process_token = str(env["CCCC_HOOK_LAUNCH_TOKEN"])
                return super().start_actor(**kwargs)

    supervisor = ConcurrentSupervisor()
    common = {
        "supervisor": supervisor,
        "home": tmp_path,
        "group_id": "g1",
        "actor_id": "peer",
        "cwd": tmp_path,
        "command": ["codex"],
        "env": {},
        "runtime": "codex",
        "max_backlog_bytes": 1024,
        "cccc_command": ["/bin/cccc"],
    }
    with ThreadPoolExecutor(max_workers=2) as pool:
        first = pool.submit(start_actor_with_hooks, **common)
        assert supervisor.start_entered.wait(timeout=2)
        second = pool.submit(start_actor_with_hooks, **common)
        assert not supervisor.second_running_check.wait(timeout=0.1)
        supervisor.finish_start.set()
        assert second.result(timeout=2) is first.result(timeout=2)

    identity = read_launch_identity(tmp_path, "g1", "peer")
    capability = supervisor.session._runtime_hook_capability
    assert identity is not None
    assert identity["launch_token"] == supervisor.process_token
    assert capability.launch_token == supervisor.process_token
    assert len(supervisor.commands) == 1


def test_spawn_failure_records_explicit_unavailable_reason(tmp_path: Path) -> None:
    supervisor = _FakeSupervisor()

    def fail(**kwargs: object) -> object:
        raise OSError("spawn denied")

    supervisor.start_actor = fail  # type: ignore[method-assign]
    try:
        start_actor_with_hooks(
            supervisor=supervisor,
            home=tmp_path,
            group_id="g1",
            actor_id="peer",
            cwd=tmp_path,
            command=["codex"],
            env={},
            runtime="codex",
            max_backlog_bytes=1024,
            cccc_executable=Path("/bin/cccc"),
        )
    except OSError:
        pass
    else:
        raise AssertionError("expected spawn failure")
    state = read_state(tmp_path, "codex", "g1", "peer")
    assert state is not None and state.event == "HookUnavailableSpawn"


def test_hook_setup_failure_preserves_process_launch_and_fails_closed(
    tmp_path: Path,
) -> None:
    supervisor = _FakeSupervisor()

    def unavailable(
        _command: list[str], _cwd: Path, _env: object
    ) -> tuple[int, int, int] | None:
        raise OSError("version probe unavailable")

    session = start_actor_with_hooks(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=["claude"],
        env={},
        runtime="claude",
        max_backlog_bytes=1024,
        cccc_executable=Path("/bin/cccc"),
        claude_version_probe=unavailable,
    )
    assert session is supervisor.session
    assert supervisor.commands == [["claude"]]
    state = read_state(tmp_path, "claude", "g1", "peer")
    assert state is not None and state.event == "HookUnavailableVersion"


def test_direct_codex_resume_rotates_token_and_injects_hooks(tmp_path: Path) -> None:
    begin_launch(tmp_path, "codex", "g1", "peer", "stale-token")
    record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "stale-token",
        {"hook_event_name": "SessionStart", "session_id": "stale-session"},
    )
    supervisor = _FakeSupervisor()
    session = start_actor_with_hooks(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=["codex", "resume", "session-new"],
        env={},
        runtime="codex",
        max_backlog_bytes=1024,
        cccc_command=["/bin/cccc"],
    )
    state = read_state(tmp_path, "codex", "g1", "peer")
    assert state is not None and state.launch_token != "stale-token"
    assert state.awaiting_session_start
    assert "hooks.UserPromptSubmit=" in " ".join(supervisor.commands[0])


def test_claude_unsupported_launch_revokes_stale_hook_capability(
    tmp_path: Path,
) -> None:
    begin_launch(tmp_path, "claude", "g1", "peer", "stale-token")
    record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        "stale-token",
        {"hook_event_name": "SessionStart", "session_id": "stale-session"},
    )
    supervisor = _FakeSupervisor()
    start_actor_with_hooks(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=["claude"],
        env={},
        runtime="claude",
        max_backlog_bytes=1024,
        cccc_command=["/bin/cccc"],
        claude_version_probe=lambda *_args: None,
    )
    state = read_state(tmp_path, "claude", "g1", "peer")
    identity = read_launch_identity(tmp_path, "g1", "peer")
    assert state is not None and state.event == "HookUnavailableVersion"
    assert state.launch_token != "stale-token"
    assert identity is not None and identity["hook_enabled"] is False


def test_bracketed_paste_split_across_raw_writes_opens_one_generation(
    tmp_path: Path,
) -> None:
    supervisor = _FakeSupervisor()
    session = start_actor_with_hooks(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=["claude"],
        env={},
        runtime="claude",
        max_backlog_bytes=1024,
        cccc_command=["/bin/cccc"],
        claude_version_probe=lambda *_args: (2, 1, 999),
    )
    token = read_state(tmp_path, "claude", "g1", "peer").launch_token  # type: ignore[union-attr]
    record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        token,
        {"hook_event_name": "SessionStart", "session_id": "session-1"},
    )
    session._input_observer(session, b"\x1b[200~first\n")  # type: ignore[attr-defined]
    session._input_observer(  # type: ignore[attr-defined]
        session, b"second\x1b[201~"
    )
    session._input_observer(session, b"\r")  # type: ignore[attr-defined]
    assert read_state(tmp_path, "claude", "g1", "peer").turn_generation == 1  # type: ignore[union-attr]


def test_claude_probe_requires_success_exit_even_with_stderr_version(
    tmp_path: Path, monkeypatch
) -> None:
    monkeypatch.setattr(
        "cccc.daemon.runtime_hooks.launch.subprocess.run",
        lambda *_args, **_kwargs: SimpleNamespace(
            returncode=1, stdout="", stderr="2.1.999"
        ),
    )
    assert _probe_claude_version(["claude"], tmp_path, {}) is None


def test_multi_argv_cccc_fallback_is_preserved_in_codex_injection(
    tmp_path: Path,
) -> None:
    configured, _env = configure_codex_launch(
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        command=["codex"],
        env={},
        cccc_command=["/usr/bin/python3", "-m", "cccc.cli"],
        launch_token="token",
    )
    joined = " ".join(configured)
    assert 'mcp_servers.cccc.command="/usr/bin/python3"' in joined
    assert 'mcp_servers.cccc.args=["-m","cccc.cli","mcp"]' in joined
    assert "/usr/bin/python3 -m cccc.cli hook codex-state" in joined


@pytest.mark.parametrize(
    ("runtime", "command", "version", "expected_event"),
    [
        ("codex", ["wrapper", "codex"], None, "HookUnavailableCommand"),
        ("codex", ["codex", "app-server"], None, "HookUnavailableCommand"),
        ("claude", ["claude"], (2, 1, 100), "HookUnavailableVersion"),
    ],
)
def test_every_real_noneligible_launch_revokes_stale_capability_first(
    tmp_path: Path,
    runtime: str,
    command: list[str],
    version: tuple[int, int, int] | None,
    expected_event: str,
) -> None:
    begin_launch(tmp_path, runtime, "g1", "peer", "stale-token")
    supervisor = _FakeSupervisor()
    start_actor_with_hooks(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=command,
        env={},
        runtime=runtime,
        max_backlog_bytes=1024,
        cccc_command=["/bin/cccc"],
        claude_version_probe=lambda *_args: version,
    )
    state = read_state(tmp_path, runtime, "g1", "peer")
    identity = read_launch_identity(tmp_path, "g1", "peer")
    assert state is not None and state.launch_token != "stale-token"
    assert state.event == expected_event
    assert identity is not None and identity["hook_enabled"] is False
    assert identity["launch_token"] == state.launch_token


def test_headless_and_noneligible_processes_do_not_consume_hook_projection(
    tmp_path: Path,
) -> None:
    supervisor = _FakeSupervisor()
    start_actor_with_hooks(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=["codex"],
        env={},
        runtime="codex",
        max_backlog_bytes=1024,
        cccc_command=["/bin/cccc"],
    )
    assert (
        runtime_hook_working_projection(
            tmp_path,
            running=True,
            effective_runner="headless",
            runtime="codex",
            group_id="g1",
            actor_id="peer",
        )
        is None
    )

    supervisor.running = False
    start_actor_with_hooks(
        supervisor=supervisor,
        home=tmp_path,
        group_id="g1",
        actor_id="peer",
        cwd=tmp_path,
        command=["codex", "app-server"],
        env={},
        runtime="codex",
        max_backlog_bytes=1024,
        cccc_command=["/bin/cccc"],
    )
    projection = runtime_hook_working_projection(
        tmp_path,
        running=True,
        effective_runner="pty",
        runtime="codex",
        group_id="g1",
        actor_id="peer",
    )
    assert projection is None
    result = derive_effective_working_state(
        running=True,
        effective_runner="pty",
        runtime="codex",
        idle_seconds=0,
        pty_terminal_text="old heuristic remains authoritative",
        runtime_hook_projection=projection,
    )
    assert result["effective_working_reason"] == "pty_no_prompt_recent_output"
