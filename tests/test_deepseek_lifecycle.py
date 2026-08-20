from __future__ import annotations

import io
import os
import queue
import threading
import time
from types import SimpleNamespace

from cccc.daemon.actors import deepseek_runtime
from cccc.daemon.actors.deepseek_setup import DeepSeekSetupOutcome
from cccc.daemon.actors.actor_profile_store import _normalize_profile_command
from cccc.daemon.actors.actor_runtime_ops import resolve_actor_launch_config
from cccc.daemon.group.bootstrap_actor_ops import autostart_running_groups
from cccc.daemon.group.group_lifecycle_ops import handle_group_start
from cccc.kernel.group import Group
from cccc.kernel.deepseek_acp import validate_session_update
from cccc.runners.deepseek import DeepSeekSupervisor
from cccc.runners.deepseek_streams import merge_subprocess_env


def _fake_acp_script() -> str:
    return r"""while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
elif printf '%s' "$line" | grep -q '"prompt":\[' && printf '%s' "$line" | grep -q '"type":"text"'; then
  printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ok"}}}}'
  printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
else
  printf '%s\n' '{"jsonrpc":"2.0","id":3,"error":{"message":"prompt must be ContentBlock[]"}}'
fi
done"""


def test_fake_acp_handshake_update_and_generation_cleanup(tmp_path) -> None:
    supervisor = DeepSeekSupervisor(
        ["sh", "-c", _fake_acp_script()],
        cwd=str(tmp_path),
        env=dict(os.environ),
    )
    supervisor.start()
    try:
        assert supervisor.handshake(timeout=2.0) == "fake-session"
        assert supervisor.submit("hello") == 3
        update = supervisor.next_frame(timeout=2.0)
        assert (
            validate_session_update(update, "fake-session")["sessionId"]
            == "fake-session"
        )
        assert supervisor.next_frame(timeout=2.0)["id"] == 3
    finally:
        supervisor.stop()
    assert not supervisor.is_running()


def test_unterminated_final_frame_does_not_block_when_frame_queue_is_full(
    tmp_path,
) -> None:
    supervisor = DeepSeekSupervisor(["unused"], cwd=str(tmp_path), env={})
    process = SimpleNamespace(
        stdout=io.BytesIO(b'{"jsonrpc":"2.0","id":1,"result":{}}')
    )
    frames: queue.Queue[object] = queue.Queue(maxsize=1)
    frames.put_nowait({"occupied": True})
    supervisor.generation = "generation"
    supervisor._process = process
    supervisor._protocol.register(1)
    reader = threading.Thread(
        target=supervisor._read_stdout,
        args=(process, "generation", frames),
        daemon=True,
    )

    reader.start()
    reader.join(timeout=0.2)
    blocked = reader.is_alive()
    if blocked:
        frames.get_nowait()
        reader.join(timeout=1.0)

    assert not blocked
    assert isinstance(supervisor._reader_error, Exception)
    assert supervisor._stopping.is_set()


def test_dropped_eof_sentinel_is_still_observed_without_waiting_for_timeout(
    tmp_path,
) -> None:
    supervisor = DeepSeekSupervisor(["unused"], cwd=str(tmp_path), env={})
    process = SimpleNamespace(stdout=io.BytesIO(b""))
    frames: queue.Queue[object] = queue.Queue(maxsize=1)
    frames.put_nowait({"jsonrpc": "2.0", "method": "session/update"})
    supervisor._frames = frames
    supervisor.generation = "generation"
    supervisor._process = process
    reader = threading.Thread(
        target=supervisor._read_stdout,
        args=(process, "generation", frames),
        daemon=True,
    )

    reader.start()
    reader.join(timeout=1.0)
    assert not reader.is_alive()
    assert supervisor.next_frame(timeout=0.1)["method"] == "session/update"
    started = time.monotonic()
    try:
        supervisor.next_frame(timeout=0.5)
    except RuntimeError:
        pass
    else:
        raise AssertionError("reader EOF must terminate the ACP generation")
    assert time.monotonic() - started < 0.2


def test_permission_request_is_answered_and_stop_clears_pending(tmp_path) -> None:
    script = r"""while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
else
  printf '%s\n' '{"jsonrpc":"2.0","id":9,"method":"session/request_permission","params":{"sessionId":"fake-session","options":[]}}'
  read -r response
  printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
fi
done"""
    supervisor = DeepSeekSupervisor(
        ["sh", "-c", script], cwd=str(tmp_path), env=dict(os.environ)
    )
    supervisor.start()
    try:
        supervisor.handshake(timeout=2.0)
        assert supervisor.submit("hello") == 3
        permission = supervisor.next_frame(timeout=2.0)
        assert permission["method"] == "session/request_permission"
        supervisor.respond_permission(permission["id"], [], stopping=False)
        assert supervisor.next_frame(timeout=2.0)["id"] == 3
    finally:
        supervisor.stop()
    assert not supervisor.is_running()


def test_python_daemon_registers_deepseek_and_restores_default_command(
    tmp_path,
) -> None:
    actor = {
        "id": "deepseek",
        "runtime": "deepseek",
        "runner": "headless",
        "command": [],
        "env": {},
    }
    group = Group(
        group_id="deepseek-launch",
        path=tmp_path,
        doc={"group_id": "deepseek-launch", "actors": [actor], "automation": {}},
    )
    launch = resolve_actor_launch_config(
        group,
        "deepseek",
        command=[],
        env={},
        runner="headless",
        runtime="deepseek",
        effective_runner_kind=lambda runner: runner,
    )
    expected = ["dsh-acp-demo"]
    assert launch["command"] == expected
    assert (
        _normalize_profile_command(runtime="deepseek", runner="headless", command=[])
        == expected
    )


def test_deepseek_subprocess_environment_inherits_daemon_values(monkeypatch) -> None:
    monkeypatch.setenv("DSH_HOME", "/daemon/dsh")
    monkeypatch.setenv("PATH", "/daemon/bin")
    env = merge_subprocess_env({"PATH": "/actor/bin", "ACTOR_ONLY": "yes"})
    assert env["DSH_HOME"] == "/daemon/dsh"
    assert env["PATH"] == "/actor/bin"
    assert env["ACTOR_ONLY"] == "yes"


def test_registry_runs_first_use_setup_before_starting_dsh(
    tmp_path, monkeypatch
) -> None:
    calls = []
    dsh_home = tmp_path / ".cccc/runtimes/deepseek/0.1.0-rc.6"

    class FakeSupervisor:
        def __init__(self, command, *, cwd, env):
            calls.append(("supervisor", list(command), cwd, dict(env)))

        def start(self):
            calls.append(("start",))

        def handshake(self, *, timeout):
            calls.append(("handshake", timeout))
            return "session"

        def stop(self):
            calls.append(("stop",))

        def is_running(self):
            return True

    def setup(env):
        env["CCCC_HOME"] = str(tmp_path / ".cccc")
        env["DSH_HOME"] = str(dsh_home)
        calls.append(("setup", dict(env)))
        return DeepSeekSetupOutcome(
            dsh_home=dsh_home,
            profile=dsh_home / "profiles" / "cccc-acp",
            packages_installed=False,
            profile_created=False,
        )

    monkeypatch.setattr(deepseek_runtime, "ensure_deepseek_setup", setup)
    monkeypatch.setattr(deepseek_runtime, "DeepSeekSupervisor", FakeSupervisor)
    deepseek_runtime.start(
        group_id="g-first-use",
        actor_id="deepseek",
        cwd=tmp_path,
        command=["dsh-acp-demo"],
        env={},
    )
    try:
        assert [call[0] for call in calls[:4]] == [
            "setup",
            "supervisor",
            "start",
            "handshake",
        ]
        assert calls[1][3]["DSH_HOME"] == str(dsh_home)
        assert calls[1][3]["CCCC_GROUP_ID"] == "g-first-use"
        assert calls[1][3]["CCCC_ACTOR_ID"] == "deepseek"
        assert calls[1][3]["CCCC_DEEPSEEK_SESSION_ROOT"] == str(
            tmp_path / ".cccc/groups/g-first-use/state/deepseek/deepseek/sessions"
        )
        assert calls[1][1][-2:] == [
            "--config",
            str(dsh_home / "profiles/cccc-acp/cordis.yml"),
        ]
    finally:
        deepseek_runtime.stop(group_id="g-first-use", actor_id="deepseek")


def _deepseek_group(path) -> Group:
    path.mkdir(parents=True, exist_ok=True)
    return Group(
        group_id="deepseek-group",
        path=path,
        doc={
            "group_id": "deepseek-group",
            "active_scope_key": "main",
            "running": True,
            "state": "active",
            "actors": [
                {
                    "id": "deepseek",
                    "runtime": "deepseek",
                    "runner": "headless",
                    "command": [],
                    "env": {},
                    "enabled": True,
                }
            ],
            "automation": {},
        },
    )


def test_group_start_routes_deepseek_to_dedicated_registry(
    tmp_path, monkeypatch
) -> None:
    import cccc.daemon.group.group_lifecycle_ops as ops

    group = _deepseek_group(tmp_path / "group")
    group.save()
    starts = []
    monkeypatch.setattr(ops, "load_group", lambda _group_id: group)
    monkeypatch.setattr(ops, "ensure_deepseek_setup", lambda _env: None)
    monkeypatch.setattr(ops, "require_group_permission", lambda *_args, **_kwargs: None)
    monkeypatch.setattr(
        ops, "runtime_start_preflight_error", lambda *_args, **_kwargs: ""
    )
    monkeypatch.setattr(
        ops.deepseek_runtime, "start", lambda **kwargs: starts.append(kwargs)
    )
    monkeypatch.setattr(
        ops.headless_runner.SUPERVISOR,
        "start_actor",
        lambda **_kwargs: (_ for _ in ()).throw(
            AssertionError("generic headless route used")
        ),
    )
    response = handle_group_start(
        {"group_id": group.group_id, "by": "user"},
        effective_runner_kind=lambda runner: runner,
        find_scope_url=lambda _group, _scope: str(tmp_path),
        ensure_mcp_installed=lambda *_args, **_kwargs: True,
        merge_actor_env_with_private=lambda _gid, _aid, env: dict(env),
        inject_actor_context_env=lambda env, **_kwargs: dict(env),
        normalize_runtime_command=lambda _runtime, command: list(command),
        prepare_pty_env=lambda env: dict(env),
        pty_backlog_bytes=lambda: 1024,
        write_headless_state=lambda _gid, _aid: None,
        write_pty_state=lambda *_args, **_kwargs: None,
        clear_preamble_sent=lambda _group, _aid: None,
        throttle_reset_actor=lambda *_args, **_kwargs: None,
        reset_automation_timers_if_active=lambda _group: None,
        supported_runtimes=("deepseek",),
        get_actor_profile=lambda _profile_id: None,
        load_actor_profile_secrets=lambda _profile_id: {},
        load_actor_private_env=lambda _gid, _aid: {},
        update_actor_private_env=lambda *_args, **_kwargs: {},
        delete_actor_private_env=lambda _gid, _aid: None,
    )
    assert response.ok, response.error
    assert starts[0]["command"] == ["dsh-acp-demo"]


def test_autostart_routes_deepseek_to_dedicated_registry(tmp_path, monkeypatch) -> None:
    import cccc.daemon.group.bootstrap_actor_ops as ops

    home = tmp_path / "home"
    group = _deepseek_group(home / "groups" / "deepseek-group")
    group.save()
    starts = []
    monkeypatch.setattr(ops, "load_group", lambda _group_id: group)
    monkeypatch.setattr(ops, "ensure_deepseek_setup", lambda _env: None)
    monkeypatch.setattr(
        ops, "runtime_start_preflight_error", lambda *_args, **_kwargs: ""
    )
    monkeypatch.setattr(
        ops.deepseek_runtime, "start", lambda **kwargs: starts.append(kwargs)
    )
    monkeypatch.setattr(
        ops.headless_runner.SUPERVISOR,
        "start_actor",
        lambda **_kwargs: (_ for _ in ()).throw(
            AssertionError("generic headless route used")
        ),
    )
    autostart_running_groups(
        home,
        effective_runner_kind=lambda runner: runner,
        find_scope_url=lambda _group, _scope: str(tmp_path),
        supported_runtimes=("deepseek",),
        ensure_mcp_installed=lambda *_args, **_kwargs: True,
        auto_mcp_runtimes=(),
        merge_actor_env_with_private=lambda _gid, _aid, env: dict(env),
        inject_actor_context_env=lambda env, _gid, _aid: dict(env),
        prepare_pty_env=lambda env: dict(env),
        normalize_runtime_command=lambda _runtime, command: list(command),
        pty_backlog_bytes=lambda: 1024,
        write_headless_state=lambda _gid, _aid: None,
        write_pty_state=lambda *_args, **_kwargs: None,
        clear_preamble_sent=lambda _group, _aid: None,
        throttle_reset_actor=lambda *_args, **_kwargs: None,
        automation_on_resume=lambda _group: None,
        get_group_state=lambda _group: "active",
        load_actor_private_env=lambda _gid, _aid: {},
        update_actor_private_env=lambda *_args, **_kwargs: {},
        delete_actor_private_env=lambda _gid, _aid: None,
    )
    assert starts[0]["command"] == ["dsh-acp-demo"]
