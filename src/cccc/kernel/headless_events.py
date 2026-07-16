from __future__ import annotations

import json
import uuid
from pathlib import Path
from typing import Any, Dict, List

from .ledger import read_last_lines
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.time import utc_now_iso

_HEADLESS_REPLAY_START_TYPES = {
    "headless.turn.started",
    "headless.control.queued",
    "headless.control.started",
    "headless.control.requeued",
}
_HEADLESS_REPLAY_END_TYPES = {
    "headless.turn.completed",
    "headless.turn.failed",
    "headless.control.completed",
    "headless.control.failed",
}


def headless_events_path(group_dir: Path) -> Path:
    return group_dir / "state" / "headless" / "events.jsonl"


def headless_events_lock_path(group_dir: Path) -> Path:
    return group_dir / "state" / "headless" / "events.lock"


def append_headless_event(group_dir: Path, *, group_id: str, actor_id: str, event_type: str, data: Dict[str, Any]) -> Dict[str, Any]:
    payload = {
        "id": uuid.uuid4().hex,
        "ts": utc_now_iso(),
        "group_id": str(group_id or "").strip(),
        "actor_id": str(actor_id or "").strip(),
        "type": str(event_type or "").strip(),
        "data": data if isinstance(data, dict) else {},
    }
    if not payload["group_id"] or not payload["actor_id"] or not payload["type"]:
        raise ValueError("missing headless event fields")

    path = headless_events_path(group_dir)
    path.parent.mkdir(parents=True, exist_ok=True)
    line = json.dumps(payload, ensure_ascii=False)
    lock = acquire_lockfile(headless_events_lock_path(group_dir), blocking=True)
    try:
        with path.open("a", encoding="utf-8") as handle:
            handle.write(line + "\n")
    finally:
        release_lockfile(lock)
    return payload


def read_headless_replay_lines(group_dir: Path, *, limit: int = 400) -> List[str]:
    path = headless_events_path(group_dir)
    try:
        raw_lines = read_last_lines(path, max(50, int(limit or 400)))
    except Exception:
        return []

    indexed: list[tuple[int, str, str, str]] = []
    for idx, raw in enumerate(raw_lines):
        try:
            payload = json.loads(raw)
        except Exception:
            continue
        if not isinstance(payload, dict):
            continue
        actor_id = str(payload.get("actor_id") or "").strip()
        event_type = str(payload.get("type") or "").strip()
        if not actor_id or not event_type:
            continue
        indexed.append((idx, raw, actor_id, event_type))

    active_start_by_actor: dict[str, int] = {}
    latest_completed_start_by_actor: dict[str, int] = {}
    latest_seen_start_by_actor: dict[str, int] = {}
    first_seen_by_actor: dict[str, int] = {}
    for idx, _raw, actor_id, event_type in indexed:
        first_seen_by_actor.setdefault(actor_id, idx)
        if event_type in _HEADLESS_REPLAY_START_TYPES:
            active_start_by_actor[actor_id] = idx
            latest_seen_start_by_actor[actor_id] = idx
            continue
        if event_type in _HEADLESS_REPLAY_END_TYPES:
            latest_completed_start_by_actor[actor_id] = active_start_by_actor.pop(
                actor_id,
                latest_seen_start_by_actor.get(actor_id, first_seen_by_actor.get(actor_id, idx)),
            )

    replay_start_by_actor = dict(latest_completed_start_by_actor)
    replay_start_by_actor.update(active_start_by_actor)
    if not replay_start_by_actor:
        return []

    replay_lines: list[str] = []
    for idx, raw, actor_id, _event_type in indexed:
        start_idx = replay_start_by_actor.get(actor_id)
        if start_idx is None or idx < start_idx:
            continue
        replay_lines.append(raw)
    return replay_lines


def read_headless_replay_events(group_dir: Path, *, limit: int = 400) -> List[Dict[str, Any]]:
    events: list[Dict[str, Any]] = []
    for raw in read_headless_replay_lines(group_dir, limit=limit):
        try:
            payload = json.loads(raw)
        except Exception:
            continue
        if isinstance(payload, dict):
            events.append(payload)
    return events
