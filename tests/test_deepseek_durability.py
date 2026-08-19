from __future__ import annotations

import json

from cccc.kernel.group import Group

def test_marker_dedupe_survives_large_history(tmp_path) -> None:
    from cccc.kernel.headless_events import append_headless_event

    group = Group(
        group_id="deepseek-bounds",
        path=tmp_path,
        doc={"group_id": "deepseek-bounds", "actors": [{"id": "deepseek", "runtime": "deepseek", "runner": "headless"}], "automation": {}},
    )
    first = append_headless_event(
        tmp_path,
        group_id=group.group_id,
        actor_id="deepseek",
        event_type="headless.message.delta",
        data={"event_id": "event-1"},
        dedupe_key="deepseek.update:event-1:0",
    )
    for ordinal in range(1, 320):
        append_headless_event(
            tmp_path,
            group_id=group.group_id,
            actor_id="deepseek",
            event_type="headless.message.delta",
            data={"payload": "x" * 1024},
            dedupe_key=f"deepseek.update:event-1:{ordinal}",
        )
    path = tmp_path / "state" / "headless" / "events.jsonl"
    line_count = len(path.read_text(encoding="utf-8").splitlines())
    second = append_headless_event(
        tmp_path,
        group_id=group.group_id,
        actor_id="deepseek",
        event_type="headless.message.delta",
        data={"event_id": "event-1"},
        dedupe_key="deepseek.update:event-1:0",
    )
    assert first["id"] == second["id"]
    assert len(path.read_text(encoding="utf-8").splitlines()) == line_count
    assert len(path.read_text(encoding="utf-8").splitlines()) > 256


def test_large_legacy_headless_log_builds_dedupe_index(tmp_path) -> None:
    from cccc.kernel.headless_events import append_headless_event

    path = tmp_path / "state" / "headless" / "events.jsonl"
    path.parent.mkdir(parents=True)
    with path.open("w", encoding="utf-8") as handle:
        for index in range(5000):
            handle.write(
                json.dumps(
                    {
                        "id": f"legacy-{index}",
                        "group_id": "g",
                        "actor_id": "codex",
                        "type": "headless.message.delta",
                        "dedupe_key": "legacy-key" if index == 2500 else None,
                        "data": {"delta": "x" * 64},
                    },
                    separators=(",", ":"),
                )
                + "\n"
            )
    original_lines = len(path.read_text(encoding="utf-8").splitlines())
    assert path.stat().st_size > 256 * 1024
    assert original_lines > 4096
    append_headless_event(
        tmp_path,
        group_id="g",
        actor_id="deepseek",
        event_type="headless.message.delta",
        data={"event_id": "legacy-source"},
        dedupe_key="legacy-key",
    )
    assert len(path.read_text(encoding="utf-8").splitlines()) == original_lines
    assert (path.parent / "events.dedupe" / "index.ready").exists()


def test_oversized_legacy_event_without_dedupe_identity_does_not_block_new_output(tmp_path) -> None:
    from cccc.kernel.headless_events import append_headless_event

    path = tmp_path / "state" / "headless" / "events.jsonl"
    path.parent.mkdir(parents=True)
    path.write_text(
        json.dumps(
            {
                "id": "legacy-large",
                "group_id": "g",
                "actor_id": "codex",
                "type": "headless.item.completed",
                "data": {"item": "x" * (1024 * 1024 + 32)},
            },
            separators=(",", ":"),
        )
        + "\n",
        encoding="utf-8",
    )

    append_headless_event(
        tmp_path,
        group_id="g",
        actor_id="deepseek",
        event_type="headless.turn.started",
        data={"event_id": "new-source"},
        dedupe_key="deepseek.turn.started:new-source",
    )

    assert len(path.read_text(encoding="utf-8").splitlines()) == 2
    assert (path.parent / "events.dedupe" / "index.ready").exists()


def test_oversized_event_claiming_dedupe_identity_still_fails_closed(tmp_path) -> None:
    from cccc.kernel.headless_events import append_headless_event

    path = tmp_path / "state" / "headless" / "events.jsonl"
    path.parent.mkdir(parents=True)
    path.write_text(
        json.dumps(
            {
                "id": "legacy-large",
                "group_id": "g",
                "actor_id": "deepseek",
                "type": "headless.message.delta",
                "dedupe_key": "legacy-key",
                "data": {"delta": "x" * (1024 * 1024 + 32)},
            },
            separators=(",", ":"),
        )
        + "\n",
        encoding="utf-8",
    )

    try:
        append_headless_event(
            tmp_path,
            group_id="g",
            actor_id="deepseek",
            event_type="headless.message.delta",
            data={"event_id": "legacy-source"},
            dedupe_key="legacy-key",
        )
    except OSError as exc:
        assert "has dedupe identity" in str(exc)
    else:
        raise AssertionError("oversized dedupe-bearing event must fail closed")
    assert not (path.parent / "events.dedupe" / "index.ready").exists()
