from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict, Iterable, List, Literal, Optional, Tuple

from ..util.fs import atomic_write_json, read_json
from ..util.time import parse_utc_iso, utc_now_iso
from .actors import find_actor, get_effective_role, list_actors
from .group import Group


# Message kind filter
MessageKindFilter = Literal["all", "chat", "notify"]


def iter_events(ledger_path: Path) -> Iterable[Dict[str, Any]]:
    """Iterate over all events in a ledger file."""
    if not ledger_path.exists():
        return
    with ledger_path.open("r", encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except Exception:
                continue
            if isinstance(obj, dict):
                yield obj


def _cursor_path(group: Group) -> Path:
    return group.path / "state" / "read_cursors.json"


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

    # system.notify: check target_actor_id
    if kind == "system.notify":
        data = event.get("data")
        if not isinstance(data, dict):
            return False
        target = str(data.get("target_actor_id") or "").strip()
        # Empty target = broadcast to everyone
        if not target:
            return True
        return target == actor_id

    # chat.message: check the "to" field
    targets = _message_targets(event)

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


def find_event(group: Group, event_id: str) -> Optional[Dict[str, Any]]:
    """Find an event by event_id."""
    wanted = event_id.strip()
    if not wanted:
        return None
    for ev in iter_events(group.ledger_path):
        if str(ev.get("id") or "") == wanted:
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
    
    # Collect all matching events
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
