from __future__ import annotations

import json
import uuid
from dataclasses import replace
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Mapping, Optional, Sequence

from ...util.file_lock import acquire_lockfile, release_lockfile
from ...util.time import utc_now_iso
from .committed_io import write_json_committed
from .contracts import (
    ACTIVITY_EVENT_LIMIT,
    ACTIVITY_RETENTION_SECONDS,
    ACTIVITY_VERSION,
    HookState,
    RuntimeActivityEvent,
)
from .activity_projection import project_hook_activity
from .projection import project_snapshot


def _group_state_dir(home: Path, group_id: str) -> Path:
    group = str(group_id or "").strip()
    if not group or group in {".", ".."} or "/" in group or "\\" in group:
        raise ValueError("invalid group identity")
    return Path(home) / "groups" / group / "state" / "runtime-activity"


def events_path(home: Path, group_id: str) -> Path:
    return _group_state_dir(home, group_id) / "events.json"


def _lock_path(home: Path, group_id: str) -> Path:
    return _group_state_dir(home, group_id) / "events.lock"


def _parse_time(value: str) -> datetime:
    parsed = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return parsed.astimezone(timezone.utc)


def _read_events_unlocked(home: Path, group_id: str) -> list[RuntimeActivityEvent]:
    path = events_path(home, group_id)
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return []
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ValueError(f"invalid runtime activity file: {path}") from exc
    if not isinstance(raw, list):
        raise ValueError("runtime activity store must be an array")
    return [RuntimeActivityEvent.from_dict(item) for item in raw]


def _write_events(home: Path, group_id: str, events: Sequence[RuntimeActivityEvent]) -> None:
    write_json_committed(
        events_path(home, group_id),
        [event.to_dict() for event in events],
    )


def _prune(events: list[RuntimeActivityEvent], now: datetime) -> list[RuntimeActivityEvent]:
    kept: list[RuntimeActivityEvent] = []
    for event in events:
        try:
            age = (now - _parse_time(event.ts)).total_seconds()
        except (TypeError, ValueError):
            continue
        if age <= ACTIVITY_RETENTION_SECONDS:
            kept.append(event)
    return _enforce_limit(kept)


def _active(event: RuntimeActivityEvent) -> bool:
    return event.status in {"started", "waiting", "stuck"}


def _enforce_limit(events: list[RuntimeActivityEvent]) -> list[RuntimeActivityEvent]:
    result = list(events)
    while len(result) > ACTIVITY_EVENT_LIMIT:
        index = next((i for i, item in enumerate(result) if not _active(item)), None)
        if index is None:
            index = next(
                (
                    i
                    for i, item in enumerate(result)
                    if item.status in {"waiting", "stuck"}
                ),
                None,
            )
        if index is None:
            raise OSError("runtime activity capacity exhausted by active events")
        result.pop(index)
    return result


def record_hook_activity(
    home: Path,
    runtime: str,
    launch_token: str,
    payload: Mapping[str, Any],
    state: HookState,
) -> Optional[RuntimeActivityEvent]:
    draft = project_hook_activity(runtime, launch_token, payload, state)
    if draft is None:
        return None
    lock = acquire_lockfile(_lock_path(home, state.group_id))
    try:
        events = _prune(
            _read_events_unlocked(home, state.group_id), datetime.now(timezone.utc)
        )
        terminalized = 0
        if draft["kind"] == "session" and draft["status"] == "completed":
            terminalized = _terminalize(
                events, state, "SessionEnded", "failed", utc_now_iso()
            )
        elif draft["kind"] == "turn" and draft["status"] == "started":
            terminalized = _terminalize(
                events, state, "TurnSuperseded", "failed", utc_now_iso()
            )
        if runtime == "claude" and draft["kind"] == "session":
            if terminalized:
                _write_events(home, state.group_id, _enforce_limit(events))
            return None
        if any(
            event.actor_id == state.actor_id
            and event.activity_id == draft["activity_id"]
            and event.event_type == draft["event_type"]
            and event.status == draft["status"]
            for event in reversed(events)
        ):
            return None
        now = utc_now_iso()
        started = next(
            (
                event
                for event in reversed(events)
                if event.actor_id == state.actor_id
                and event.activity_id == draft["activity_id"]
                and event.status == "started"
            ),
            None,
        )
        duration_ms = None
        if draft["status"] in {"completed", "failed"} and started is not None:
            duration_ms = max(
                0, int((_parse_time(now) - _parse_time(started.ts)).total_seconds() * 1000)
            )
        tool_name = draft["tool_name"] or next(
            (
                event.tool_name
                for event in reversed(events)
                if event.actor_id == state.actor_id
                and event.activity_id == draft["activity_id"]
                and event.tool_name
            ),
            None,
        )
        events = [
            event
            for event in events
            if (
                event.actor_id != state.actor_id
                or event.activity_id != draft["activity_id"]
                or (draft["status"] == "waiting" and event.status == "started")
            )
        ]
        event = RuntimeActivityEvent(
            v=ACTIVITY_VERSION,
            id=uuid.uuid4().hex,
            ts=now,
            group_id=state.group_id,
            actor_id=state.actor_id,
            runtime=runtime,
            activity_id=draft["activity_id"],
            kind=draft["kind"],
            status=draft["status"],
            event_type=draft["event_type"],
            session_id=state.session_id,
            turn_id=draft["turn_id"],
            operation_id=draft["operation_id"],
            tool_name=tool_name,
            duration_ms=duration_ms,
        )
        events.append(event)
        _write_events(home, state.group_id, _enforce_limit(events))
        return event
    finally:
        release_lockfile(lock)


def close_actor_activities(
    home: Path, state: HookState, event_type: str
) -> None:
    lock = acquire_lockfile(_lock_path(home, state.group_id))
    try:
        events = _prune(
            _read_events_unlocked(home, state.group_id), datetime.now(timezone.utc)
        )
        if _terminalize(events, state, event_type, "failed", utc_now_iso()):
            _write_events(home, state.group_id, _enforce_limit(events))
    finally:
        release_lockfile(lock)


def _terminalize(
    events: list[RuntimeActivityEvent],
    state: HookState,
    event_type: str,
    status: str,
    now: str,
) -> int:
    latest: dict[tuple[str, str], RuntimeActivityEvent] = {}
    for event in events:
        if (
            event.actor_id == state.actor_id
            and event.session_id == state.session_id
            and event.kind != "session"
            and _active(event)
        ):
            latest[(event.actor_id, event.activity_id)] = event
    for active in latest.values():
        started = next(
            (
                event
                for event in events
                if event.actor_id == active.actor_id
                and event.activity_id == active.activity_id
                and event.status == "started"
            ),
            active,
        )
        try:
            duration = max(
                0, int((_parse_time(now) - _parse_time(started.ts)).total_seconds() * 1000)
            )
        except (TypeError, ValueError):
            duration = None
        events[:] = [
            event
            for event in events
            if event.actor_id != active.actor_id
            or event.activity_id != active.activity_id
        ]
        events.append(
            replace(
                active,
                id=uuid.uuid4().hex,
                ts=now,
                status=status,
                event_type=event_type,
                duration_ms=duration,
            )
        )
    return len(latest)


def read_events(home: Path, group_id: str) -> list[RuntimeActivityEvent]:
    return _prune(
        _read_events_unlocked(Path(home), group_id), datetime.now(timezone.utc)
    )
