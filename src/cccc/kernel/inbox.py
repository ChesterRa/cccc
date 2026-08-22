from __future__ import annotations

import hashlib
import json
import os
import re
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, List, Literal, Optional, Tuple

from .context import ContextStorage
from ..util.fs import atomic_write_json, read_json
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.time import parse_utc_iso, utc_now_iso
from .actors import find_actor, get_effective_role, is_internal_actor, list_actors
from .group import Group
from .ledger_index import (
    lookup_event_by_id,
    lookup_event_positions,
    lookup_events_by_ids,
    lookup_latest_actor_add_boundaries,
    lookup_latest_actor_add_positions,
    search_event_ids_indexed,
)
from .ledger_segments import iter_source_lines, list_ledger_sources
from .ledger_state_snapshot import can_replay_from_basis, current_ledger_basis, load_latest_ledger_snapshot


# Message kind filter
MessageKindFilter = Literal["all", "chat", "notify"]

_UNREAD_INDEX_SCHEMA = 3
_MAIL_CURSOR_SCHEMA = 1
_FULL_EVENT_ID_RE = re.compile(r"^[0-9a-fA-F]{32}$")


def _is_full_event_id(value: str) -> bool:
    return bool(_FULL_EVENT_ID_RE.fullmatch(str(value or "").strip()))


def iter_events(ledger_path: Path) -> Iterable[Dict[str, Any]]:
    """Iterate over all events in sealed segments followed by the active ledger."""
    for source in list_ledger_sources(ledger_path.parent):
        abs_path = source.get("abs_path")
        if not isinstance(abs_path, Path) or not abs_path.exists():
            continue
        for line in iter_source_lines(abs_path):
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except Exception:
                continue
            if isinstance(obj, dict):
                yield obj


def iter_events_reverse(ledger_path: Path, *, block_size: int = 65536) -> Iterable[Dict[str, Any]]:
    """Iterate over ledger events from newest to oldest across all sources."""
    sources = list_ledger_sources(ledger_path.parent)
    for source in reversed(sources):
        abs_path = source.get("abs_path")
        if not isinstance(abs_path, Path) or not abs_path.exists():
            continue
        if str(abs_path.name).endswith(".gz"):
            events: List[Dict[str, Any]] = []
            for line in iter_source_lines(abs_path):
                line = line.strip()
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except Exception:
                    continue
                if isinstance(obj, dict):
                    events.append(obj)
            for ev in reversed(events):
                if isinstance(ev, dict):
                    yield ev
            continue
        try:
            with abs_path.open("rb") as f:
                f.seek(0, os.SEEK_END)
                file_size = f.tell()
                buffer = b""
                pos = file_size
                while pos > 0:
                    read_size = min(max(1024, int(block_size or 65536)), pos)
                    pos -= read_size
                    f.seek(pos)
                    chunk = f.read(read_size)
                    if not chunk:
                        continue
                    buffer = chunk + buffer
                    parts = buffer.split(b"\n")
                    buffer = parts[0]
                    for raw_line in reversed(parts[1:]):
                        line = raw_line.strip()
                        if not line:
                            continue
                        try:
                            obj = json.loads(line.decode("utf-8", errors="replace"))
                        except Exception:
                            continue
                        if isinstance(obj, dict):
                            yield obj
                tail = buffer.strip()
                if tail:
                    try:
                        obj = json.loads(tail.decode("utf-8", errors="replace"))
                    except Exception:
                        obj = None
                    if isinstance(obj, dict):
                        yield obj
        except Exception:
            events: List[Dict[str, Any]] = []
            for line in iter_source_lines(abs_path):
                line = line.strip()
                if not line:
                    continue
                try:
                    obj = json.loads(line)
                except Exception:
                    continue
                if isinstance(obj, dict):
                    events.append(obj)
            for ev in reversed(events):
                yield ev


def _latest_actor_add_event_ids(group: Group, actor_ids: Iterable[str]) -> Dict[str, str]:
    """Return the latest actor.add event id for each requested actor."""
    normalized = [
        str(actor_id or "").strip()
        for actor_id in actor_ids
        if str(actor_id or "").strip() and str(actor_id or "").strip() != "user"
    ]
    if not normalized:
        return {}
    return {
        actor_id: event_id
        for actor_id, (event_id, _position) in lookup_latest_actor_add_boundaries(
            group.ledger_path, normalized
        ).items()
        if event_id
    }


def _cursor_path(group: Group) -> Path:
    return group.path / "state" / "read_cursors.json"


def _cursor_lock_path(group: Group) -> Path:
    return _cursor_path(group).with_name("read_cursors.json.lock")


def _pending_read_path(group: Group) -> Path:
    return _cursor_path(group).with_name("read_cursors.pending.json")


def _unread_index_path(group: Group) -> Path:
    return group.path / "state" / "unread_index.json"


def _load_unread_index(group: Group) -> Dict[str, Any]:
    raw = read_json(_unread_index_path(group))
    if not isinstance(raw, dict):
        raw = {}
    counts_raw = raw.get("counts")
    counts: Dict[str, int] = {}
    if isinstance(counts_raw, dict):
        for actor_id, value in counts_raw.items():
            aid = str(actor_id or "").strip()
            if not aid:
                continue
            try:
                counts[aid] = max(0, int(value or 0))
            except Exception:
                counts[aid] = 0
    ledger_basis_raw = raw.get("ledger_basis") if isinstance(raw.get("ledger_basis"), dict) else {}
    ledger_basis = {
        "segment_ids": [str(item).strip() for item in (ledger_basis_raw.get("segment_ids") if isinstance(ledger_basis_raw.get("segment_ids"), list) else []) if str(item).strip()],
        "active_size": max(0, int(ledger_basis_raw.get("active_size") or 0)) if isinstance(ledger_basis_raw, dict) else 0,
        "active_mtime_ns": max(0, int(ledger_basis_raw.get("active_mtime_ns") or 0)) if isinstance(ledger_basis_raw, dict) else 0,
        "active_prefix_sha256": str(ledger_basis_raw.get("active_prefix_sha256") or "") if isinstance(ledger_basis_raw, dict) else "",
    }
    if not ledger_basis["segment_ids"] and not ledger_basis["active_size"]:
        try:
            legacy_size = max(0, int(raw.get("ledger_size") or 0))
        except Exception:
            legacy_size = 0
        ledger_basis = {"segment_ids": [], "active_size": legacy_size, "active_mtime_ns": 0}
    try:
        actors_rev = max(0, int(raw.get("actors_rev") or 0))
    except Exception:
        actors_rev = 0
    try:
        schema = max(0, int(raw.get("schema") or 0))
    except Exception:
        schema = 0
    return {
        "schema": schema,
        "actors_rev": actors_rev,
        "cursor_sig": str(raw.get("cursor_sig") or ""),
        "ledger_basis": ledger_basis,
        "counts": counts,
        "updated_at": str(raw.get("updated_at") or ""),
    }


def _save_unread_index(
    group: Group,
    *,
    actors_rev: int,
    cursor_sig: str,
    ledger_basis: Dict[str, Any],
    counts: Dict[str, int],
) -> Dict[str, Any]:
    out = {
        "schema": _UNREAD_INDEX_SCHEMA,
        "actors_rev": max(0, int(actors_rev or 0)),
        "cursor_sig": str(cursor_sig or ""),
        "ledger_basis": {
            "segment_ids": [
                str(item).strip()
                for item in (ledger_basis.get("segment_ids") if isinstance(ledger_basis.get("segment_ids"), list) else [])
                if str(item).strip()
            ],
            "active_size": max(0, int(ledger_basis.get("active_size") or 0)),
            "active_mtime_ns": max(0, int(ledger_basis.get("active_mtime_ns") or 0)),
            "active_prefix_sha256": str(ledger_basis.get("active_prefix_sha256") or ""),
        },
        "counts": {str(actor_id): max(0, int(value or 0)) for actor_id, value in counts.items() if str(actor_id or "").strip()},
        "updated_at": utc_now_iso(),
    }
    p = _unread_index_path(group)
    p.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_json(p, out)
    return out


def _current_actors_rev(group: Group) -> int:
    try:
        return max(0, int(ContextStorage(group).load_version_state().get("actors_rev") or 0))
    except Exception:
        return 0


def _cursor_sig_for_actor_ids(group: Group, actor_ids: List[str]) -> str:
    cursors = load_cursors(group)
    digest = hashlib.sha256()
    for actor_id in sorted({str(item or "").strip() for item in actor_ids if str(item or "").strip()}):
        cur = cursors.get(actor_id)
        event_id = str(cur.get("event_id") or "") if isinstance(cur, dict) else ""
        ts = str(cur.get("ts") or "") if isinstance(cur, dict) else ""
        digest.update(actor_id.encode("utf-8"))
        digest.update(b"\0")
        digest.update(event_id.encode("utf-8"))
        digest.update(b"\0")
        digest.update(ts.encode("utf-8"))
        digest.update(b"\n")
    return digest.hexdigest()


def _iter_events_from_offset(ledger_path: Path, start: int) -> Tuple[List[Dict[str, Any]], int]:
    out: List[Dict[str, Any]] = []
    if not ledger_path.exists():
        return out, 0
    offset = max(0, int(start or 0))
    with ledger_path.open("rb") as handle:
        handle.seek(offset, os.SEEK_SET)
        while True:
            line = handle.readline()
            if not line:
                break
            try:
                obj = json.loads(line.decode("utf-8", errors="replace").strip())
            except Exception:
                return [], -1
            if isinstance(obj, dict):
                out.append(obj)
        return out, int(handle.tell())


def _seed_unread_index_from_snapshot(
    group: Group,
    *,
    actors_rev: int,
    cursor_sig: str,
    ledger_basis: Dict[str, Any],
) -> Optional[Dict[str, Any]]:
    snapshot = load_latest_ledger_snapshot(group)
    state = snapshot.get("state") if isinstance(snapshot.get("state"), dict) else {}
    unread_index = state.get("unread_index") if isinstance(state.get("unread_index"), dict) else {}
    if not unread_index:
        return None
    try:
        if int(unread_index.get("schema") or 0) != _UNREAD_INDEX_SCHEMA:
            return None
    except Exception:
        return None
    if int(unread_index.get("actors_rev") or 0) != actors_rev:
        return None
    if str(unread_index.get("cursor_sig") or "") != cursor_sig:
        return None
    snapshot_basis = unread_index.get("ledger_basis") if isinstance(unread_index.get("ledger_basis"), dict) else {}
    if not can_replay_from_basis(snapshot_basis, ledger_basis):
        return None
    counts = unread_index.get("counts") if isinstance(unread_index.get("counts"), dict) else {}
    return {
        "actors_rev": actors_rev,
        "cursor_sig": cursor_sig,
        "ledger_basis": snapshot_basis,
        "counts": {str(actor_id): max(0, int(value or 0)) for actor_id, value in counts.items() if str(actor_id or "").strip()},
    }


def _apply_unread_delta(
    group: Group,
    *,
    actors: List[Dict[str, Any]],
    counts: Dict[str, int],
    events: List[Dict[str, Any]],
) -> Dict[str, int]:
    next_counts = {aid: max(0, int(counts.get(aid, 0))) for aid in counts}
    actor_ids = [str(actor.get("id") or "").strip() for actor in actors if str(actor.get("id") or "").strip()]
    actor_roles = {aid: get_effective_role(group, aid) for aid in actor_ids}

    # `events` is the suffix after the persisted ledger basis. Its append order is
    # authoritative even if two timestamps collide or the wall clock moves backwards.
    for ev in events:
        ev_kind = str(ev.get("kind") or "")
        if not _is_mail_message(ev):
            continue
        ev_by = str(ev.get("by") or "").strip()
        for aid in actor_ids:
            if ev_by == aid:
                continue
            if not is_message_for_actor(group, actor_id=aid, event=ev, role=actor_roles.get(aid)):
                continue
            next_counts[aid] = max(0, int(next_counts.get(aid, 0)) + 1)
    return next_counts


def get_indexed_unread_counts(
    group: Group,
    *,
    actors: List[Dict[str, Any]],
) -> Dict[str, int]:
    """Return unread counts from the persisted unread snapshot when possible.

    Semantics are intentionally bound to current actor topology via `actors_rev`.
    If actor membership/order changes, the snapshot is rebuilt from ledger truth.
    """
    actor_ids = [str(actor.get("id") or "").strip() for actor in actors if str(actor.get("id") or "").strip()]
    if not actor_ids:
        return {}

    actors_rev = _current_actors_rev(group)
    cursor_sig = _cursor_sig_for_actor_ids(group, actor_ids)
    ledger_basis = current_ledger_basis(group)
    snapshot = _load_unread_index(group)
    snapshot_counts = snapshot.get("counts") if isinstance(snapshot.get("counts"), dict) else {}
    snapshot_basis = snapshot.get("ledger_basis") if isinstance(snapshot.get("ledger_basis"), dict) else {}

    if (
        int(snapshot.get("schema") or 0) == _UNREAD_INDEX_SCHEMA
        and int(snapshot.get("actors_rev") or 0) == actors_rev
        and str(snapshot.get("cursor_sig") or "") == cursor_sig
        and snapshot_basis == ledger_basis
    ):
        return {aid: max(0, int(snapshot_counts.get(aid, 0))) for aid in actor_ids}

    if (
        int(snapshot.get("schema") or 0) == _UNREAD_INDEX_SCHEMA
        and int(snapshot.get("actors_rev") or 0) == actors_rev
        and str(snapshot.get("cursor_sig") or "") == cursor_sig
        and can_replay_from_basis(snapshot_basis, ledger_basis)
    ):
        delta_events, end_offset = _iter_events_from_offset(group.ledger_path, int(snapshot_basis.get("active_size") or 0))
        if end_offset >= 0:
            next_counts = _apply_unread_delta(
                group,
                actors=actors,
                counts={aid: max(0, int(snapshot_counts.get(aid, 0))) for aid in actor_ids},
                events=delta_events,
            )
            _save_unread_index(
                group,
                actors_rev=actors_rev,
                cursor_sig=cursor_sig,
                ledger_basis={**ledger_basis, "active_size": end_offset},
                counts=next_counts,
            )
            return next_counts

    seeded = _seed_unread_index_from_snapshot(
        group,
        actors_rev=actors_rev,
        cursor_sig=cursor_sig,
        ledger_basis=ledger_basis,
    )
    if seeded is not None:
        delta_events, end_offset = _iter_events_from_offset(group.ledger_path, int(((seeded.get("ledger_basis") if isinstance(seeded.get("ledger_basis"), dict) else {}) or {}).get("active_size") or 0))
        if end_offset >= 0:
            next_counts = _apply_unread_delta(
                group,
                actors=actors,
                counts={aid: max(0, int((seeded.get("counts") if isinstance(seeded.get("counts"), dict) else {}).get(aid, 0))) for aid in actor_ids},
                events=delta_events,
            )
            _save_unread_index(
                group,
                actors_rev=actors_rev,
                cursor_sig=cursor_sig,
                ledger_basis={**ledger_basis, "active_size": end_offset},
                counts=next_counts,
            )
            return next_counts

    rebuilt = batch_unread_counts(group, actor_ids=actor_ids)
    out = {aid: max(0, int(rebuilt.get(aid, 0))) for aid in actor_ids}
    _save_unread_index(
        group,
        actors_rev=actors_rev,
        cursor_sig=cursor_sig,
        ledger_basis=ledger_basis,
        counts=out,
    )
    return out


def _load_cursors_raw(group: Group) -> Dict[str, Any]:
    p = _cursor_path(group)
    if not p.exists():
        return {}
    doc = json.loads(p.read_text(encoding="utf-8"))
    if not isinstance(doc, dict):
        raise ValueError(f"read cursor document must be an object: {p}")
    if doc.get("schema") != _MAIL_CURSOR_SCHEMA:
        # Pre-Mail cursor documents tracked direct delivery as well as Inbox
        # consumption. They are not valid boundaries for the Mail projection.
        return {}
    cursors = doc.get("cursors")
    if not isinstance(cursors, dict):
        raise ValueError(f"Mail cursor document is missing cursors: {p}")
    return cursors


def _load_pending_read(group: Group) -> Dict[str, Any]:
    path = _pending_read_path(group)
    if not path.exists():
        return {}
    doc = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(doc, dict) or doc.get("schema") != 1:
        raise ValueError(f"pending Mail read document is invalid: {path}")
    if str(doc.get("group_id") or "").strip() != group.group_id:
        raise ValueError(f"pending Mail read belongs to another group: {path}")
    actor_id = str(doc.get("actor_id") or "").strip()
    expected = doc.get("expected")
    target = doc.get("target")
    if not actor_id or not isinstance(expected, dict) or not isinstance(target, dict):
        raise ValueError(f"pending Mail read document is incomplete: {path}")
    if not str(target.get("event_id") or "").strip():
        raise ValueError(f"pending Mail read target is missing event_id: {path}")
    return doc


def _pending_read_has_fact(group: Group, pending: Dict[str, Any]) -> bool:
    actor_id = str(pending.get("actor_id") or "").strip()
    target = pending.get("target") if isinstance(pending.get("target"), dict) else {}
    target_event_id = str(target.get("event_id") or "").strip()
    for event in iter_events_reverse(group.ledger_path):
        if str(event.get("kind") or "") != "mail.read":
            continue
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if (
            str(data.get("actor_id") or "").strip() == actor_id
            and str(data.get("event_id") or "").strip() == target_event_id
        ):
            return True
    return False


def _clear_pending_read(group: Group) -> None:
    try:
        _pending_read_path(group).unlink()
    except FileNotFoundError:
        return


def _recover_pending_read_locked(group: Group) -> None:
    pending = _load_pending_read(group)
    if not pending:
        return
    if not _pending_read_has_fact(group, pending):
        _clear_pending_read(group)
        return

    actor_id = str(pending.get("actor_id") or "").strip()
    expected = pending.get("expected") if isinstance(pending.get("expected"), dict) else {}
    target = pending.get("target") if isinstance(pending.get("target"), dict) else {}
    cursors = _load_cursors_raw(group)
    current = cursors.get(actor_id) if isinstance(cursors.get(actor_id), dict) else {}
    current_event_id = str(current.get("event_id") or "").strip()
    current_ts = str(current.get("ts") or "")
    target_event_id = str(target.get("event_id") or "").strip()
    if current_event_id == target_event_id or (
        current_event_id
        and _cursor_record_covers_event(
            current,
            {"id": target_event_id, "ts": str(target.get("ts") or "")},
            positions=_ledger_positions(group, [current_event_id, target_event_id]),
        )
    ):
        _clear_pending_read(group)
        return
    if current_event_id != str(expected.get("event_id") or "").strip() or current_ts != str(
        expected.get("ts") or ""
    ):
        raise RuntimeError("pending Mail read cursor changed concurrently")
    cursors[actor_id] = dict(target)
    _save_cursors(group, cursors)
    _clear_pending_read(group)


def recover_pending_read(group: Group) -> None:
    """Finish or discard an interrupted Mail read transaction."""
    lock = acquire_lockfile(_cursor_lock_path(group), blocking=True)
    try:
        _recover_pending_read_locked(group)
    finally:
        release_lockfile(lock)


def load_cursors(group: Group) -> Dict[str, Any]:
    """Load effective Mail cursors, including a ledger-committed pending read."""
    cursors = _load_cursors_raw(group)
    pending = _load_pending_read(group)
    if pending and _pending_read_has_fact(group, pending):
        actor_id = str(pending.get("actor_id") or "").strip()
        target = pending.get("target") if isinstance(pending.get("target"), dict) else {}
        current = cursors.get(actor_id) if isinstance(cursors.get(actor_id), dict) else {}
        current_event_id = str(current.get("event_id") or "").strip()
        target_event_id = str(target.get("event_id") or "").strip()
        if not current_event_id or not _cursor_record_covers_event(
            current,
            {"id": target_event_id, "ts": str(target.get("ts") or "")},
            positions=_ledger_positions(group, [current_event_id, target_event_id]),
        ):
            cursors[actor_id] = dict(target)
    return cursors


def _save_cursors(group: Group, doc: Dict[str, Any]) -> None:
    p = _cursor_path(group)
    p.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_json(p, {"schema": _MAIL_CURSOR_SCHEMA, "cursors": doc})


def get_cursor_details(group: Group, actor_id: str) -> Tuple[str, str, str]:
    """Get an actor's Mail cursor: (event_id, ts, updated_at)."""
    cursors = load_cursors(group)
    cur = cursors.get(actor_id)
    if isinstance(cur, dict):
        event_id = str(cur.get("event_id") or "")
        ts = str(cur.get("ts") or "")
        updated_at = str(cur.get("updated_at") or "")
        return event_id, ts, updated_at
    return "", "", ""


def get_cursor(group: Group, actor_id: str) -> Tuple[str, str]:
    """Get an actor's Mail cursor: (event_id, ts)."""
    event_id, ts, _updated_at = get_cursor_details(group, actor_id)
    return event_id, ts


def _cursor_boundaries(
    group: Group,
    actor_ids: List[str],
    *,
    cursors: Optional[Dict[str, Any]] = None,
) -> Dict[str, Tuple[str, Optional[Any]]]:
    cursor_doc = cursors if isinstance(cursors, dict) else load_cursors(group)
    cursor_ids: Dict[str, str] = {}
    for actor_id in actor_ids:
        cursor = cursor_doc.get(actor_id)
        cursor_ids[actor_id] = str(cursor.get("event_id") or "").strip() if isinstance(cursor, dict) else ""

    wanted_ids = list(dict.fromkeys(event_id for event_id in cursor_ids.values() if event_id))
    found_ids: set[str] = set()
    if wanted_ids:
        try:
            found_ids = {
                event_id
                for event_id, position in zip(
                    wanted_ids,
                    lookup_event_positions(group.ledger_path, wanted_ids),
                )
                if position is not None
            }
        except Exception:
            found_ids = set()

    out: Dict[str, Tuple[str, Optional[Any]]] = {}
    for actor_id in actor_ids:
        cursor = cursor_doc.get(actor_id)
        cursor_ts = str(cursor.get("ts") or "") if isinstance(cursor, dict) else ""
        cursor_id = cursor_ids.get(actor_id, "")
        out[actor_id] = (
            cursor_id if cursor_id in found_ids else "",
            parse_utc_iso(cursor_ts) if cursor_ts else None,
        )
    return out


def _ledger_positions(group: Group, event_ids: Iterable[str]) -> Dict[str, Tuple[int, int]]:
    wanted_ids = list(
        dict.fromkeys(
            str(event_id or "").strip()
            for event_id in event_ids
            if str(event_id or "").strip()
        )
    )
    if not wanted_ids:
        return {}
    try:
        positions = lookup_event_positions(group.ledger_path, wanted_ids)
    except Exception:
        return {}
    return {
        event_id: position
        for event_id, position in zip(wanted_ids, positions)
        if position is not None
    }


def actor_generation_positions(
    group: Group,
    actor_ids: Iterable[str],
) -> Dict[str, Tuple[int, int]]:
    normalized = list(
        dict.fromkeys(
            str(actor_id or "").strip()
            for actor_id in actor_ids
            if str(actor_id or "").strip()
        )
    )
    if not normalized:
        return {}
    try:
        return lookup_latest_actor_add_positions(group.ledger_path, normalized)
    except Exception:
        return {}


def actor_existed_at_event(
    group: Group,
    *,
    actor: Dict[str, Any],
    event: Dict[str, Any],
    positions: Optional[Dict[str, Tuple[int, int]]] = None,
    generations: Optional[Dict[str, Tuple[int, int]]] = None,
) -> bool:
    actor_id = str(actor.get("id") or "").strip()
    event_id = str(event.get("id") or "").strip()
    effective_positions = positions if positions is not None else _ledger_positions(group, [event_id])
    effective_generations = (
        generations
        if generations is not None
        else actor_generation_positions(group, [actor_id])
    )
    event_position = effective_positions.get(event_id)
    generation_position = effective_generations.get(actor_id)
    if event_position is not None and generation_position is not None:
        return event_position >= generation_position

    created_ts = str(actor.get("created_at") or "").strip()
    event_ts = str(event.get("ts") or "").strip()
    created_dt = parse_utc_iso(created_ts) if created_ts else None
    event_dt = parse_utc_iso(event_ts) if event_ts else None
    return not (created_dt is not None and event_dt is not None and created_dt > event_dt)


def _cursor_record_covers_event(
    cursor: Any,
    event: Dict[str, Any],
    *,
    positions: Dict[str, Tuple[int, int]],
) -> bool:
    cursor_event_id = str(cursor.get("event_id") or "").strip() if isinstance(cursor, dict) else ""
    event_id = str(event.get("id") or "").strip()
    if cursor_event_id and cursor_event_id == event_id:
        return True
    cursor_position = positions.get(cursor_event_id)
    event_position = positions.get(event_id)
    if cursor_position is not None and event_position is not None:
        return cursor_position >= event_position

    cursor_ts = str(cursor.get("ts") or "") if isinstance(cursor, dict) else ""
    cursor_dt = parse_utc_iso(cursor_ts) if cursor_ts else None
    event_dt = parse_utc_iso(str(event.get("ts") or ""))
    return bool(cursor_dt is not None and event_dt is not None and event_dt <= cursor_dt)


def _event_after_cursor_boundary(
    event: Dict[str, Any],
    *,
    cursor_event_id: str,
    cursor_dt: Optional[Any],
    cursor_seen: bool,
) -> Tuple[bool, bool]:
    if cursor_event_id:
        if cursor_seen:
            return True, True
        if str(event.get("id") or "").strip() == cursor_event_id:
            return False, True
        return False, False
    if cursor_dt is not None:
        event_dt = parse_utc_iso(str(event.get("ts") or ""))
        if event_dt is not None and event_dt <= cursor_dt:
            return False, True
    return True, True


def commit_read_cursor(
    group: Group,
    actor_id: str,
    *,
    expected_event_id: str,
    expected_ts: str,
    event_id: str,
    ts: str,
    append_read_event: Callable[[], Dict[str, Any]],
) -> Tuple[Dict[str, Any], Dict[str, Any]]:
    """Atomically claim one consuming read against the current cursor.

    The cursor compare-and-set prevents concurrent readers from returning the
    same unread prefix. A small recovery marker makes the ledger append the
    authoritative commit: a crash after ``mail.read`` but before the cursor
    projection is saved is completed on the next read instead of replaying or
    silently skipping Mail.
    """

    lock = acquire_lockfile(_cursor_lock_path(group), blocking=True)
    try:
        _recover_pending_read_locked(group)
        cursors = _load_cursors_raw(group)
        current = cursors.get(actor_id)
        current_event_id = (
            str(current.get("event_id") or "").strip()
            if isinstance(current, dict)
            else ""
        )
        current_ts = (
            str(current.get("ts") or "") if isinstance(current, dict) else ""
        )
        if current_event_id != str(expected_event_id or "").strip() or current_ts != str(
            expected_ts or ""
        ):
            raise RuntimeError("read cursor changed concurrently")

        target_event_id = str(event_id or "").strip()
        if not target_event_id:
            raise ValueError("read cursor target event_id is required")
        if current_event_id and _cursor_record_covers_event(
            current,
            {"id": target_event_id, "ts": str(ts or "")},
            positions=_ledger_positions(group, [current_event_id, target_event_id]),
        ):
            raise RuntimeError("read cursor changed concurrently")
        # This consuming operation returns the complete unread Mail prefix
        # through target_event_id. Non-Mail ledger events are intentionally
        # outside this projection and do not participate in its cursor.

        next_cursor = {
            "event_id": target_event_id,
            "ts": str(ts or ""),
            "updated_at": utc_now_iso(),
        }
        pending = {
            "schema": 1,
            "group_id": group.group_id,
            "actor_id": actor_id,
            "expected": {
                "event_id": current_event_id,
                "ts": current_ts,
            },
            "target": next_cursor,
        }
        atomic_write_json(_pending_read_path(group), pending)
        try:
            read_event = append_read_event()
        except Exception as append_error:
            try:
                _clear_pending_read(group)
            except Exception as cleanup_error:
                raise RuntimeError(
                    f"{append_error}; pending Mail read cleanup failed: {cleanup_error}"
                ) from append_error
            raise
        cursors[actor_id] = next_cursor
        _save_cursors(group, cursors)
        try:
            _clear_pending_read(group)
        except OSError:
            # Both durable facts are committed; a retained marker is
            # idempotently cleared by the next consuming read.
            pass
        return next_cursor, read_event
    finally:
        release_lockfile(lock)


def delete_cursor(group: Group, actor_id: str) -> bool:
    """Delete an actor's Mail cursor entry (used when an actor is removed)."""
    aid = str(actor_id or "").strip()
    if not aid:
        return False
    lock = acquire_lockfile(_cursor_lock_path(group), blocking=True)
    try:
        _recover_pending_read_locked(group)
        cursors = _load_cursors_raw(group)
        if aid not in cursors:
            return False
        cursors.pop(aid, None)
        _save_cursors(group, cursors)
        return True
    finally:
        release_lockfile(lock)


def _message_outcomes(
    group: Group,
    *,
    event_ids: set[str],
) -> tuple[
    Dict[str, Dict[str, int]],
    Dict[str, int],
    Dict[str, Dict[str, str]],
]:
    replies: Dict[str, Dict[str, int]] = {}
    cancellations: Dict[str, int] = {}
    deliveries: Dict[str, Dict[str, str]] = {}
    if not event_ids:
        return replies, cancellations, deliveries
    for position, event in enumerate(iter_events(group.ledger_path)):
        kind = str(event.get("kind") or "")
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if kind == "chat.message":
            source_event_id = str(data.get("reply_to") or "").strip()
            actor_id = str(event.get("by") or "").strip()
            if source_event_id in event_ids and actor_id:
                replies.setdefault(source_event_id, {}).setdefault(actor_id, position)
        elif kind == "chat.reply_request.cancelled":
            source_event_id = str(data.get("source_event_id") or "").strip()
            if source_event_id in event_ids:
                cancellations.setdefault(source_event_id, position)
        elif kind == "runtime.delivery":
            source_event_id = str(data.get("source_event_id") or "").strip()
            actor_id = str(data.get("actor_id") or "").strip()
            state = str(data.get("state") or "").strip()
            if source_event_id in event_ids and actor_id and state:
                deliveries.setdefault(source_event_id, {})[actor_id] = state
    return replies, cancellations, deliveries


def get_obligation_status_batch(
    group: Group,
    events: List[Dict[str, Any]],
) -> Dict[str, Dict[str, Dict[str, Any]]]:
    """Compute per-recipient obligation status for chat messages.

    Returns:
      {
        "<message_event_id>": {
          "<recipient_id>": {
            "replied": bool,
            "reply_requested": bool,
            "cancelled": bool,
            "delivery_state": str,
          },
          ...
        },
        ...
      }

    Notes:
    - Includes only local-group chat.message events (dst_group_id empty).
    - Recipients are resolved from current roster with actor-generation ledger checks.
    - "user" is included only when explicitly targeted.
    """
    actors = list_actors(group)

    target_ids: set[str] = set()
    for ev in events:
        if str(ev.get("kind") or "") != "chat.message":
            continue
        data = ev.get("data")
        if not isinstance(data, dict):
            continue
        if str(data.get("dst_group_id") or "").strip():
            continue
        event_id = str(ev.get("id") or "").strip()
        if event_id:
            target_ids.add(event_id)

    replies, cancellations, deliveries = _message_outcomes(group, event_ids=target_ids)
    positions = _ledger_positions(group, target_ids)
    actor_ids = [
        str(actor.get("id") or "").strip()
        for actor in actors
        if isinstance(actor, dict) and str(actor.get("id") or "").strip()
    ]
    generations = actor_generation_positions(group, actor_ids)

    result: Dict[str, Dict[str, Dict[str, Any]]] = {}

    for ev in events:
        if str(ev.get("kind") or "") != "chat.message":
            continue

        data = ev.get("data")
        if not isinstance(data, dict):
            continue
        if str(data.get("dst_group_id") or "").strip():
            continue

        event_id = str(ev.get("id") or "").strip()
        if not event_id:
            continue

        by = str(ev.get("by") or "").strip()
        reply_requested = str(data.get("message_mode") or "") == "request_reply"

        to_raw = data.get("to")
        to_tokens = [str(x).strip() for x in to_raw] if isinstance(to_raw, list) else []
        to_set = {t for t in to_tokens if t}

        recipients: List[str] = []
        for actor in actors:
            if not isinstance(actor, dict):
                continue
            aid = str(actor.get("id") or "").strip()
            if not aid or aid == "user" or aid == by:
                continue
            if not actor_existed_at_event(
                group,
                actor=actor,
                event=ev,
                positions=positions,
                generations=generations,
            ):
                continue
            if not is_message_for_actor(group, actor_id=aid, event=ev):
                continue
            recipients.append(aid)

        if by != "user" and ("user" in to_set or "@user" in to_set):
            recipients.append("user")

        reply_positions = replies.get(event_id, {})
        cancellation_position = cancellations.get(event_id)
        delivery_states = deliveries.get(event_id, {})

        status: Dict[str, Dict[str, Any]] = {}
        for rid in recipients:
            reply_position = reply_positions.get(rid)
            cancelled = bool(
                reply_requested
                and cancellation_position is not None
                and (reply_position is None or cancellation_position < reply_position)
            )
            replied = bool(reply_position is not None and not cancelled)

            status[rid] = {
                "replied": replied,
                "reply_requested": reply_requested,
                "cancelled": cancelled,
                "delivery_state": delivery_states.get(rid, ""),
            }

        result[event_id] = status

    return result


def _message_targets(event: Dict[str, Any]) -> List[str]:
    """Get the 'to' targets for a chat message event."""
    data = event.get("data")
    if not isinstance(data, dict):
        return []
    to = data.get("to")
    if isinstance(to, list):
        return [str(x) for x in to if isinstance(x, str) and x.strip()]
    return []


def _actor_role(group: Group, actor_id: str) -> str:
    """Get the actor's effective role (derived from position)."""
    return get_effective_role(group, actor_id)


def is_message_for_actor(
    group: Group,
    *,
    actor_id: str,
    event: Dict[str, Any],
    role: Optional[str] = None,
) -> bool:
    """Return True if the event should be visible/delivered to the given actor.

    Args:
        group: Working group
        actor_id: Actor id
        event: Event dict
        role: Pre-computed actor role (optimization to avoid repeated lookups).
              If None, will be computed via get_effective_role().
    """
    kind = str(event.get("kind") or "")
    actor = find_actor(group, actor_id)
    actor_internal = isinstance(actor, dict) and is_internal_actor(actor)

    # system.notify: check target_actor_id
    if kind == "system.notify":
        data = event.get("data")
        if not isinstance(data, dict):
            return False
        if str(data.get("kind") or "") in {"mail_notice", "reply_notice"}:
            return False
        target = str(data.get("target_actor_id") or "").strip()
        if actor_internal:
            return bool(target) and target == actor_id
        # Empty target = broadcast to everyone
        if not target:
            return True
        return target == actor_id

    # chat.message: check the "to" field
    targets = _message_targets(event)

    if actor_internal:
        return actor_id in targets

    # Empty targets = broadcast (everyone can see)
    if not targets:
        return True

    # @all = all actors
    if "@all" in targets:
        return True

    # Direct actor_id mention
    if actor_id in targets:
        return True

    # Role-based matching (use pre-computed role if provided)
    if role is None:
        role = _actor_role(group, actor_id)
    if role == "peer" and "@peers" in targets:
        return True
    if role == "foreman" and "@foreman" in targets:
        return True

    return False


def _is_mail_message(event: Dict[str, Any]) -> bool:
    if str(event.get("kind") or "") != "chat.message":
        return False
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    return str(data.get("message_mode") or "") == "mail"


def unread_messages(group: Group, *, actor_id: str, limit: int = 50) -> List[Dict[str, Any]]:
    """Return the actor's unread Mail projection in ledger append order."""
    cursor_event_id, cursor_dt = _cursor_boundaries(group, [actor_id])[actor_id]
    cursor_seen = not bool(cursor_event_id)
    generation_event_id = _latest_actor_add_event_ids(group, [actor_id]).get(actor_id, "")
    generation_seen = not bool(generation_event_id)

    out: List[Dict[str, Any]] = []
    for ev in iter_events(group.ledger_path):
        event_id = str(ev.get("id") or "").strip()
        if event_id == generation_event_id:
            generation_seen = True
        if (
            not generation_seen
            or (not _is_mail_message(ev) and event_id != cursor_event_id)
        ):
            continue
        after_cursor, cursor_seen = _event_after_cursor_boundary(
            ev,
            cursor_event_id=cursor_event_id,
            cursor_dt=cursor_dt,
            cursor_seen=cursor_seen,
        )
        if not after_cursor or not _is_mail_message(ev):
            continue
        if str(ev.get("by") or "") == actor_id:
            continue
        if not is_message_for_actor(group, actor_id=actor_id, event=ev):
            continue
        out.append(ev)
        if limit > 0 and len(out) >= limit:
            break
    return out


def unread_count(group: Group, *, actor_id: str) -> int:
    """Count unread Mail for an actor."""
    cursor_event_id, cursor_dt = _cursor_boundaries(group, [actor_id])[actor_id]
    cursor_seen = not bool(cursor_event_id)
    generation_event_id = _latest_actor_add_event_ids(group, [actor_id]).get(actor_id, "")
    generation_seen = not bool(generation_event_id)

    count = 0
    for ev in iter_events(group.ledger_path):
        event_id = str(ev.get("id") or "").strip()
        if event_id == generation_event_id:
            generation_seen = True
        if (
            not generation_seen
            or (not _is_mail_message(ev) and event_id != cursor_event_id)
        ):
            continue
        after_cursor, cursor_seen = _event_after_cursor_boundary(
            ev,
            cursor_event_id=cursor_event_id,
            cursor_dt=cursor_dt,
            cursor_seen=cursor_seen,
        )
        if not after_cursor or not _is_mail_message(ev):
            continue
        if str(ev.get("by") or "") == actor_id:
            continue
        if not is_message_for_actor(group, actor_id=actor_id, event=ev):
            continue
        count += 1
    return count


def mail_pending_summary(group: Group, *, actor_id: str) -> Dict[str, Any]:
    """Return every unread Mail item without mutating the Mail cursor.

    Reply and manual-delivery facts suppress the one-shot active notice, but
    they do not consume Mail.  Natural hints therefore mirror the Inbox
    projection exactly instead of exposing the smaller notice-eligible set.
    """

    pending = unread_messages(group, actor_id=actor_id, limit=0)
    if not pending:
        return {}
    oldest = parse_utc_iso(str(pending[0].get("ts") or ""))
    now = parse_utc_iso(utc_now_iso())
    oldest_age_seconds = 0
    if oldest is not None and now is not None:
        oldest_age_seconds = max(0, int((now - oldest).total_seconds()))
    return {
        "count": len(pending),
        "oldest_age_seconds": oldest_age_seconds,
        "action": "cccc_inbox_read()",
    }


def batch_unread_counts(
    group: Group,
    *,
    actor_ids: List[str],
) -> Dict[str, int]:
    """Count unread Mail for multiple actors in a single ledger pass.

    This remains O(n * m) where n = actors and m = events, but it avoids
    re-reading/parsing the ledger for each actor and loads cursors once.

    Args:
        group: Working group
        actor_ids: List of actor ids to count for
    Returns:
        Dict mapping actor_id -> unread count
    """
    if not actor_ids:
        return {}

    # Load all cursors at once
    cursors = load_cursors(group)
    cursor_boundaries = _cursor_boundaries(group, actor_ids, cursors=cursors)
    cursor_seen = {aid: not bool(cursor_boundaries[aid][0]) for aid in actor_ids}
    cursor_anchor_ids = {boundary[0] for boundary in cursor_boundaries.values() if boundary[0]}
    generation_event_ids = _latest_actor_add_event_ids(group, actor_ids)
    generation_anchor_ids = set(generation_event_ids.values())
    generation_seen = {aid: aid not in generation_event_ids for aid in actor_ids}

    # Initialize counts
    counts: Dict[str, int] = {aid: 0 for aid in actor_ids}

    # Pre-compute actor roles once (optimization: avoids repeated get_effective_role calls)
    actor_roles: Dict[str, str] = {aid: get_effective_role(group, aid) for aid in actor_ids}

    # Single pass through the ledger
    for ev in iter_events(group.ledger_path):
        event_id = str(ev.get("id") or "").strip()
        if (
            not _is_mail_message(ev)
            and event_id not in cursor_anchor_ids
            and event_id not in generation_anchor_ids
        ):
            continue

        ev_by = str(ev.get("by") or "")

        # Check each actor
        for aid in actor_ids:
            if event_id == generation_event_ids.get(aid, ""):
                generation_seen[aid] = True
            if not generation_seen[aid]:
                continue
            cursor_event_id, cursor_dt = cursor_boundaries[aid]
            after_cursor, cursor_seen[aid] = _event_after_cursor_boundary(
                ev,
                cursor_event_id=cursor_event_id,
                cursor_dt=cursor_dt,
                cursor_seen=cursor_seen[aid],
            )
            if not after_cursor or not _is_mail_message(ev):
                continue
            if ev_by == aid:
                continue
            # Check delivery/visibility rules (pass pre-computed role)
            if not is_message_for_actor(group, actor_id=aid, event=ev, role=actor_roles[aid]):
                continue
            counts[aid] += 1

    return counts


def resolve_event_id(group: Group, event_id: str) -> str:
    """Resolve an event id from an exact id or a unique id prefix."""
    wanted = str(event_id or "").strip()
    if not wanted:
        return ""

    event = lookup_event_by_id(group.ledger_path, wanted)
    if event is not None:
        return str(event.get("id") or "").strip()
    if _is_full_event_id(wanted):
        return wanted

    exact_match = ""
    prefix_match = ""
    prefix_ambiguous = False
    for ev in iter_events_reverse(group.ledger_path):
        candidate = str(ev.get("id") or "").strip()
        if not candidate:
            continue
        if candidate == wanted:
            exact_match = candidate
            break
        if candidate.startswith(wanted):
            if not prefix_match:
                prefix_match = candidate
            elif prefix_match != candidate:
                prefix_ambiguous = True
                break
    if exact_match:
        return exact_match
    if prefix_match and not prefix_ambiguous:
        return prefix_match
    return ""


def find_event(group: Group, event_id: str) -> Optional[Dict[str, Any]]:
    """Find an event by exact id or a unique id prefix."""
    resolved = resolve_event_id(group, event_id)
    if not resolved:
        return None
    event = lookup_event_by_id(group.ledger_path, resolved)
    if event is not None:
        return event
    for ev in iter_events_reverse(group.ledger_path):
        if str(ev.get("id") or "").strip() == resolved:
            return ev
    return None


def get_quote_text(group: Group, event_id: str, max_len: int = 100) -> Optional[str]:
    """Get a short quoted snippet for reply_to rendering."""
    ev = find_event(group, event_id)
    if ev is None:
        return None
    data = ev.get("data")
    if not isinstance(data, dict):
        return None
    text = data.get("text")
    if not isinstance(text, str):
        return None
    text = text.strip()
    if len(text) > max_len:
        return text[:max_len] + "..."
    return text


def get_read_status(group: Group, event_id: str) -> Dict[str, bool]:
    """Get per-actor read status for one Mail event."""
    ev = find_event(group, event_id)
    if ev is None:
        return {}

    if not _is_mail_message(ev):
        return {}

    return get_read_status_batch(group, [ev]).get(str(ev.get("id") or ""), {})


def get_read_status_batch(
    group: Group,
    events: List[Dict[str, Any]],
) -> Dict[str, Dict[str, bool]]:
    """Batch compute per-actor Mail read status.

    This is an optimized version of get_read_status() that loads cursors and
    actors only once, avoiding N+1 queries.

    Args:
        group: Working group
        events: List of events (only Mail messages will be processed)

    Returns:
        Dict mapping event_id -> {actor_id: bool}
    """
    # Load shared data once
    cursors = load_cursors(group)
    actors = list_actors(group)
    event_ids = [
        str(event.get("id") or "").strip()
        for event in events
        if str(event.get("id") or "").strip()
    ]
    cursor_event_ids = [
        str(cursor.get("event_id") or "").strip()
        for cursor in cursors.values()
        if isinstance(cursor, dict) and str(cursor.get("event_id") or "").strip()
    ]
    positions = _ledger_positions(group, [*event_ids, *cursor_event_ids])
    actor_ids = [
        str(actor.get("id") or "").strip()
        for actor in actors
        if isinstance(actor, dict) and str(actor.get("id") or "").strip()
    ]
    generations = actor_generation_positions(group, actor_ids)

    result: Dict[str, Dict[str, bool]] = {}

    for ev in events:
        if not _is_mail_message(ev):
            continue

        event_id = str(ev.get("id") or "")
        if not event_id:
            continue

        by = str(ev.get("by") or "").strip()
        status: Dict[str, bool] = {}

        for actor in actors:
            if not isinstance(actor, dict):
                continue
            actor_id = str(actor.get("id") or "").strip()
            if not actor_id or actor_id == "user" or actor_id == by:
                continue
            if not actor_existed_at_event(
                group,
                actor=actor,
                event=ev,
                positions=positions,
                generations=generations,
            ):
                continue
            if not is_message_for_actor(group, actor_id=actor_id, event=ev):
                continue

            cur = cursors.get(actor_id)
            status[actor_id] = _cursor_record_covers_event(cur, ev, positions=positions)

        result[event_id] = status

    return result


def search_messages(
    group: Group,
    *,
    query: str = "",
    kind_filter: MessageKindFilter = "all",
    by_filter: str = "",
    before_id: str = "",
    after_id: str = "",
    limit: int = 50,
) -> Tuple[List[Dict[str, Any]], bool]:
    """Search and paginate messages in the ledger.
    
    Args:
        group: Working group
        query: Text search query (case-insensitive substring match)
        kind_filter: Filter by message type (all/chat/notify)
        by_filter: Filter by sender (actor_id or "user")
        before_id: Return messages before this event_id (for backward pagination)
        after_id: Return messages after this event_id (for forward pagination)
        limit: Maximum number of messages to return
    
    Returns:
        Tuple of (messages, has_more)
    """
    # Determine allowed kinds
    if kind_filter == "chat":
        allowed_kinds = {"chat.message"}
    elif kind_filter == "notify":
        allowed_kinds = {"system.notify"}
    else:
        allowed_kinds = {"chat.message", "system.notify"}
    
    query_lower = query.lower().strip() if query else ""
    by_filter = by_filter.strip()
    
    if not query_lower:
        event_ids, has_more = search_event_ids_indexed(
            group.ledger_path,
            allowed_kinds=allowed_kinds,
            query="",
            by_filter=by_filter,
            before_id=before_id,
            after_id=after_id,
            limit=limit,
        )
        if event_ids:
            events: List[Dict[str, Any]] = []
            for ev in lookup_events_by_ids(group.ledger_path, event_ids):
                if isinstance(ev, dict):
                    events.append(ev)
            if not before_id and not after_id:
                events.reverse()
            return events, has_more
        if before_id or after_id:
            return [], False

    event_ids, has_more = search_event_ids_indexed(
        group.ledger_path,
        allowed_kinds=allowed_kinds,
        query=query_lower,
        by_filter=by_filter,
        before_id=before_id,
        after_id=after_id,
        limit=limit,
    )
    if event_ids:
        events = []
        for ev in lookup_events_by_ids(group.ledger_path, event_ids):
            if isinstance(ev, dict):
                data = ev.get("data")
                if isinstance(data, dict):
                    text = str(data.get("text") or "").lower()
                    insight = str(data.get("insight") or "").lower()
                    title = str(data.get("title") or "").lower()
                    message = str(data.get("message") or "").lower()
                    if (
                        query_lower not in text
                        and query_lower not in insight
                        and query_lower not in title
                        and query_lower not in message
                    ):
                        continue
                else:
                    continue
                events.append(ev)
        if events:
            if not before_id and not after_id:
                events.reverse()
            return events, has_more
        if before_id or after_id:
            return [], False

    # Fallback: collect all matching events
    all_events: List[Dict[str, Any]] = []
    for ev in iter_events(group.ledger_path):
        ev_kind = str(ev.get("kind") or "")
        if ev_kind not in allowed_kinds:
            continue
        
        # Filter by sender
        if by_filter:
            ev_by = str(ev.get("by") or "")
            if ev_by != by_filter:
                continue
        
        # Text search
        if query_lower:
            data = ev.get("data")
            if isinstance(data, dict):
                text = str(data.get("text") or "").lower()
                insight = str(data.get("insight") or "").lower()
                title = str(data.get("title") or "").lower()
                message = str(data.get("message") or "").lower()
                if (
                    query_lower not in text
                    and query_lower not in insight
                    and query_lower not in title
                    and query_lower not in message
                ):
                    continue
            else:
                continue
        
        all_events.append(ev)
    
    # Handle pagination
    if before_id:
        # Find the index of before_id and return events before it
        idx = -1
        for i, ev in enumerate(all_events):
            if str(ev.get("id") or "") == before_id:
                idx = i
                break
        if idx > 0:
            start = max(0, idx - limit)
            result = all_events[start:idx]
            has_more = start > 0
            return result, has_more
        return [], False
    
    if after_id:
        # Find the index of after_id and return events after it
        idx = -1
        for i, ev in enumerate(all_events):
            if str(ev.get("id") or "") == after_id:
                idx = i
                break
        if idx >= 0 and idx < len(all_events) - 1:
            start = idx + 1
            end = min(len(all_events), start + limit)
            result = all_events[start:end]
            has_more = end < len(all_events)
            return result, has_more
        return [], False
    
    # Default: return last N messages
    if len(all_events) > limit:
        result = all_events[-limit:]
        has_more = True
    else:
        result = all_events
        has_more = False
    
    return result, has_more
