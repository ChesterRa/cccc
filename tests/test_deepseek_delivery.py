from __future__ import annotations

import os
import json

from cccc.daemon.actors import deepseek_runtime
from cccc.daemon.messaging.deepseek_delivery import deliver_messages
from cccc.daemon.messaging.delivery import PendingMessage
from cccc.kernel.group import Group
from cccc.kernel.headless_events import read_headless_replay_events
from cccc.kernel.ledger import append_event


def _fake_acp_script() -> str:
    return r'''while IFS= read -r line; do
if printf '%s' "$line" | grep -q '"method":"initialize"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"fake"}}}'
elif printf '%s' "$line" | grep -q '"method":"session/new"'; then
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"fake-session"}}'
elif printf '%s' "$line" | grep -q '"prompt":\[' && printf '%s' "$line" | grep -q '"type":"text"'; then
  printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","updateOrdinal":999999,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Hel"}}}}'
  printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"fake-session","updateOrdinal":0,"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"lo"}}}}'
  rid=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "${rid:-3}"
else
  printf '%s\n' '{"jsonrpc":"2.0","id":3,"error":{"message":"prompt must be ContentBlock[]"}}'
fi
done'''


def test_deepseek_delivery_persists_output_before_success(tmp_path) -> None:
    group = Group(group_id="deepseek-delivery", path=tmp_path, doc={"group_id": "deepseek-delivery", "actors": [], "automation": {}})
    deepseek_runtime.start(
        group_id=group.group_id,
        actor_id="deepseek",
        actor_created_at="",
        group_path=group.path,
        cwd=tmp_path,
        command=["sh", "-c", _fake_acp_script()],
        env={**os.environ, "CCCC_HOME": str(tmp_path / "cccc-home")},
    )
    try:
        message = PendingMessage(
            event_id="event-1",
            by="user",
            to=["deepseek"],
            text="hello",
            ts="2026-01-01T00:00:00Z",
        )
        assert deliver_messages(group, actor_id="deepseek", messages=[message]) is True
        assert deliver_messages(group, actor_id="deepseek", messages=[message]) is True
        raw = (tmp_path / "state" / "headless" / "events.jsonl").read_text(encoding="utf-8")
        assert raw.count("headless.message.delta") == 2
        assert "headless.message.completed" in raw
        assert "headless.turn.started" in raw
        assert "headless.turn.completed" in raw
        replay_types = [event["type"] for event in read_headless_replay_events(tmp_path)]
        assert replay_types == [
            "headless.turn.started",
            "headless.message.delta",
            "headless.message.delta",
            "headless.message.completed",
            "headless.turn.completed",
        ]
    finally:
        deepseek_runtime.stop(group_id=group.group_id, actor_id="deepseek")


def test_large_event_log_reuses_the_durable_completion_marker(
    tmp_path, monkeypatch
) -> None:
    from cccc.kernel.headless_events import append_headless_event

    group = Group(
        group_id="deepseek-large-recovery",
        path=tmp_path,
        doc={"group_id": "deepseek-large-recovery", "actors": [], "automation": {}},
    )
    event_id = "event-already-completed"
    append_headless_event(
        tmp_path,
        group_id=group.group_id,
        actor_id="deepseek",
        event_type="headless.turn.completed",
        data={"event_id": event_id},
        dedupe_key=f"deepseek.turn:headless.turn.completed:{event_id}",
    )
    events = tmp_path / "state" / "headless" / "events.jsonl"
    with events.open("ab") as handle:
        handle.write(b"x" * (4 * 1024 * 1024 + 1) + b"\n")

    class ExistingSession:
        session_id = "existing-session"

        def submit(self, _prompt: str) -> int:
            raise AssertionError("a durable completed event must not be submitted again")

    monkeypatch.setattr(deepseek_runtime, "get", lambda **_kwargs: ExistingSession())
    message = PendingMessage(
        event_id=event_id,
        by="user",
        to=["deepseek"],
        text="do not repeat",
        ts="2026-01-01T00:00:00Z",
    )
    assert deliver_messages(group, actor_id="deepseek", messages=[message]) is True


def test_terminal_append_failure_keeps_source_delivery_failed(tmp_path, monkeypatch) -> None:
    group = Group(group_id="deepseek-terminal-failure", path=tmp_path, doc={"group_id": "deepseek-terminal-failure", "actors": [], "automation": {}})
    deepseek_runtime.start(
        group_id=group.group_id,
        actor_id="deepseek",
        actor_created_at="",
        group_path=group.path,
        cwd=tmp_path,
        command=["sh", "-c", _fake_acp_script()],
        env={**os.environ, "CCCC_HOME": str(tmp_path / "cccc-home")},
    )
    import cccc.daemon.messaging.deepseek_delivery as adapter

    original_append = adapter.append_headless_event

    def fail_terminal(*args, **kwargs):
        if kwargs.get("event_type") == "headless.turn.completed":
            raise OSError("injected terminal append failure")
        return original_append(*args, **kwargs)

    monkeypatch.setattr(adapter, "append_headless_event", fail_terminal)
    try:
        message = PendingMessage(event_id="event-terminal-failure", by="user", to=["deepseek"], text="hello", ts="2026-01-01T00:00:00Z")
        assert deliver_messages(group, actor_id="deepseek", messages=[message]) is False
        raw = (tmp_path / "state" / "headless" / "events.jsonl").read_text(encoding="utf-8")
        assert "headless.message.delta" in raw
        assert "headless.turn.completed" not in raw
    finally:
        deepseek_runtime.stop(group_id=group.group_id, actor_id="deepseek")


def test_shared_durability_vector_preserves_failed_prefix() -> None:
    import json
    from pathlib import Path

    vector = json.loads(
        (Path(__file__).parent / "fixtures" / "deepseek_durability_vectors.json").read_text(encoding="utf-8")
    )
    assert vector["expected"]["cursor"] is None
    assert vector["expected"]["delivered_prefix"] == []
    from cccc.kernel.headless_events import append_headless_event

    with __import__("tempfile").TemporaryDirectory() as directory:
        root = Path(directory)
        first = append_headless_event(
            root,
            group_id="g",
            actor_id="deepseek",
            event_type="headless.turn.completed",
            data={"event_id": "event-1"},
            dedupe_key="deepseek.turn.completed:event-1",
        )
        second = append_headless_event(
            root,
            group_id="g",
            actor_id="deepseek",
            event_type="headless.turn.completed",
            data={"event_id": "event-1"},
            dedupe_key="deepseek.turn.completed:event-1",
        )
        assert first["id"] == second["id"]
        assert len((root / "state" / "headless" / "events.jsonl").read_text().splitlines()) == 1


def test_timeout_cancels_and_confirms_terminal_before_retry(tmp_path, monkeypatch) -> None:
    group = Group(
        group_id="deepseek-timeout",
        path=tmp_path,
        doc={"group_id": "deepseek-timeout", "actors": [], "automation": {}},
    )

    class FakeSupervisor:
        session_id = "fake-session"

        def __init__(self) -> None:
            self.cancelled = 0
            self.reads = 0

        def submit(self, _prompt: str) -> int:
            return 3

        def next_frame(self, *, timeout: float):
            del timeout
            self.reads += 1
            if self.reads == 1:
                raise TimeoutError("turn timed out")
            return {"jsonrpc": "2.0", "id": 3, "result": {"stopReason": "cancelled"}}

        def cancel(self) -> None:
            self.cancelled += 1

        def respond_permission(self, *_args, **_kwargs) -> None:
            raise AssertionError("no permission request expected")

    supervisor = FakeSupervisor()
    monkeypatch.setattr(deepseek_runtime, "get", lambda **_kwargs: supervisor)
    monkeypatch.setattr(
        deepseek_runtime,
        "stop",
        lambda **_kwargs: (_ for _ in ()).throw(AssertionError("confirmed cancel must not force-stop")),
    )
    message = PendingMessage(
        event_id="event-timeout",
        by="user",
        to=["deepseek"],
        text="hello",
        ts="2026-01-01T00:00:00Z",
    )
    assert deliver_messages(group, actor_id="deepseek", messages=[message], timeout=0.1) is False
    assert supervisor.cancelled == 1
    events = (tmp_path / "state" / "headless" / "events.jsonl").read_text(encoding="utf-8")
    assert "headless.turn.failed" in events
    assert '"code":"timeout"' in events


def test_failed_attempt_output_does_not_hide_successful_retry(tmp_path, monkeypatch) -> None:
    group = Group(
        group_id="deepseek-retry-output",
        path=tmp_path,
        doc={"group_id": "deepseek-retry-output", "actors": [], "automation": {}},
    )

    class FakeSupervisor:
        session_id = "fake-session"

        def __init__(self) -> None:
            self.request_id = 2
            self.frames: list[dict] = []

        def submit(self, _prompt: str) -> int:
            self.request_id += 1
            text = "partial" if self.request_id == 3 else "complete"
            terminal = (
                {"jsonrpc": "2.0", "id": self.request_id, "error": {"message": "temporary"}}
                if self.request_id == 3
                else {
                    "jsonrpc": "2.0",
                    "id": self.request_id,
                    "result": {"stopReason": "end_turn"},
                }
            )
            self.frames = [
                {
                    "jsonrpc": "2.0",
                    "method": "session/update",
                    "params": {
                        "sessionId": self.session_id,
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "content": {"type": "text", "text": text},
                        },
                    },
                },
                terminal,
            ]
            return self.request_id

        def next_frame(self, *, timeout: float):
            del timeout
            return self.frames.pop(0)

        def cancel(self) -> None:
            raise AssertionError("terminal response must not be cancelled")

        def respond_permission(self, *_args, **_kwargs) -> None:
            raise AssertionError("no permission request expected")

    supervisor = FakeSupervisor()
    monkeypatch.setattr(deepseek_runtime, "get", lambda **_kwargs: supervisor)
    message = PendingMessage(
        event_id="event-retry-output",
        by="user",
        to=["deepseek"],
        text="hello",
        ts="2026-01-01T00:00:00Z",
    )

    assert deliver_messages(group, actor_id="deepseek", messages=[message]) is False
    assert deliver_messages(group, actor_id="deepseek", messages=[message]) is True

    events = [
        json.loads(line)
        for line in (tmp_path / "state" / "headless" / "events.jsonl")
        .read_text(encoding="utf-8")
        .splitlines()
    ]
    assert [
        event["data"]["delta"]
        for event in events
        if event["type"] == "headless.message.delta"
    ] == ["partial", "complete"]
    assert [
        event["data"]["text"]
        for event in events
        if event["type"] == "headless.message.completed"
    ] == ["partial", "complete"]


def test_missing_credential_is_structured_secret_free_and_stops_runtime(
    tmp_path, monkeypatch
) -> None:
    group = Group(
        group_id="deepseek-missing-credential",
        path=tmp_path,
        doc={"group_id": "deepseek-missing-credential", "actors": [], "automation": {}},
    )

    class FakeSupervisor:
        session_id = "fake-session"
        generation = "launch-credential"

        def submit(self, _prompt: str) -> int:
            return 3

        def next_frame(self, *, timeout: float):
            del timeout
            return {
                "jsonrpc": "2.0",
                "id": 3,
                "error": {
                    "message": "no API key for DEEPSEEK_API_KEY; diagnostic=should-not-leak"
                },
            }

    blocked: list[dict[str, object]] = []
    monkeypatch.setattr(deepseek_runtime, "get", lambda **_kwargs: FakeSupervisor())
    monkeypatch.setattr(
        deepseek_runtime,
        "require_manual_restart",
        lambda **kwargs: blocked.append(kwargs) or True,
    )
    message = PendingMessage(
        event_id="event-missing-credential",
        by="user",
        to=["deepseek"],
        text="hello",
        ts="2026-01-01T00:00:00Z",
    )

    assert deliver_messages(group, actor_id="deepseek", messages=[message]) is False
    assert len(blocked) == 1
    assert blocked[0]["group_id"] == group.group_id
    assert blocked[0]["actor_id"] == "deepseek"
    assert blocked[0]["group_path"] == group.path
    assert blocked[0]["expected_generation"] == "launch-credential"
    assert blocked[0]["reason_code"] == "credential_unavailable"
    events = (tmp_path / "state" / "headless" / "events.jsonl").read_text(encoding="utf-8")
    assert '"code":"credential_unavailable"' in events
    assert '"category":"environment"' in events
    assert "DeepSeek API credential is not configured" in events
    assert "should-not-leak" not in events


def test_context_overflow_is_structured_and_stops_runtime(tmp_path, monkeypatch) -> None:
    group = Group(
        group_id="deepseek-context-overflow",
        path=tmp_path,
        doc={"group_id": "deepseek-context-overflow", "actors": [], "automation": {}},
    )

    class FakeSupervisor:
        session_id = "oversized-session"
        generation = "launch-context"

        def submit(self, _prompt: str) -> int:
            return 3

        def next_frame(self, *, timeout: float):
            del timeout
            return {
                "jsonrpc": "2.0",
                "id": 3,
                "error": {
                    "code": -32603,
                    "message": "This model request failed",
                    "data": (
                        "maximum context length is 1048576 tokens; "
                        "diagnostic=should-not-leak"
                    ),
                },
            }

    blocked: list[dict[str, object]] = []
    monkeypatch.setattr(deepseek_runtime, "get", lambda **_kwargs: FakeSupervisor())
    monkeypatch.setattr(
        deepseek_runtime,
        "require_manual_restart",
        lambda **kwargs: blocked.append(kwargs) or True,
    )
    message = PendingMessage(
        event_id="event-context-overflow",
        by="user",
        to=["deepseek"],
        text="hello",
        ts="2026-01-01T00:00:00Z",
    )

    assert deliver_messages(group, actor_id="deepseek", messages=[message]) is False
    assert len(blocked) == 1
    assert blocked[0]["group_id"] == group.group_id
    assert blocked[0]["actor_id"] == "deepseek"
    assert blocked[0]["group_path"] == group.path
    assert blocked[0]["expected_generation"] == "launch-context"
    assert blocked[0]["reason_code"] == "context_window_exceeded"
    events = (tmp_path / "state" / "headless" / "events.jsonl").read_text(encoding="utf-8")
    assert '"code":"context_window_exceeded"' in events
    assert '"category":"context"' in events
    assert "restart the actor to create a fresh session" in events
    assert "should-not-leak" not in events
