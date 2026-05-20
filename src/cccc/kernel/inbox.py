from __future__ import annotations

import hashlib
import json
import os
import re
from pathlib import Path
from typing import Any, Dict, Iterable, List, Literal, Optional, Tuple

from .context import ContextStorage
from ..util.fs import atomic_write_json, read_json
from ..util.time import parse_utc_iso, utc_now_iso
from .actors import find_actor, get_effective_role, is_internal_actor, list_actors
from .group import Group
from .ledger_index import (
    lookup_event_by_id,
    lookup_event_with_chat_ack_indexed,
    lookup_events_by_ids,
    search_event_ids_indexed,
)
from .ledger_segments import iter_source_lines, list_ledger_sources
from .ledger_state_snapshot import can_replay_from_basis, current_ledger_basis, load_latest_ledger_snapshot


# Message kind filter
MessageKindFilter = Literal["all", "chat", "notify"]

_UNREAD_INDEX_SCHEMA = 1
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


def _cursor_path(group: Group) -> Path:
    return group.path / "state" / "read_cursors.json"


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
    return {
        "schema": int(raw.get("schema", _UNREAD_INDEX_SCHEMA) or _UNREAD_INDEX_SCHEMA),
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
    kind_filter: MessageKindFilter,
) -> Dict[str, int]:
    next_counts = {aid: max(0, int(counts.get(aid, 0))) for aid in counts}
    actor_ids = [str(actor.get("id") or "").strip() for actor in actors if str(actor.get("id") or "").strip()]
    actor_roles = {aid: get_effective_role(group, aid) for aid in actor_ids}
    cursors = load_cursors(group)
    actor_cursor_dts: Dict[str, Optional[Any]] = {}
    for aid in actor_ids:
        cur = cursors.get(aid)
        cursor_ts = str(cur.get("ts") or "") if isinstance(cur, dict) else ""
        actor_cursor_dts[aid] = parse_utc_iso(cursor_ts) if cursor_ts else None

    if kind_filter == "chat":
        allowed_kinds = {"chat.message"}
    elif kind_filter == "notify":
        allowed_kinds = {"system.notify"}
    else:
        allowed_kinds = {"chat.message", "system.notify"}

    for ev in events:
        ev_kind = str(ev.get("kind") or "")
        if ev_kind not in allowed_kinds:
            continue
        ev_by = str(ev.get("by") or "").strip()
        ev_ts = str(ev.get("ts") or "")
        ev_dt = parse_utc_iso(ev_ts) if ev_ts else None
        for aid in actor_ids:
            if ev_kind == "chat.message" and ev_by == aid:
                continue
            cursor_dt = actor_cursor_dts.get(aid)
            if cursor_dt is not None and ev_dt is not None and ev_dt <= cursor_dt:
                continue
            if not is_message_for_actor(group, actor_id=aid, event=ev, role=actor_roles.get(aid)):
                continue
            next_counts[aid] = max(0, int(next_counts.get(aid, 0)) + 1)
    return next_counts


def get_indexed_unread_counts(
    group: Group,
    *,
    actors: List[Dict[str, Any]],
    kind_filter: MessageKindFilter = "all",
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
        int(snapshot.get("actors_rev") or 0) == actors_rev
        and str(snapshot.get("cursor_sig") or "") == cursor_sig
        and snapshot_basis == ledger_basis
    ):
        return {aid: max(0, int(snapshot_counts.get(aid, 0))) for aid in actor_ids}

    if (
        int(snapshot.get("actors_rev") or 0) == actors_rev
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
                kind_filter=kind_filter,
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
                kind_filter=kind_filter,
            )
            _save_unread_index(
                group,
                actors_rev=actors_rev,
                cursor_sig=cursor_sig,
                ledger_basis={**ledger_basis, "active_size": end_offset},
                counts=next_counts,
            )
            return next_counts

    rebuilt = batch_unread_counts(group, actor_ids=actor_ids, kind_filter=kind_filter)
    out = {aid: max(0, int(rebuilt.get(aid, 0))) for aid in actor_ids}
    _save_unread_index(
        group,
        actors_rev=actors_rev,
        cursor_sig=cursor_sig,
        ledger_basis=ledger_basis,
        counts=out,
    )
    return out


def load_cursors(group: Group) -> Dict[str, Any]:
    """Load read cursors for all actors."""
    p = _cursor_path(group)
    doc = read_json(p)
    return doc if isinstance(doc, dict) else {}


def _save_cursors(group: Group, doc: Dict[str, Any]) -> None:
    p = _cursor_path(group)
    p.parent.mkdir(parents=True, exist_ok=True)
    atomic_write_json(p, doc)


def get_cursor(group: Group, actor_id: str) -> Tuple[str, str]:
    """Get an actor's read cursor: (event_id, ts)."""
    cursors = load_cursors(group)
    cur = cursors.get(actor_id)
    if isinstance(cur, dict):
        event_id = str(cur.get("event_id") or "")
        ts = str(cur.get("ts") or "")
        return event_id, ts
    return "", ""


def set_cursor(group: Group, actor_id: str, *, event_id: str, ts: str) -> Dict[str, Any]:
    """Set an actor's read cursor (monotonic forward-only)."""
    cursors = load_cursors(group)
    cur = cursors.get(actor_id)

    # Ensure the cursor moves forward (never backwards).
    if isinstance(cur, dict):
        cur_ts = str(cur.get("ts") or "")
        if cur_ts:
            cur_dt = parse_utc_iso(cur_ts)
            new_dt = parse_utc_iso(ts)
            if cur_dt is not None and new_dt is not None and new_dt < cur_dt:
                # Do not allow moving backwards.
                return dict(cur)

    cursors[str(actor_id)] = {
        "event_id": str(event_id),
        "ts": str(ts),
        "updated_at": utc_now_iso(),
    }
    _save_cursors(group, cursors)
    return dict(cursors[str(actor_id)])


def delete_cursor(group: Group, actor_id: str) -> bool:
    """Delete an actor's read cursor entry (used when an actor is removed)."""
    aid = str(actor_id or "").strip()
    if not aid:
        return False
    cursors = load_cursors(group)
    if aid not in cursors:
        return False
    cursors.pop(aid, None)
    _save_cursors(group, cursors)
    return True


def _collect_chat_acks(group: Group, *, event_ids: set[str]) -> Dict[str, set[str]]:
    """Collect acked recipients for a set of message event IDs.

    Ledger is the source of truth: we derive ack status by scanning chat.ack events.
    """
    out: Dict[str, set[str]] = {}
    if not event_ids:
        return out

    for ev in iter_events(group.ledger_path):
        if str(ev.get("kind") or "") != "chat.ack":
            continue
        data = ev.get("data")
        if not isinstance(data, dict):
            continue
        target_event_id = str(data.get("event_id") or "").strip()
        if not target_event_id or target_event_id not in event_ids:
            continue
        actor_id = str(data.get("actor_id") or "").strip()
        if not actor_id:
            continue
        out.setdefault(target_event_id, set()).add(actor_id)

    return out


def _collect_chat_replies(group: Group, *, event_ids: set[str]) -> Dict[str, set[str]]:
    """Collect replied recipients for a set of message event IDs.

    A recipient is considered replied when they send a chat.message with
    data.reply_to == target_event_id.
    """
    out: Dict[str, set[str]] = {}
    if not event_ids:
        return out

    for ev in iter_events(group.ledger_path):
        if str(ev.get("kind") or "") != "chat.message":
            continue
        data = ev.get("data")
        if not isinstance(data, dict):
            continue
        target_event_id = str(data.get("reply_to") or "").strip()
        if not target_event_id or target_event_id not in event_ids:
            continue
        actor_id = str(ev.get("by") or "").strip()
        if not actor_id:
            continue
        out.setdefault(target_event_id, set()).add(actor_id)

    return out


def has_chat_ack(group: Group, *, event_id: str, actor_id: str) -> bool:
    """Return True if a chat.ack already exists for (event_id, actor_id)."""
    eid = str(event_id or "").strip()
    aid = str(actor_id or "").strip()
    if not eid or not aid:
        return False
    for ev in iter_events(group.ledger_path):
        if str(ev.get("kind") or "") != "chat.ack":
            continue
        data = ev.get("data")
        if not isinstance(data, dict):
            continue
        if str(data.get("event_id") or "").strip() != eid:
            continue
        if str(data.get("actor_id") or "").strip() != aid:
            continue
        return True
    return False


def get_ack_status_batch(group: Group, events: List[Dict[str, Any]]) -> Dict[str, Dict[str, bool]]:
    """Compute per-recipient ack status for attention chat messages.

    Returns:
      { "<message_event_id>": { "<recipient_id>": bool, ... }, ... }

    Notes:
    - Only includes chat.message events with data.priority == "attention".
    - Recipient expansion is based on the message "to" tokens and the actor roster,
      excluding the sender and actors created after the message timestamp.
    - "user" is included only if explicitly targeted (to includes "user" or "@user").
    """
    actors = list_actors(group)

    attention_ids: set[str] = set()
    for ev in events:
        if str(ev.get("kind") or "") != "chat.message":
            continue
        data = ev.get("data")
        if not isinstance(data, dict):
            continue
        if str(data.get("priority") or "normal").strip() != "attention":
            continue
        # Outbound cross-group records are not ACK-tracked in the source group.
        if str(data.get("dst_group_id") or "").strip():
            continue
        event_id = str(ev.get("id") or "").strip()
        if event_id:
            attention_ids.add(event_id)

    acked_by_message = _collect_chat_acks(group, event_ids=attention_ids)

    result: Dict[str, Dict[str, bool]] = {}

    for ev in events:
        if str(ev.get("kind") or "") != "chat.message":
            continue

        data = ev.get("data")
        if not isinstance(data, dict):
            continue

        priority = str(data.get("priority") or "normal").strip()
        if priority != "attention":
            continue
        # Outbound cross-group records are not ACK-tracked in the source group.
        if str(data.get("dst_group_id") or "").strip():
            continue

        event_id = str(ev.get("id") or "").strip()
        if not event_id:
            continue

        ev_ts = str(ev.get("ts") or "").strip()
        ev_dt = parse_utc_iso(ev_ts) if ev_ts else None
        if ev_dt is None:
            continue

        by = str(ev.get("by") or "").strip()

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
            created_ts = str(actor.get("created_at") or "").strip()
            created_dt = parse_utc_iso(created_ts) if created_ts else None
            if created_dt is not None and created_dt > ev_dt:
                continue
            if not is_message_for_actor(group, actor_id=aid, event=ev):
                continue
            recipients.append(aid)

        if by != "user" and ("user" in to_set or "@user" in to_set):
            recipients.append("user")

        acked_set = acked_by_message.get(event_id, set())
        status: Dict[str, bool] = {}
        for rid in recipients:
            status[rid] = rid in acked_set
        result[event_id] = status

    return result


def get_obligation_status_batch(group: Group, events: List[Dict[str, Any]]) -> Dict[str, Dict[str, Dict[str, bool]]]:
    """Compute per-recipient obligation status for chat messages.

    Returns:
      {
        "<message_event_id>": {
          "<recipient_id>": {
            "read": bool,
            "acked": bool,
            "replied": bool,
            "reply_required": bool,
          },
          ...
        },
        ...
      }

    Notes:
    - Includes only local-group chat.message events (dst_group_id empty).
    - Recipients are resolved from current roster with message-time existence checks.
    - "user" is included only when explicitly targeted.
    """
    actors = list_actors(group)
    cursors = load_cursors(group)

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

    acked_by_message = _collect_chat_acks(group, event_ids=target_ids)
    replied_by_message = _collect_chat_replies(group, event_ids=target_ids)

    result: Dict[str, Dict[str, Dict[str, bool]]] = {}

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

        ev_ts = str(ev.get("ts") or "").strip()
        ev_dt = parse_utc_iso(ev_ts) if ev_ts else None
        if ev_dt is None:
            continue

        by = str(ev.get("by") or "").strip()
        is_attention = str(data.get("priority") or "normal").strip() == "attention"
        reply_required = bool(data.get("reply_required") is True)

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
            created_ts = str(actor.get("created_at") or "").strip()
            created_dt = parse_utc_iso(created_ts) if created_ts else None
            if created_dt is not None and created_dt > ev_dt:
                continue
            if not is_message_for_actor(group, actor_id=aid, event=ev):
                continue
            recipients.append(aid)

        if by != "user" and ("user" in to_set or "@user" in to_set):
            recipients.append("user")

        acked_set = acked_by_message.get(event_id, set())
        replied_set = replied_by_message.get(event_id, set())

        status: Dict[str, Dict[str, bool]] = {}
        for rid in recipients:
            cur = cursors.get(rid)
            cur_ts = str(cur.get("ts") or "") if isinstance(cur, dict) else ""
            cur_dt = parse_utc_iso(cur_ts) if cur_ts else None
            read = bool(cur_dt is not None and cur_dt >= ev_dt)

            replied = rid in replied_set
            acked = replied or (rid in acked_set)
            if is_attention and read:
                # mark_read on attention is treated as ack gesture for recipients.
                acked = True

            status[rid] = {
                "read": read,
                "acked": acked,
                "replied": replied,
                "reply_required": reply_required,
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


def unread_messages(group: Group, *, actor_id: str, limit: int = 50, kind_filter: MessageKindFilter = "all") -> List[Dict[str, Any]]:
    """Get unread events for an actor.

    Args:
        group: Working group
        actor_id: Actor id
        limit: Max results (0 = unlimited)
        kind_filter:
            - "all": chat.message + system.notify
            - "chat": chat.message only
            - "notify": system.notify only
    """
    _, cursor_ts = get_cursor(group, actor_id)
    cursor_dt = parse_utc_iso(cursor_ts) if cursor_ts else None

    # Determine which kinds to include.
    if kind_filter == "chat":
        allowed_kinds = {"chat.message"}
    elif kind_filter == "notify":
        allowed_kinds = {"system.notify"}
    else:
        allowed_kinds = {"chat.message", "system.notify"}

    out: List[Dict[str, Any]] = []
    for ev in iter_events(group.ledger_path):
        ev_kind = str(ev.get("kind") or "")
        if ev_kind not in allowed_kinds:
            continue
        # Exclude messages sent by the actor itself.
        if ev_kind == "chat.message" and str(ev.get("by") or "") == actor_id:
            continue
        # Check delivery/visibility rules.
        if not is_message_for_actor(group, actor_id=actor_id, event=ev):
            continue
        # Check read cursor.
        if cursor_dt is not None:
            ev_dt = parse_utc_iso(str(ev.get("ts") or ""))
            if ev_dt is not None and ev_dt <= cursor_dt:
                continue
        out.append(ev)
        if limit > 0 and len(out) >= limit:
            break
    return out


def unread_count(group: Group, *, actor_id: str, kind_filter: MessageKindFilter = "all") -> int:
    """Count unread events for an actor.

    Args:
        group: Working group
        actor_id: Actor id
        kind_filter: Same semantics as unread_messages()
    """
    _, cursor_ts = get_cursor(group, actor_id)
    cursor_dt = parse_utc_iso(cursor_ts) if cursor_ts else None

    # Determine which kinds to include.
    if kind_filter == "chat":
        allowed_kinds = {"chat.message"}
    elif kind_filter == "notify":
        allowed_kinds = {"system.notify"}
    else:
        allowed_kinds = {"chat.message", "system.notify"}

    count = 0
    for ev in iter_events(group.ledger_path):
        ev_kind = str(ev.get("kind") or "")
        if ev_kind not in allowed_kinds:
            continue
        if ev_kind == "chat.message" and str(ev.get("by") or "") == actor_id:
            continue
        if not is_message_for_actor(group, actor_id=actor_id, event=ev):
            continue
        if cursor_dt is not None:
            ev_dt = parse_utc_iso(str(ev.get("ts") or ""))
            if ev_dt is not None and ev_dt <= cursor_dt:
                continue
        count += 1
    return count


def batch_unread_counts(
    group: Group,
    *,
    actor_ids: List[str],
    kind_filter: MessageKindFilter = "all",
) -> Dict[str, int]:
    """Count unread events for multiple actors in a single ledger pass.

    This remains O(n * m) where n = actors and m = events, but it avoids
    re-reading/parsing the ledger for each actor and loads cursors once.

    Args:
        group: Working group
        actor_ids: List of actor ids to count for
        kind_filter: Same semantics as unread_messages()

    Returns:
        Dict mapping actor_id -> unread count
    """
    if not actor_ids:
        return {}

    # Load all cursors at once
    cursors = load_cursors(group)
    actor_cursor_dts: Dict[str, Optional[Any]] = {}
    for aid in actor_ids:
        cur = cursors.get(aid)
        if isinstance(cur, dict):
            cursor_ts = str(cur.get("ts") or "")
            actor_cursor_dts[aid] = parse_utc_iso(cursor_ts) if cursor_ts else None
        else:
            actor_cursor_dts[aid] = None

    # Determine which kinds to include
    if kind_filter == "chat":
        allowed_kinds = {"chat.message"}
    elif kind_filter == "notify":
        allowed_kinds = {"system.notify"}
    else:
        allowed_kinds = {"chat.message", "system.notify"}

    # Initialize counts
    counts: Dict[str, int] = {aid: 0 for aid in actor_ids}

    # Pre-compute actor roles once (optimization: avoids repeated get_effective_role calls)
    actor_roles: Dict[str, str] = {aid: get_effective_role(group, aid) for aid in actor_ids}

    # Single pass through the ledger
    for ev in iter_events(group.ledger_path):
        ev_kind = str(ev.get("kind") or "")
        if ev_kind not in allowed_kinds:
            continue

        ev_by = str(ev.get("by") or "")
        ev_ts = str(ev.get("ts") or "")
        ev_dt = parse_utc_iso(ev_ts) if ev_ts else None

        # Check each actor
        for aid in actor_ids:
            # Exclude messages sent by the actor itself
            if ev_kind == "chat.message" and ev_by == aid:
                continue
            # Check delivery/visibility rules (pass pre-computed role)
            if not is_message_for_actor(group, actor_id=aid, event=ev, role=actor_roles[aid]):
                continue
            # Check read cursor
            cursor_dt = actor_cursor_dts[aid]
            if cursor_dt is not None and ev_dt is not None and ev_dt <= cursor_dt:
                continue
            counts[aid] += 1

    return counts


def latest_unread_event(
    group: Group,
    *,
    actor_id: str,
    kind_filter: MessageKindFilter = "all",
) -> Optional[Dict[str, Any]]:
    """Get the latest unread event for an actor (or None if none).

    This is used for safe bulk-clear flows (mark-all-read): advance the cursor
    only up to the latest currently-unread message, without requiring clients
    to enumerate every event_id.
    """
    _, cursor_ts = get_cursor(group, actor_id)
    cursor_dt = parse_utc_iso(cursor_ts) if cursor_ts else None

    if kind_filter == "chat":
        allowed_kinds = {"chat.message"}
    elif kind_filter == "notify":
        allowed_kinds = {"system.notify"}
    else:
        allowed_kinds = {"chat.message", "system.notify"}

    last: Optional[Dict[str, Any]] = None
    for ev in iter_events(group.ledger_path):
        ev_kind = str(ev.get("kind") or "")
        if ev_kind not in allowed_kinds:
            continue
        if ev_kind == "chat.message" and str(ev.get("by") or "") == actor_id:
            continue
        if not is_message_for_actor(group, actor_id=actor_id, event=ev):
            continue
        if cursor_dt is not None:
            ev_dt = parse_utc_iso(str(ev.get("ts") or ""))
            if ev_dt is not None and ev_dt <= cursor_dt:
                continue
        last = ev
    return last


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


def find_event_with_chat_ack(group: Group, *, event_id: str, actor_id: str) -> Tuple[Optional[Dict[str, Any]], bool]:
    """Find an event and whether a matching chat.ack already exists."""
    raw_event_id = str(event_id or "").strip()
    wanted = raw_event_id if _is_full_event_id(raw_event_id) else resolve_event_id(group, raw_event_id)
    actor = str(actor_id or "").strip()
    if not wanted:
        return None, False

    found_event, found_ack = lookup_event_with_chat_ack_indexed(group.ledger_path, event_id=wanted, actor_id=actor)
    if found_event is None and _is_full_event_id(raw_event_id):
        return None, found_ack
    if found_event is not None:
        return found_event, found_ack

    for ev in iter_events_reverse(group.ledger_path):
        kind = str(ev.get("kind") or "").strip()
        if found_event is None and str(ev.get("id") or "").strip() == wanted:
            found_event = ev
            if found_ack:
                break
            continue
        if found_ack or kind != "chat.ack":
            continue
        data = ev.get("data")
        if not isinstance(data, dict):
            continue
        if str(data.get("event_id") or "").strip() != wanted:
            continue
        if actor and str(data.get("actor_id") or "").strip() != actor:
            continue
        found_ack = True
        if found_event is not None:
            break
    return found_event, found_ack


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
    """Get per-actor read status for a chat.message event."""
    ev = find_event(group, event_id)
    if ev is None:
        return {}

    if str(ev.get("kind") or "") != "chat.message":
        return {}

    ev_ts = str(ev.get("ts") or "")
    ev_dt = parse_utc_iso(ev_ts) if ev_ts else None
    if ev_dt is None:
        return {}

    cursors = load_cursors(group)
    result: Dict[str, bool] = {}

    by = str(ev.get("by") or "").strip()

    for actor in list_actors(group):
        if not isinstance(actor, dict):
            continue
        actor_id = str(actor.get("id") or "").strip()
        if not actor_id or actor_id == "user" or actor_id == by:
            continue
        created_ts = str(actor.get("created_at") or "").strip()
        created_dt = parse_utc_iso(created_ts) if created_ts else None
        if created_dt is not None and created_dt > ev_dt:
            # Actor did not exist yet at the time of this message.
            continue
        if not is_message_for_actor(group, actor_id=actor_id, event=ev):
            continue

        cur = cursors.get(actor_id)
        cur_ts = str(cur.get("ts") or "") if isinstance(cur, dict) else ""
        cur_dt = parse_utc_iso(cur_ts) if cur_ts else None
        result[actor_id] = bool(cur_dt is not None and cur_dt >= ev_dt)

    return result


def get_read_status_batch(
    group: Group,
    events: List[Dict[str, Any]],
) -> Dict[str, Dict[str, bool]]:
    """Batch compute per-actor read status for multiple chat.message events.

    This is an optimized version of get_read_status() that loads cursors and
    actors only once, avoiding N+1 queries.

    Args:
        group: Working group
        events: List of events (only chat.message events will be processed)

    Returns:
        Dict mapping event_id -> {actor_id: bool}
    """
    # Load shared data once
    cursors = load_cursors(group)
    actors = list_actors(group)

    result: Dict[str, Dict[str, bool]] = {}

    for ev in events:
        if str(ev.get("kind") or "") != "chat.message":
            continue

        event_id = str(ev.get("id") or "")
        if not event_id:
            continue

        ev_ts = str(ev.get("ts") or "")
        ev_dt = parse_utc_iso(ev_ts) if ev_ts else None
        if ev_dt is None:
            continue

        by = str(ev.get("by") or "").strip()
        status: Dict[str, bool] = {}

        for actor in actors:
            if not isinstance(actor, dict):
                continue
            actor_id = str(actor.get("id") or "").strip()
            if not actor_id or actor_id == "user" or actor_id == by:
                continue
            created_ts = str(actor.get("created_at") or "").strip()
            created_dt = parse_utc_iso(created_ts) if created_ts else None
            if created_dt is not None and created_dt > ev_dt:
                # Actor did not exist yet at the time of this message.
                continue
            if not is_message_for_actor(group, actor_id=actor_id, event=ev):
                continue

            cur = cursors.get(actor_id)
            cur_ts = str(cur.get("ts") or "") if isinstance(cur, dict) else ""
            cur_dt = parse_utc_iso(cur_ts) if cur_ts else None
            status[actor_id] = bool(cur_dt is not None and cur_dt >= ev_dt)

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
                    title = str(data.get("title") or "").lower()
                    message = str(data.get("message") or "").lower()
                    if query_lower not in text and query_lower not in title and query_lower not in message:
                        continue
                else:
                    continue
                events.append(ev)
        if events:
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
                title = str(data.get("title") or "").lower()
                message = str(data.get("message") or "").lower()
                if query_lower not in text and query_lower not in title and query_lower not in message:
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
