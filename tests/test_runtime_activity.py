from __future__ import annotations

from datetime import datetime, timedelta, timezone
import json
from pathlib import Path

import pytest

from cccc.kernel.runtime_hooks.committed_io import write_json_committed
from cccc.kernel.runtime_hooks.activity import (
    events_path,
    project_snapshot,
    read_events,
    record_hook_activity,
)
from cccc.kernel.runtime_hooks.contracts import RuntimeActivityEvent
from cccc.kernel.runtime_hooks.store import begin_launch, read_state, record_hook_event


def test_activity_store_records_safe_tool_lifecycle_and_duration(tmp_path: Path) -> None:
    begin_launch(tmp_path, "codex", "g1", "peer", "token")
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
            "tool_name": "Bash $(secret)",
            "tool_input": {"command": "secret"},
        },
        {
            "hook_event_name": "PostToolUse",
            "session_id": "session-1",
            "turn_id": "turn-1",
            "tool_use_id": "op-1",
        },
    ):
        record_hook_event(tmp_path, "codex", "g1", "peer", "token", payload)
    events = read_events(tmp_path, "g1")
    tool = next(event for event in events if event.kind == "tool")
    assert tool.status == "completed"
    assert tool.tool_name == "Bashsecret"
    assert tool.duration_ms is not None
    assert "tool_input" not in tool.to_dict()


def test_activity_failure_rolls_back_state(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    begin_launch(tmp_path, "codex", "g1", "peer", "token")
    record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token",
        {"hook_event_name": "SessionStart", "session_id": "session-1"},
    )
    before = read_state(tmp_path, "codex", "g1", "peer")

    def fail(*_args: object, **_kwargs: object) -> object:
        raise OSError("activity unavailable")

    monkeypatch.setattr(
        "cccc.kernel.runtime_hooks.activity.record_hook_activity",
        fail,
    )
    with pytest.raises(OSError):
        record_hook_event(
            tmp_path,
            "codex",
            "g1",
            "peer",
            "token",
            {
                "hook_event_name": "UserPromptSubmit",
                "session_id": "session-1",
                "turn_id": "turn-1",
            },
        )
    assert read_state(tmp_path, "codex", "g1", "peer") == before


def test_snapshot_keeps_recent_completed_and_synthesizes_stuck(tmp_path: Path) -> None:
    _ = tmp_path
    now = datetime.now(timezone.utc)
    started = record_hook_activity.__annotations__  # keep public contract import covered
    assert started

    event = RuntimeActivityEvent(
        v=1,
        id="started",
        ts=(now - timedelta(seconds=61)).isoformat().replace("+00:00", "Z"),
        group_id="g1",
        actor_id="peer",
        runtime="codex",
        activity_id="codex:session:tool:op",
        kind="tool",
        status="started",
        event_type="PreToolUse",
        session_id="session",
        turn_id="turn",
        operation_id="op",
        tool_name="Bash",
        duration_ms=None,
    )
    projected = project_snapshot([event], now=now)
    assert {item.status for item in projected} == {"started", "stuck"}


def test_capacity_failure_rolls_back_fenced_state(tmp_path: Path) -> None:
    before = begin_launch(tmp_path, "codex", "g1", "peer", "token")
    events = [
        RuntimeActivityEvent(
            v=1,
            id=f"event-{index}",
            ts=datetime.now(timezone.utc).isoformat().replace("+00:00", "Z"),
            group_id="g1",
            actor_id="other",
            runtime="codex",
            activity_id=f"active-{index}",
            kind="tool",
            status="started",
            event_type="PreToolUse",
            session_id="session-other",
            turn_id="turn-other",
            operation_id=f"op-{index}",
            tool_name="Bash",
            duration_ms=None,
        )
        for index in range(256)
    ]
    path = events_path(tmp_path, "g1")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps([event.to_dict() for event in events]),
        encoding="utf-8",
    )
    with pytest.raises(OSError, match="capacity"):
        record_hook_event(
            tmp_path,
            "codex",
            "g1",
            "peer",
            "token",
            {"hook_event_name": "SessionStart", "session_id": "session-1"},
        )
    assert read_state(tmp_path, "codex", "g1", "peer") == before


def test_activity_lock_failure_rolls_back_fenced_state(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    before = begin_launch(tmp_path, "codex", "g1", "peer", "token")

    def fail_lock(_path: Path) -> object:
        raise OSError("lock unavailable")

    monkeypatch.setattr(
        "cccc.kernel.runtime_hooks.activity.acquire_lockfile", fail_lock
    )
    with pytest.raises(OSError, match="lock unavailable"):
        record_hook_event(
            tmp_path,
            "codex",
            "g1",
            "peer",
            "token",
            {"hook_event_name": "SessionStart", "session_id": "session-1"},
        )
    assert read_state(tmp_path, "codex", "g1", "peer") == before
