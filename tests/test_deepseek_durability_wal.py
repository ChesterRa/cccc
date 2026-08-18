from __future__ import annotations

import json
import os


def test_pending_wal_recovers_after_marker_failure_and_fresh_path(tmp_path, monkeypatch) -> None:
    from cccc.kernel import headless_events

    first = headless_events.append_headless_event(
        tmp_path,
        group_id="g",
        actor_id="deepseek",
        event_type="headless.message.delta",
        data={"event_id": "event-1"},
        dedupe_key="deepseek.update:event-1:0",
    )
    path = tmp_path / "state" / "headless" / "events.jsonl"
    next((path.parent / "events.dedupe").glob("*.marker")).unlink()
    original = headless_events._write_dedupe_marker

    def fail_marker(*args, **kwargs):
        raise OSError("injected marker failure")

    monkeypatch.setattr(headless_events, "_write_dedupe_marker", fail_marker)
    try:
        headless_events.append_headless_event(
            tmp_path,
            group_id="g",
            actor_id="deepseek",
            event_type="headless.message.delta",
            data={"event_id": "event-1"},
            dedupe_key="deepseek.update:event-1:1",
        )
    except OSError:
        pass
    monkeypatch.setattr(headless_events, "_write_dedupe_marker", original)
    recovered = headless_events.append_headless_event(
        tmp_path,
        group_id="g",
        actor_id="deepseek",
        event_type="headless.message.delta",
        data={"event_id": "event-1"},
        dedupe_key="deepseek.update:event-1:1",
    )
    assert recovered["id"] != first["id"]
    assert len(path.read_text(encoding="utf-8").splitlines()) == 2


def test_pending_wal_uses_stable_offset_after_ready_log_growth(tmp_path, monkeypatch) -> None:
    from cccc.kernel import headless_events

    headless_events.append_headless_event(
        tmp_path,
        group_id="g",
        actor_id="deepseek",
        event_type="headless.message.delta",
        data={"event_id": "event-1"},
        dedupe_key="deepseek.update:event-1:0",
    )
    path = tmp_path / "state" / "headless" / "events.jsonl"
    with path.open("ab") as handle:
        handle.write(b'{"filler":1}\n' * 30000)
        handle.flush()
        os.fsync(handle.fileno())

    original = headless_events._write_dedupe_marker

    def fail_marker(*args, **kwargs):
        raise OSError("injected marker failure")

    monkeypatch.setattr(headless_events, "_write_dedupe_marker", fail_marker)
    try:
        headless_events.append_headless_event(
            tmp_path,
            group_id="g",
            actor_id="deepseek",
            event_type="headless.message.delta",
            data={"event_id": "event-2"},
            dedupe_key="deepseek.update:event-2:0",
        )
    except OSError:
        pass
    finally:
        monkeypatch.setattr(headless_events, "_write_dedupe_marker", original)

    pending = tmp_path / "state" / "headless" / "events.dedupe" / "pending.json"
    record = json.loads(pending.read_text(encoding="utf-8"))
    assert record["offset"] + record["line_len"] + 1 <= path.stat().st_size
    recovered = headless_events.append_headless_event(
        tmp_path,
        group_id="g",
        actor_id="deepseek",
        event_type="headless.message.delta",
        data={"event_id": "event-2"},
        dedupe_key="deepseek.update:event-2:0",
    )
    lines = path.read_text(encoding="utf-8").splitlines()
    assert sum('"deepseek.update:event-2:0"' in line for line in lines) == 1
    assert recovered["id"] == record["event_id"]


def test_pending_wal_is_recovered_before_non_dedupe_writer(tmp_path) -> None:
    from cccc.kernel import headless_events

    path = tmp_path / "state" / "headless" / "events.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch()
    key = "deepseek.update:event-reserved:0"
    event = {
        "id": "reserved-output",
        "ts": "2026-01-01T00:00:00Z",
        "group_id": "g",
        "actor_id": "deepseek",
        "type": "headless.message.delta",
        "dedupe_key": key,
        "data": {"event_id": "event-reserved"},
    }
    marker_dir = path.parent / "events.dedupe"
    marker_dir.mkdir()
    line = headless_events._serialize_event_line(event)
    headless_events._write_pending(marker_dir, key, event, offset=0, line=line)

    headless_events.append_headless_event(
        tmp_path,
        group_id="g",
        actor_id="deepseek",
        event_type="headless.permission.responded",
        data={"event_id": "permission-1"},
    )
    lines = path.read_text(encoding="utf-8").splitlines()
    assert json.loads(lines[0])["dedupe_key"] == key
    assert json.loads(lines[1])["type"] == "headless.permission.responded"
    assert not (marker_dir / "pending.json").exists()


def test_invalid_marker_on_ready_large_log_fails_closed(tmp_path) -> None:
    from cccc.kernel import headless_events

    key = "deepseek.update:event-invalid:0"
    headless_events.append_headless_event(
        tmp_path,
        group_id="g",
        actor_id="deepseek",
        event_type="headless.message.delta",
        data={"event_id": "event-invalid"},
        dedupe_key=key,
    )
    path = tmp_path / "state" / "headless" / "events.jsonl"
    with path.open("ab") as handle:
        handle.write(b'{"filler":1}\n' * 30000)
        handle.flush()
        os.fsync(handle.fileno())
    marker = next((path.parent / "events.dedupe").glob("*.marker"))
    marker.write_text("corrupt\n", encoding="utf-8")
    try:
        headless_events.append_headless_event(
            tmp_path,
            group_id="g",
            actor_id="deepseek",
            event_type="headless.message.delta",
            data={"event_id": "event-invalid"},
            dedupe_key=key,
        )
    except OSError as exc:
        assert "marker is invalid" in str(exc)
    else:
        raise AssertionError("invalid marker must fail closed")
    assert sum(key in line for line in path.read_text(encoding="utf-8").splitlines()) == 1
