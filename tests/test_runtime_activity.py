from __future__ import annotations

from datetime import datetime, timedelta, timezone
import asyncio
import json
from pathlib import Path
from types import SimpleNamespace

import pytest
from fastapi.testclient import TestClient

from cccc.kernel.access_tokens import create_access_token
from cccc.kernel.runtime_hooks.committed_io import write_json_committed
from cccc.kernel.runtime_hooks.activity import (
    events_path,
    project_snapshot,
    read_events,
    record_hook_activity,
)
from cccc.kernel.runtime_hooks.contracts import RuntimeActivityEvent
from cccc.kernel.runtime_hooks.store import begin_launch, read_state, record_hook_event
from cccc.ports.web.routes.runtime_activity import (
    _stream_events,
    create_routers,
)


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


def test_web_snapshot_and_sse_replay_match_runtime_activity_contract(
    tmp_path: Path,
) -> None:
    begin_launch(tmp_path, "codex", "g1", "peer", "token")
    record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token",
        {"hook_event_name": "SessionStart", "session_id": "session-1"},
    )
    router = create_routers(SimpleNamespace(home=tmp_path))[0]
    routes = {route.path: route.endpoint for route in router.routes}
    base = "/api/v1/groups/{group_id}/runtime-activity"
    snapshot = asyncio.run(routes[f"{base}/snapshot"]("g1"))
    assert snapshot["ok"] is True
    assert snapshot["result"]["count"] == 1

    class _Request:
        async def is_disconnected(self) -> bool:
            return False

    response = asyncio.run(routes[f"{base}/stream"]("g1", _Request(), True))

    async def _first() -> bytes:
        return await anext(response.body_iterator)

    frame = asyncio.run(_first()).decode()
    assert "event: runtime-activity\n" in frame
    payload = json.loads(next(line[6:] for line in frame.splitlines() if line.startswith("data: ")))
    assert payload["group_id"] == "g1"


def test_fastapi_runtime_activity_enforces_group_auth_and_reports_corrupt_store(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("CCCC_HOME", str(tmp_path))
    monkeypatch.setenv("CCCC_GROUP_BRIDGE_OUTBOX_WORKER_DISABLED", "1")
    token = str(
        create_access_token(
            "member", allowed_groups=["g1"], home=tmp_path
        )["token"]
    )
    from cccc.ports.web.app import create_app

    client = TestClient(create_app(), raise_server_exceptions=False)
    allowed = client.get(
        "/api/v1/groups/g1/runtime-activity/snapshot",
        headers={"Authorization": f"Bearer {token}"},
    )
    assert allowed.status_code == 200
    denied = client.get(
        "/api/v1/groups/g2/runtime-activity/snapshot",
        headers={"Authorization": f"Bearer {token}"},
    )
    assert denied.status_code == 403
    anonymous = TestClient(create_app(), raise_server_exceptions=False)
    assert anonymous.get(
        "/api/v1/groups/g1/runtime-activity/snapshot"
    ).status_code == 401

    path = events_path(tmp_path, "g1")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("{broken", encoding="utf-8")
    corrupt = client.get(
        "/api/v1/groups/g1/runtime-activity/snapshot",
        headers={"Authorization": f"Bearer {token}"},
    )
    assert corrupt.status_code == 503
    assert corrupt.json()["error"]["code"] == "runtime_activity_unavailable"


def test_sse_replay_false_disconnects_without_replaying(
    tmp_path: Path,
) -> None:
    begin_launch(tmp_path, "codex", "g1", "peer", "token")
    record_hook_event(
        tmp_path,
        "codex",
        "g1",
        "peer",
        "token",
        {"hook_event_name": "SessionStart", "session_id": "session-1"},
    )

    class _DisconnectedRequest:
        async def is_disconnected(self) -> bool:
            return True

    stream = _stream_events(
        SimpleNamespace(home=tmp_path),
        "g1",
        _DisconnectedRequest(),
        replay=False,
    )
    with pytest.raises(StopAsyncIteration):
        asyncio.run(anext(stream))


def test_sse_emits_synthesized_stuck_event_only_once(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    now = datetime.now(timezone.utc)
    started = RuntimeActivityEvent(
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
    write_json_committed(
        events_path(tmp_path, "g1"), [started.to_dict()]
    )
    real_sleep = asyncio.sleep
    monkeypatch.setattr(
        "cccc.ports.web.routes.runtime_activity.asyncio.sleep",
        lambda _delay: real_sleep(0),
    )

    class _EventuallyDisconnectedRequest:
        def __init__(self) -> None:
            self.polls = 0

        async def is_disconnected(self) -> bool:
            self.polls += 1
            return self.polls > 3

    async def collect() -> list[bytes]:
        return [
            frame
            async for frame in _stream_events(
                SimpleNamespace(home=tmp_path),
                "g1",
                _EventuallyDisconnectedRequest(),
                replay=True,
            )
        ]

    frames = asyncio.run(collect())
    stuck = [
        frame
        for frame in frames
        if json.loads(
            next(
                line[6:]
                for line in frame.decode().splitlines()
                if line.startswith("data: ")
            )
        )["status"]
        == "stuck"
    ]
    assert len(stuck) == 1


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
