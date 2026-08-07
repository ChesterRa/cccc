from __future__ import annotations

import json
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import pytest

from cccc.kernel.runtime_hooks.contracts import HookState, RuntimeActivityEvent
from cccc.kernel.runtime_hooks.activity import read_events
from cccc.kernel.runtime_hooks.committed_io import write_json_committed
from cccc.kernel.runtime_hooks.projection import (
    launch_identity_path,
    read_launch_identity,
    read_working_projection,
    runtime_hook_working_projection,
)
from cccc.kernel.runtime_hooks.store import (
    begin_launch,
    read_state,
    record_hook_event,
    record_interrupt,
    record_terminal_input,
    state_path,
)
from cccc.kernel.working_state import derive_effective_working_state


def _payload(event: str, **extra: object) -> dict[str, object]:
    return {"hook_event_name": event, **extra}


def _write_eligible_identity(
    home: Path, runtime: str, group_id: str, actor_id: str, token: str
) -> None:
    write_json_committed(
        launch_identity_path(home, group_id, actor_id),
        {
            "v": 1,
            "group_id": group_id,
            "actor_id": actor_id,
            "runtime": runtime,
            "launch_token": token,
            "hook_enabled": True,
            "pid": 123,
        },
    )


def test_non_numeric_launch_identity_pid_fails_closed(
    tmp_path: Path,
) -> None:
    path = launch_identity_path(tmp_path, "g1", "peer")
    write_json_committed(
        path,
        {
            "v": 1,
            "group_id": "g1",
            "actor_id": "peer",
            "runtime": "codex",
            "launch_token": "token",
            "hook_enabled": True,
            "pid": "corrupt",
        },
    )
    capability = {
        "runtime": "codex",
        "launch_token": "token",
        "runtime_state_source": "terminal",
        "pid": 123,
    }
    assert read_launch_identity(tmp_path, "g1", "peer") is None
    assert runtime_hook_working_projection(
        tmp_path,
        running=True,
        effective_runner="pty",
        runtime="codex",
        group_id="g1",
        actor_id="peer",
        session_capability=capability,
    ) is None


def test_shared_rust_python_golden_decodes_exact_schema() -> None:
    fixture = json.loads(
        (Path(__file__).parent / "fixtures/runtime_hooks/v3_state_and_activity.json").read_text()
    )
    assert HookState.from_dict(fixture["state"]).to_dict() == fixture["state"]
    assert RuntimeActivityEvent.from_dict(fixture["activity"]).to_dict() == fixture["activity"]


def test_v3_launch_session_turn_operation_and_session_end_are_fenced(tmp_path: Path) -> None:
    pending = begin_launch(tmp_path, "codex", "g1", "peer", "token-new")
    assert pending.awaiting_session_start
    assert record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token-old",
        _payload("SessionStart", session_id="stale"),
    ) == pending

    started = record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token-new",
        _payload("SessionStart", session_id="session-1"),
    )
    assert started.status == "idle"
    turn = record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token-new",
        _payload("UserPromptSubmit", session_id="session-1", turn_id="turn-1"),
    )
    assert (turn.status, turn.turn_generation, turn.turn_id) == ("working", 1, "turn-1")
    operation = record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token-new",
        _payload(
            "PreToolUse",
            session_id="session-1",
            turn_id="turn-1",
            tool_use_id="operation-1",
        ),
    )
    assert operation.operation_id == "operation-1"
    failed = record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token-new",
        _payload(
            "StopFailure",
            session_id="session-1",
            turn_id="turn-1",
        ),
    )
    assert (failed.status, failed.event, failed.turn_id) == (
        "idle",
        "StopFailure",
        None,
    )
    failed_activity = next(
        event
        for event in read_events(tmp_path, "g1")
        if event.event_type == "StopFailure"
    )
    assert (failed_activity.kind, failed_activity.status) == ("turn", "failed")
    stopped = record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token-new",
        _payload("SessionEnd", session_id="session-1"),
    )
    assert stopped.status == "stopped"
    assert stopped.session_closed
    assert record_terminal_input(tmp_path, "claude", "g1", "peer") is None


def test_claude_generation_is_owned_by_logical_input_and_interrupt(tmp_path: Path) -> None:
    begin_launch(tmp_path, "claude", "g1", "peer", "token")
    record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        "token",
        _payload("SessionStart", session_id="session-1"),
    )
    first = record_terminal_input(tmp_path, "claude", "g1", "peer")
    assert first is not None
    assert first.turn_id == "local:1"
    interrupted = record_interrupt(tmp_path, "claude", "g1", "peer")
    assert interrupted is not None
    assert (interrupted.status, interrupted.interrupted) == ("idle", True)
    second = record_terminal_input(tmp_path, "claude", "g1", "peer")
    assert second is not None
    assert second.turn_id == "local:2"


def test_claude_completion_hook_closes_only_an_active_local_turn(tmp_path: Path) -> None:
    begin_launch(tmp_path, "claude", "g1", "peer", "token")
    idle = record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        "token",
        _payload("SessionStart", session_id="session-1"),
    )
    assert record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        "token",
        _payload("Stop", session_id="session-1"),
    ) == idle

    active = record_terminal_input(tmp_path, "claude", "g1", "peer")
    assert active is not None
    completed = record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        "token",
        _payload("Stop", session_id="session-1"),
    )
    assert (completed.status, completed.event, completed.turn_id) == (
        "idle",
        "Stop",
        None,
    )

    next_turn = record_terminal_input(tmp_path, "claude", "g1", "peer")
    assert next_turn is not None
    assert next_turn.turn_id == "local:2"
    completed_by_notification = record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        "token",
        _payload(
            "Notification",
            session_id="session-1",
            notification_type="agent_completed",
        ),
    )
    assert (
        completed_by_notification.status,
        completed_by_notification.event,
        completed_by_notification.turn_id,
    ) == ("idle", "Notification", None)

    assert record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        "old-token",
        _payload("Stop", session_id="session-1"),
    ) == completed_by_notification
    assert record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        "token",
        _payload("Stop", session_id="old-session"),
    ) == completed_by_notification


def test_cross_process_lock_serializes_interrupt_and_late_hook(tmp_path: Path) -> None:
    begin_launch(tmp_path, "claude", "g1", "peer", "token")
    record_hook_event(
        tmp_path,
        "claude",
        "g1",
        "peer",
        "token",
        _payload("SessionStart", session_id="session-1"),
    )
    record_terminal_input(tmp_path, "claude", "g1", "peer")
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures = [
            pool.submit(record_interrupt, tmp_path, "claude", "g1", "peer"),
            pool.submit(
                record_hook_event,
                tmp_path,
                "claude",
                "g1",
                "peer",
                "token",
                _payload("UserPromptSubmit", session_id="session-1", prompt_id="stale"),
            ),
        ]
        for future in futures:
            future.result()
    final = read_state(tmp_path, "claude", "g1", "peer")
    assert final is not None
    assert (final.status, final.event, final.interrupted) == ("idle", "UserInterrupt", True)


def test_launch_reset_and_old_token_hook_are_serialized_in_threads(
    tmp_path: Path,
) -> None:
    begin_launch(tmp_path, "codex", "g1", "peer", "token-old")
    record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token-old",
        _payload("SessionStart", session_id="old-session"),
    )
    with ThreadPoolExecutor(max_workers=2) as pool:
        futures = [
            pool.submit(
                begin_launch,
                tmp_path,
                "codex",
                "g1",
                "peer",
                "token-new",
                "HookUnavailableCommand",
            ),
            pool.submit(
                record_hook_event,
                tmp_path,
                "codex",
                "g1",
                "peer",
                "token-old",
                _payload(
                    "UserPromptSubmit",
                    session_id="old-session",
                    turn_id="late-turn",
                ),
            ),
        ]
        for future in futures:
            future.result()
    final = read_state(tmp_path, "codex", "g1", "peer")
    assert final is not None
    assert final.launch_token == "token-new"
    assert final.event == "HookUnavailableCommand"
    assert final.session_id == ""
    assert final.turn_generation == 0


def test_launch_reset_and_old_token_hook_are_serialized_across_processes(
    tmp_path: Path,
) -> None:
    begin_launch(tmp_path, "codex", "g1", "peer", "token-old")
    record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token-old",
        _payload("SessionStart", session_id="old-session"),
    )
    reset_script = """
from pathlib import Path
import sys
from cccc.kernel.runtime_hooks.store import begin_launch
begin_launch(Path(sys.argv[1]), "codex", "g1", "peer", "token-new", "HookUnavailableCommand")
"""
    late_script = """
from pathlib import Path
import sys
from cccc.kernel.runtime_hooks.store import record_hook_event
record_hook_event(
    Path(sys.argv[1]), "codex", "g1", "peer", "token-old",
    {"hook_event_name": "UserPromptSubmit", "session_id": "old-session", "turn_id": "late-turn"},
)
"""
    env = {**os.environ, "PYTHONPATH": str(Path(__file__).parents[1] / "src")}
    processes = [
        subprocess.Popen(
            [sys.executable, "-c", script, str(tmp_path)],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        for script in (reset_script, late_script)
    ]
    for process in processes:
        _stdout, stderr = process.communicate(timeout=10)
        assert process.returncode == 0, stderr
    final = read_state(tmp_path, "codex", "g1", "peer")
    assert final is not None
    assert final.launch_token == "token-new"
    assert final.event == "HookUnavailableCommand"
    assert final.session_id == ""
    assert final.turn_generation == 0


def test_failed_replace_does_not_expose_half_committed_state(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    begin_launch(tmp_path, "codex", "g1", "peer", "token-old")
    path = state_path(tmp_path, "codex", "g1", "peer")
    before = path.read_bytes()

    def fail_replace(_source: object, _target: object) -> None:
        raise OSError("simulated replace failure")

    monkeypatch.setattr(
        "cccc.kernel.runtime_hooks.committed_io.os.replace", fail_replace
    )
    with pytest.raises(OSError, match="replace failure"):
        begin_launch(tmp_path, "codex", "g1", "peer", "token-new")
    assert path.read_bytes() == before
    assert read_state(tmp_path, "codex", "g1", "peer").launch_token == "token-old"  # type: ignore[union-attr]
    assert sorted(item.suffix for item in path.parent.iterdir()) == [".json", ".lock"]


def test_directory_fsync_error_after_replace_accepts_verified_commit(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(
        "cccc.kernel.runtime_hooks.committed_io._sync_directory",
        lambda _path: (_ for _ in ()).throw(OSError("directory fsync failed")),
    )
    state = begin_launch(tmp_path, "codex", "g1", "peer", "token-new")
    assert state.launch_token == "token-new"
    assert read_state(tmp_path, "codex", "g1", "peer") == state


def test_strict_reader_rejects_corrupt_or_identity_mismatched_state(tmp_path: Path) -> None:
    path = state_path(tmp_path, "codex", "g1", "peer")
    path.parent.mkdir(parents=True)
    path.write_text("{broken", encoding="utf-8")
    with pytest.raises(ValueError):
        read_state(tmp_path, "codex", "g1", "peer")


def test_eligible_hook_state_is_single_source_for_working_state(tmp_path: Path) -> None:
    begin_launch(tmp_path, "claude", "g1", "peer", "token")
    _write_eligible_identity(tmp_path, "claude", "g1", "peer", "token")
    projection = read_working_projection(tmp_path, "claude", "g1", "peer")
    result = derive_effective_working_state(
        running=True,
        effective_runner="pty",
        runtime="claude",
        idle_seconds=0,
        pty_terminal_text="old prompt heuristic says working",
        runtime_hook_projection=projection,
    )
    assert result["effective_working_state"] == "waiting"
    assert (
        result["effective_working_reason"]
        == "claude_pty_fail_closed_HookPending"
    )


def test_corrupt_eligible_state_fails_closed(tmp_path: Path) -> None:
    _write_eligible_identity(tmp_path, "codex", "g1", "peer", "token")
    path = state_path(tmp_path, "codex", "g1", "peer")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("{broken", encoding="utf-8")
    projection = read_working_projection(tmp_path, "codex", "g1", "peer")
    assert projection is not None
    assert projection["effective_working_state"] == "waiting"
    assert projection["effective_working_reason"] == "codex_hook_state_unavailable"


def test_v2_state_is_read_only(tmp_path: Path) -> None:
    legacy = {
        **begin_launch(tmp_path, "codex", "g1", "peer", "token").to_dict(),
        "v": 2,
    }
    path = state_path(tmp_path, "codex", "g1", "peer")
    path.write_text(json.dumps(legacy), encoding="utf-8")
    before = path.read_bytes()
    state = record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token",
        _payload("SessionStart", session_id="session-1"),
    )
    assert state.v == 2
    assert path.read_bytes() == before

    path.write_text(
        json.dumps(
            {
                **begin_launch(tmp_path, "codex", "g1", "other", "token").to_dict(),
                "actor_id": "other",
            }
        ),
        encoding="utf-8",
    )
    with pytest.raises(ValueError):
        read_state(tmp_path, "codex", "g1", "peer")
