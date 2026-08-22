from __future__ import annotations

import logging
import sqlite3
from pathlib import Path
from typing import Any, Dict, List

from .actors import list_actors
from .group import Group, load_group
from .inbox import (
    actor_existed_at_event,
    actor_generation_positions,
    is_message_for_actor,
)
from .ledger_index import lookup_event_by_id, lookup_event_positions
from .ledger_status_db import (
    connect_status_db,
    ensure_status_schema,
    is_database_busy_error,
)

_MAX_CACHED_MESSAGES = 2000
logger = logging.getLogger("cccc.ledger.status_cache")


def _status_index_path(group: Group) -> Path:
    return group.path / "state" / "ledger" / "status.sqlite3"


def _prune(conn: sqlite3.Connection) -> None:
    rows = conn.execute(
        """
        SELECT event_id FROM message_status_meta
        ORDER BY ts DESC, event_id DESC
        LIMIT -1 OFFSET ?
        """,
        (_MAX_CACHED_MESSAGES,),
    ).fetchall()
    stale_ids = [str(row[0] or "").strip() for row in rows if str(row[0] or "").strip()]
    if not stale_ids:
        return
    placeholders = ", ".join("?" for _ in stale_ids)
    conn.execute(
        f"DELETE FROM recipient_status WHERE event_id IN ({placeholders})",
        tuple(stale_ids),
    )
    conn.execute(
        f"DELETE FROM message_status_meta WHERE event_id IN ({placeholders})",
        tuple(stale_ids),
    )


def _invalidate_event(conn: sqlite3.Connection, event_id: str) -> None:
    conn.execute("DELETE FROM recipient_status WHERE event_id = ?", (event_id,))
    conn.execute("DELETE FROM message_status_meta WHERE event_id = ?", (event_id,))


def _recipient_actor_ids(group: Group, event: Dict[str, Any]) -> List[str]:
    by = str(event.get("by") or "").strip()
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    to_raw = data.get("to")
    to_tokens = (
        [str(item).strip() for item in to_raw] if isinstance(to_raw, list) else []
    )
    to_set = {token for token in to_tokens if token}

    actors = list_actors(group)
    event_id = str(event.get("id") or "").strip()
    event_position = (
        lookup_event_positions(group.ledger_path, [event_id])[0] if event_id else None
    )
    positions = (
        {event_id: event_position} if event_id and event_position is not None else {}
    )
    generations = actor_generation_positions(
        group,
        [str(actor.get("id") or "") for actor in actors if isinstance(actor, dict)],
    )

    recipients: List[str] = []
    for actor in actors:
        if not isinstance(actor, dict):
            continue
        actor_id = str(actor.get("id") or "").strip()
        if not actor_id or actor_id == "user" or actor_id == by:
            continue
        if not actor_existed_at_event(
            group,
            actor=actor,
            event=event,
            positions=positions,
            generations=generations,
        ):
            continue
        if not is_message_for_actor(group, actor_id=actor_id, event=event):
            continue
        recipients.append(actor_id)

    if by != "user" and ("user" in to_set or "@user" in to_set):
        recipients.append("user")
    return recipients


def _write_event_status_rows(
    conn: sqlite3.Connection,
    group: Group,
    event: Dict[str, Any],
    *,
    read_status: Dict[str, bool],
    obligation_status: Dict[str, Dict[str, Any]],
) -> None:
    event_id = str(event.get("id") or "").strip()
    if not event_id:
        return
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    has_obligation = int(not str(data.get("dst_group_id") or "").strip())
    conn.execute(
        """
        INSERT INTO message_status_meta(event_id, ts, has_obligation, has_read_status)
        VALUES(?, ?, ?, ?)
        ON CONFLICT(event_id) DO UPDATE SET
            ts=excluded.ts,
            has_obligation=excluded.has_obligation,
            has_read_status=excluded.has_read_status
        """,
        (
            event_id,
            str(event.get("ts") or ""),
            has_obligation,
            int(str(data.get("message_mode") or "") == "mail"),
        ),
    )
    recipients = _recipient_actor_ids(group, event)
    for actor_id in recipients:
        obligation = (
            obligation_status.get(actor_id)
            if isinstance(obligation_status.get(actor_id), dict)
            else {}
        )
        conn.execute(
            """
            INSERT INTO recipient_status(
                event_id, actor_id, is_read, is_replied,
                reply_requested, cancelled, delivery_state
            )
            VALUES(?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(event_id, actor_id) DO UPDATE SET
                is_read=excluded.is_read,
                is_replied=excluded.is_replied,
                reply_requested=excluded.reply_requested,
                cancelled=excluded.cancelled,
                delivery_state=excluded.delivery_state
            """,
            (
                event_id,
                actor_id,
                1 if bool(read_status.get(actor_id)) else 0,
                1 if bool(obligation.get("replied")) else 0,
                1 if bool(obligation.get("reply_requested")) else 0,
                1 if bool(obligation.get("cancelled")) else 0,
                str(obligation.get("delivery_state") or ""),
            ),
        )


def store_message_status_batch(
    group: Group,
    events: List[Dict[str, Any]],
    *,
    read_status_by_event: Dict[str, Dict[str, bool]],
    obligation_status_by_event: Dict[str, Dict[str, Dict[str, Any]]],
) -> None:
    if not events:
        return
    conn = connect_status_db(_status_index_path(group))
    try:
        ensure_status_schema(conn)
        for event in events:
            if str(event.get("kind") or "") != "chat.message":
                continue
            event_id = str(event.get("id") or "").strip()
            if not event_id:
                continue
            _write_event_status_rows(
                conn,
                group,
                event,
                read_status=read_status_by_event.get(event_id, {}),
                obligation_status=obligation_status_by_event.get(event_id, {}),
            )
        _prune(conn)
        conn.commit()
    finally:
        conn.close()


def get_cached_message_status_batch(
    group: Group, event_ids: List[str]
) -> Dict[str, Dict[str, Any]]:
    normalized_ids = [
        str(event_id or "").strip()
        for event_id in event_ids
        if str(event_id or "").strip()
    ]
    if not normalized_ids:
        return {}
    conn: sqlite3.Connection | None = None
    try:
        conn = connect_status_db(_status_index_path(group))
        ensure_status_schema(conn)
        placeholders = ", ".join("?" for _ in normalized_ids)
        meta_rows = conn.execute(
            f"SELECT event_id, has_obligation, has_read_status FROM message_status_meta WHERE event_id IN ({placeholders})",
            tuple(normalized_ids),
        ).fetchall()
        meta_by_id = {
            str(row[0] or "").strip(): {
                "has_obligation": bool(int(row[1] or 0)),
                "has_read_status": bool(int(row[2] or 0)),
            }
            for row in meta_rows
            if str(row[0] or "").strip()
        }
        status_rows = conn.execute(
            f"""
            SELECT event_id, actor_id, is_read, is_replied,
                   reply_requested, cancelled, delivery_state
            FROM recipient_status
            WHERE event_id IN ({placeholders})
            """,
            tuple(normalized_ids),
        ).fetchall()
    except sqlite3.OperationalError as exc:
        if not is_database_busy_error(exc):
            raise
        logger.warning(
            "ledger_status_cache_read_busy group_id=%s requested=%d",
            str(getattr(group, "group_id", "") or ""),
            len(normalized_ids),
        )
        return {}
    finally:
        if conn is not None:
            conn.close()

    result: Dict[str, Dict[str, Any]] = {}
    for event_id, meta in meta_by_id.items():
        payload: Dict[str, Any] = {}
        if meta.get("has_read_status"):
            payload["read_status"] = {}
        if meta.get("has_obligation"):
            payload["obligation_status"] = {}
        result[event_id] = payload

    for row in status_rows:
        event_id = str(row[0] or "").strip()
        actor_id = str(row[1] or "").strip()
        if not event_id or not actor_id or event_id not in result:
            continue
        payload = result[event_id]
        if isinstance(payload.get("read_status"), dict):
            payload["read_status"][actor_id] = bool(int(row[2] or 0))
        if isinstance(payload.get("obligation_status"), dict):
            payload["obligation_status"][actor_id] = {
                "replied": bool(int(row[3] or 0)),
                "reply_requested": bool(int(row[4] or 0)),
                "cancelled": bool(int(row[5] or 0)),
                "delivery_state": str(row[6] or ""),
            }
    logger.debug(
        "ledger_status_cache_read group_id=%s requested=%d hit=%d miss=%d",
        str(getattr(group, "group_id", "") or ""),
        len(normalized_ids),
        len(result),
        max(0, len(normalized_ids) - len(result)),
    )
    return result


def _apply_delivery_update(
    conn: sqlite3.Connection,
    event_id: str,
    actor_id: str,
    state: str,
) -> None:
    conn.execute(
        "UPDATE recipient_status SET delivery_state = ? WHERE event_id = ? AND actor_id = ?",
        (state, event_id, actor_id),
    )


def update_message_status_cache_on_append(
    event: Dict[str, Any],
    *,
    cache_new_message: bool = False,
) -> None:
    """Maintain only already-materialized status state on the append path.

    New message rows are read-through data and are intentionally populated by
    the explicit warm/read path. Replies still update a cached target row, and
    actor/read/delivery/cancellation events still invalidate or update existing rows.
    """
    group_id = str(event.get("group_id") or "").strip()
    kind = str(event.get("kind") or "").strip()
    if not group_id or kind not in {
        "actor.add",
        "actor.remove",
        "chat.message",
        "mail.read",
        "chat.reply_request.cancelled",
        "runtime.delivery",
    }:
        return
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    reply_to = str(data.get("reply_to") or "").strip()
    if kind == "chat.message" and not cache_new_message and not reply_to:
        return
    group = load_group(group_id)
    if group is None:
        return
    conn = connect_status_db(_status_index_path(group))
    try:
        ensure_status_schema(conn)
        if kind in {"actor.add", "actor.remove"}:
            conn.execute("DELETE FROM recipient_status")
            conn.execute("DELETE FROM message_status_meta")
            conn.commit()
            return
        if kind == "chat.message":
            if cache_new_message:
                read_status: Dict[str, bool] = {}
                obligation_status: Dict[str, Dict[str, Any]] = {}
                recipients = _recipient_actor_ids(group, event)
                reply_requested = str(data.get("message_mode") or "") == "request_reply"
                for actor_id in recipients:
                    if str(data.get("message_mode") or "") == "mail":
                        read_status[actor_id] = False
                    obligation_status[actor_id] = {
                        "replied": False,
                        "reply_requested": reply_requested,
                        "cancelled": False,
                        "delivery_state": "",
                    }
                _write_event_status_rows(
                    conn,
                    group,
                    event,
                    read_status=read_status,
                    obligation_status=obligation_status,
                )
            by = str(event.get("by") or "").strip()
            if reply_to and by:
                # Reply/cancellation is a first-terminal-wins contract. Drop the
                # materialized row so the next read recomputes ledger order.
                _invalidate_event(conn, reply_to)
        elif kind == "mail.read":
            actor_id = str(data.get("actor_id") or "").strip()
            event_id = str(data.get("event_id") or "").strip()
            if actor_id and event_id:
                # A Mail cursor covers every earlier Mail event in ledger order.
                # Invalidating the bounded cache is simpler and safer than trying
                # to reproduce ledger ordering inside this projection database.
                conn.execute("DELETE FROM recipient_status")
                conn.execute("DELETE FROM message_status_meta")
        elif kind == "chat.reply_request.cancelled":
            event_id = str(data.get("source_event_id") or "").strip()
            if event_id:
                _invalidate_event(conn, event_id)
        elif kind == "runtime.delivery":
            actor_id = str(data.get("actor_id") or "").strip()
            event_id = str(data.get("source_event_id") or "").strip()
            state = str(data.get("state") or "").strip()
            if actor_id and event_id and state:
                _apply_delivery_update(conn, event_id, actor_id, state)
        _prune(conn)
        conn.commit()
        logger.debug(
            "ledger_status_cache_write group_id=%s kind=%s event_id=%s",
            group_id,
            kind,
            str(event.get("id") or "").strip(),
        )
    finally:
        conn.close()


def warm_message_status_cache_from_event(group: Group, event_id: str) -> None:
    event = lookup_event_by_id(group.ledger_path, event_id)
    if not isinstance(event, dict) or str(event.get("kind") or "") != "chat.message":
        return
    update_message_status_cache_on_append(event, cache_new_message=True)
