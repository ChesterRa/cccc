from __future__ import annotations

import json
import hashlib
import os
import uuid
from pathlib import Path
from typing import Any, Dict, List

from .ledger import read_last_lines
from .headless_event_migration import iter_dedupe_payloads
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

_DEDUPE_SCAN_BYTES = 256 * 1024
_DEDUPE_SCAN_LINES = 4096
_DEDUPE_READY = "index.ready"
_DEDUPE_PENDING = "pending.json"
_MAX_PENDING_LINE_BYTES = 1024 * 1024


def headless_events_path(group_dir: Path) -> Path:
    return group_dir / "state" / "headless" / "events.jsonl"


def headless_events_lock_path(group_dir: Path) -> Path:
    return group_dir / "state" / "headless" / "events.lock"


def _write_dedupe_marker(path: Path, key: str, payload: Dict[str, Any]) -> None:
    temp = path.with_name(f"{path.name}.tmp-{os.getpid()}")
    with temp.open("w", encoding="utf-8") as marker:
        marker.write(f"{key}\n{json.dumps(payload, ensure_ascii=False)}\n")
        marker.flush()
        os.fsync(marker.fileno())
    os.replace(temp, path)


def _serialize_event_line(payload: Dict[str, Any]) -> bytes:
    return json.dumps(
        payload,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def _write_pending(
    marker_dir: Path,
    key: str,
    payload: Dict[str, Any],
    *,
    offset: int,
    line: bytes,
) -> None:
    if len(line) > _MAX_PENDING_LINE_BYTES:
        raise OSError("deepseek dedupe pending line exceeds cap")
    try:
        line_text = line.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise OSError("deepseek dedupe pending line is not UTF-8") from exc
    event_id = str(payload.get("id") or "")
    if not event_id or payload.get("dedupe_key") != key:
        raise OSError("deepseek dedupe pending identity mismatch")
    temp = marker_dir / f"{_DEDUPE_PENDING}.tmp-{os.getpid()}"
    pending = marker_dir / _DEDUPE_PENDING
    with temp.open("w", encoding="utf-8") as handle:
        json.dump(
            {
                "schema": 1,
                "key": key,
                "event_id": event_id,
                "offset": int(offset),
                "line_len": len(line),
                "line": line_text,
                "event": payload,
            },
            handle,
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        )
        handle.write("\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(temp, pending)


def _find_event(path: Path, key: str, event_id: str = "") -> bool:
    if path.stat().st_size > _DEDUPE_SCAN_BYTES:
        return False
    lines = path.read_text(encoding="utf-8").splitlines()
    if len(lines) > _DEDUPE_SCAN_LINES:
        return False
    for raw in lines:
        value = json.loads(raw)
        if not isinstance(value, dict):
            continue
        if value.get("dedupe_key") == key or (event_id and value.get("id") == event_id):
            return True
    return False


def _recover_pending(path: Path, marker_dir: Path) -> None:
    pending_path = marker_dir / _DEDUPE_PENDING
    if not pending_path.exists():
        return
    pending = json.loads(pending_path.read_text(encoding="utf-8"))
    key = str(pending.get("key") or "")
    event = pending.get("event")
    if not key or not isinstance(event, dict):
        raise OSError("invalid deepseek dedupe pending record")
    if pending.get("schema") != 1:
        raise OSError("unsupported deepseek dedupe pending schema")
    event_id = str(pending.get("event_id") or "")
    line = pending.get("line")
    line_len = pending.get("line_len")
    offset = pending.get("offset")
    if (
        not event_id
        or event.get("id") != event_id
        or event.get("dedupe_key") != key
        or not isinstance(line, str)
        or not isinstance(line_len, int)
        or not isinstance(offset, int)
        or offset < 0
    ):
        raise OSError("invalid deepseek dedupe pending identity")
    line_bytes = line.encode("utf-8")
    if line_len != len(line_bytes) or line_len > _MAX_PENDING_LINE_BYTES:
        raise OSError("invalid deepseek dedupe pending line length")
    try:
        if json.loads(line) != event:
            raise OSError("deepseek dedupe pending payload diverged")
    except json.JSONDecodeError as exc:
        raise OSError("invalid deepseek dedupe pending line") from exc
    marker = marker_dir / f"{hashlib.sha256(key.encode('utf-8')).hexdigest()}.marker"
    _ensure_event_at_offset(path, offset, line_bytes, event, key)
    if not marker.exists() or marker.read_text(encoding="utf-8").splitlines()[:1] != [key]:
        _write_dedupe_marker(marker, key, event)
    pending_path.unlink(missing_ok=True)


def _ensure_event_at_offset(path: Path, offset: int, line: bytes, event: Dict[str, Any], key: str) -> None:
    size = path.stat().st_size
    if offset > size:
        raise OSError("pending dedupe offset beyond event log")
    if offset == size:
        with path.open("ab") as handle:
            handle.write(line + b"\n")
            handle.flush()
            os.fsync(handle.fileno())
        return
    needed = offset + len(line) + 1
    if needed > size:
        raise OSError("pending dedupe event is truncated")
    with path.open("rb") as handle:
        handle.seek(offset)
        actual = handle.read(len(line) + 1)
    if actual != line + b"\n":
        raise OSError("pending dedupe event diverged")
    try:
        parsed = json.loads(line)
    except json.JSONDecodeError as exc:
        raise OSError("pending dedupe event is invalid JSON") from exc
    if parsed != event or parsed.get("id") != event.get("id") or parsed.get("dedupe_key") != key:
        raise OSError("pending dedupe event identity mismatch")


def _ensure_dedupe_index(path: Path, marker_dir: Path) -> None:
    ready = marker_dir / _DEDUPE_READY
    if ready.exists():
        return
    for payload in iter_dedupe_payloads(path):
        key = str(payload["dedupe_key"])
        marker = marker_dir / f"{hashlib.sha256(key.encode('utf-8')).hexdigest()}.marker"
        marker_key = marker.read_text(encoding="utf-8").splitlines()[:1] if marker.exists() else []
        if marker_key != [key]:
            _write_dedupe_marker(marker, key, payload)
    ready_tmp = marker_dir / f"{_DEDUPE_READY}.tmp-{os.getpid()}"
    with ready_tmp.open("w", encoding="utf-8") as handle:
        handle.write("ready\n")
        handle.flush()
        os.fsync(handle.fileno())
    os.replace(ready_tmp, ready)


def has_headless_event_dedupe(group_dir: Path, dedupe_key: str) -> bool:
    key = str(dedupe_key or "").strip()
    if not key:
        return False
    path = headless_events_path(group_dir)
    if not path.is_file():
        return False
    lock = acquire_lockfile(headless_events_lock_path(group_dir), blocking=True)
    try:
        marker_dir = path.parent / "events.dedupe"
        marker_dir.mkdir(parents=True, exist_ok=True)
        _recover_pending(path, marker_dir)
        _ensure_dedupe_index(path, marker_dir)
        marker = marker_dir / f"{hashlib.sha256(key.encode('utf-8')).hexdigest()}.marker"
        if not marker.is_file():
            return False
        return marker.read_text(encoding="utf-8").splitlines()[:1] == [key]
    except (OSError, ValueError, json.JSONDecodeError):
        return False
    finally:
        release_lockfile(lock)


def append_headless_event(
    group_dir: Path,
    *,
    group_id: str,
    actor_id: str,
    event_type: str,
    data: Dict[str, Any],
    dedupe_key: str | None = None,
) -> Dict[str, Any]:
    payload = {
        "id": uuid.uuid4().hex,
        "ts": utc_now_iso(),
        "group_id": str(group_id or "").strip(),
        "actor_id": str(actor_id or "").strip(),
        "type": str(event_type or "").strip(),
        "data": data if isinstance(data, dict) else {},
    }
    if dedupe_key:
        payload["dedupe_key"] = str(dedupe_key)
    if not payload["group_id"] or not payload["actor_id"] or not payload["type"]:
        raise ValueError("missing headless event fields")

    path = headless_events_path(group_dir)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch(exist_ok=True)
    lock = acquire_lockfile(headless_events_lock_path(group_dir), blocking=True)
    try:
        marker = None
        marker_dir = path.parent / "events.dedupe"
        marker_dir.mkdir(parents=True, exist_ok=True)
        _recover_pending(path, marker_dir)
        if dedupe_key:
            _ensure_dedupe_index(path, marker_dir)
            marker = marker_dir / f"{hashlib.sha256(dedupe_key.encode('utf-8')).hexdigest()}.marker"
            if marker.exists():
                marker_lines = marker.read_text(encoding="utf-8").splitlines()
                if marker_lines and marker_lines[0] == dedupe_key:
                    if len(marker_lines) > 1:
                        existing = json.loads(marker_lines[1])
                        if isinstance(existing, dict):
                            return existing
                    return payload
                # A torn marker can only be repaired from a bounded legacy log.
                if _find_event(path, dedupe_key):
                    for raw_line in path.read_text(encoding="utf-8").splitlines():
                        existing = json.loads(raw_line)
                        if isinstance(existing, dict) and existing.get("dedupe_key") == dedupe_key:
                            _write_dedupe_marker(marker, dedupe_key, existing)
                            return existing
                raise OSError("deepseek dedupe marker is invalid")
            line = _serialize_event_line(payload)
            offset = path.stat().st_size
            _write_pending(marker_dir, dedupe_key, payload, offset=offset, line=line)
        else:
            line = _serialize_event_line(payload)
        with path.open("ab") as handle:
            handle.write(line + b"\n")
            handle.flush()
            os.fsync(handle.fileno())
        if marker is not None:
            _write_dedupe_marker(marker, dedupe_key, payload)
            (marker.parent / _DEDUPE_PENDING).unlink(missing_ok=True)
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
