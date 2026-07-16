from __future__ import annotations

import logging
import sqlite3
from pathlib import Path
from typing import Any, Dict, List

from ..util.time import parse_utc_iso
from .actors import list_actors
from .group import Group, load_group
from .inbox import is_message_for_actor
from .ledger_index import lookup_event_by_id

_SCHEMA_VERSION = 1
_DEFAULT_TIMEOUT_SECONDS = 5.0
_MAX_CACHED_MESSAGES = 2000
logger = logging.getLogger("cccc.ledger.status_cache")


def _status_index_path(group: Group) -> Path:
    return group.path / "state" / "ledger" / "status.sqlite3"


def _connect(path: Path) -> sqlite3.Connection:
    path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(path), timeout=_DEFAULT_TIMEOUT_SECONDS)
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA synchronous=NORMAL")
    conn.execute("PRAGMA temp_store=MEMORY")
    return conn


def _meta_int(conn: sqlite3.Connection, key: str) -> int:
    row = conn.execute("SELECT value FROM meta WHERE key = ?", (str(key or "").strip(),)).fetchone()
    if row is None:
        return 0
    try:
        return int(row[0] or 0)
    except Exception:
        return 0


def _ensure_schema(conn: sqlite3.Connection) -> None:
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS message_status_meta (
            event_id TEXT PRIMARY KEY,
            ts TEXT NOT NULL,
            is_attention INTEGER NOT NULL,
            has_obligation INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS recipient_status (
            event_id TEXT NOT NULL,
            actor_id TEXT NOT NULL,
            is_read INTEGER NOT NULL,
            is_acked INTEGER NOT NULL,
            is_replied INTEGER NOT NULL,
            reply_required INTEGER NOT NULL,
            PRIMARY KEY (event_id, actor_id)
        );

        CREATE INDEX IF NOT EXISTS idx_message_status_meta_ts ON message_status_meta(ts, event_id);
        CREATE INDEX IF NOT EXISTS idx_recipient_status_event_id ON recipient_status(event_id);
        """
    )
    current = _meta_int(conn, "schema_version")
    if current != _SCHEMA_VERSION:
        conn.execute("DELETE FROM recipient_status")
        conn.execute("DELETE FROM message_status_meta")
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            ("schema_version", str(_SCHEMA_VERSION)),
        )


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
    conn.execute(f"DELETE FROM recipient_status WHERE event_id IN ({placeholders})", tuple(stale_ids))
    conn.execute(f"DELETE FROM message_status_meta WHERE event_id IN ({placeholders})", tuple(stale_ids))


def _recipient_actor_ids(group: Group, event: Dict[str, Any]) -> List[str]:
    by = str(event.get("by") or "").strip()
    ev_ts = str(event.get("ts") or "").strip()
    ev_dt = parse_utc_iso(ev_ts) if ev_ts else None
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    to_raw = data.get("to")
    to_tokens = [str(item).strip() for item in to_raw] if isinstance(to_raw, list) else []
    to_set = {token for token in to_tokens if token}

    recipients: List[str] = []
    for actor in list_actors(group):
        if not isinstance(actor, dict):
            continue
        actor_id = str(actor.get("id") or "").strip()
        if not actor_id or actor_id == "user" or actor_id == by:
            continue
        created_ts = str(actor.get("created_at") or "").strip()
        created_dt = parse_utc_iso(created_ts) if created_ts else None
        if ev_dt is not None and created_dt is not None and created_dt > ev_dt:
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
    ack_status: Dict[str, bool],
    obligation_status: Dict[str, Dict[str, bool]],
) -> None:
    event_id = str(event.get("id") or "").strip()
    if not event_id:
        return
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    is_attention = int(str(data.get("priority") or "normal").strip() == "attention")
    has_obligation = int(not str(data.get("dst_group_id") or "").strip())
    conn.execute(
        """
        INSERT INTO message_status_meta(event_id, ts, is_attention, has_obligation)
        VALUES(?, ?, ?, ?)
        ON CONFLICT(event_id) DO UPDATE SET
            ts=excluded.ts,
            is_attention=excluded.is_attention,
            has_obligation=excluded.has_obligation
        """,
        (event_id, str(event.get("ts") or ""), is_attention, has_obligation),
    )
    recipients = _recipient_actor_ids(group, event)
    for actor_id in recipients:
        obligation = obligation_status.get(actor_id) if isinstance(obligation_status.get(actor_id), dict) else {}
        conn.execute(
            """
            INSERT INTO recipient_status(event_id, actor_id, is_read, is_acked, is_replied, reply_required)
            VALUES(?, ?, ?, ?, ?, ?)
            ON CONFLICT(event_id, actor_id) DO UPDATE SET
                is_read=excluded.is_read,
                is_acked=excluded.is_acked,
                is_replied=excluded.is_replied,
                reply_required=excluded.reply_required
            """,
            (
                event_id,
                actor_id,
                1 if bool(read_status.get(actor_id)) else 0,
                1 if bool(ack_status.get(actor_id) or obligation.get("acked")) else 0,
                1 if bool(obligation.get("replied")) else 0,
                1 if bool(obligation.get("reply_required")) else 0,
            ),
        )


def store_message_status_batch(
    group: Group,
    events: List[Dict[str, Any]],
    *,
    read_status_by_event: Dict[str, Dict[str, bool]],
    ack_status_by_event: Dict[str, Dict[str, bool]],
    obligation_status_by_event: Dict[str, Dict[str, Dict[str, bool]]],
) -> None:
    if not events:
        return
    conn = _connect(_status_index_path(group))
    try:
        _ensure_schema(conn)
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
                ack_status=ack_status_by_event.get(event_id, {}),
                obligation_status=obligation_status_by_event.get(event_id, {}),
            )
        _prune(conn)
        conn.commit()
    finally:
        conn.close()


def get_cached_message_status_batch(group: Group, event_ids: List[str]) -> Dict[str, Dict[str, Any]]:
    normalized_ids = [str(event_id or "").strip() for event_id in event_ids if str(event_id or "").strip()]
    if not normalized_ids:
        return {}
    conn = _connect(_status_index_path(group))
    try:
        _ensure_schema(conn)
        placeholders = ", ".join("?" for _ in normalized_ids)
        meta_rows = conn.execute(
            f"SELECT event_id, is_attention, has_obligation FROM message_status_meta WHERE event_id IN ({placeholders})",
            tuple(normalized_ids),
        ).fetchall()
        meta_by_id = {
            str(row[0] or "").strip(): {
                "is_attention": bool(int(row[1] or 0)),
                "has_obligation": bool(int(row[2] or 0)),
            }
            for row in meta_rows
            if str(row[0] or "").strip()
        }
        status_rows = conn.execute(
            f"""
            SELECT event_id, actor_id, is_read, is_acked, is_replied, reply_required
            FROM recipient_status
            WHERE event_id IN ({placeholders})
            """,
            tuple(normalized_ids),
        ).fetchall()
    finally:
        conn.close()

    result: Dict[str, Dict[str, Any]] = {}
    for event_id, meta in meta_by_id.items():
        payload: Dict[str, Any] = {"read_status": {}}
        if meta.get("is_attention"):
            payload["ack_status"] = {}
        if meta.get("has_obligation"):
            payload["obligation_status"] = {}
        result[event_id] = payload

    for row in status_rows:
        event_id = str(row[0] or "").strip()
        actor_id = str(row[1] or "").strip()
        if not event_id or not actor_id or event_id not in result:
            continue
        payload = result[event_id]
        payload["read_status"][actor_id] = bool(int(row[2] or 0))
        if isinstance(payload.get("ack_status"), dict):
            payload["ack_status"][actor_id] = bool(int(row[3] or 0))
        if isinstance(payload.get("obligation_status"), dict):
            payload["obligation_status"][actor_id] = {
                "read": bool(int(row[2] or 0)),
                "acked": bool(int(row[3] or 0)),
                "replied": bool(int(row[4] or 0)),
                "reply_required": bool(int(row[5] or 0)),
            }
    logger.debug(
        "ledger_status_cache_read group_id=%s requested=%d hit=%d miss=%d",
        str(getattr(group, "group_id", "") or ""),
        len(normalized_ids),
        len(result),
        max(0, len(normalized_ids) - len(result)),
    )
    return result


def _apply_read_update(conn: sqlite3.Connection, event_id: str, actor_id: str) -> None:
    row = conn.execute(
        "SELECT is_attention FROM message_status_meta WHERE event_id = ?",
        (event_id,),
    ).fetchone()
    if row is None:
        return
    is_attention = bool(int(row[0] or 0))
    if is_attention:
        conn.execute(
            """
            UPDATE recipient_status
            SET is_read = 1, is_acked = 1
            WHERE event_id = ? AND actor_id = ?
            """,
            (event_id, actor_id),
        )
        return
    conn.execute(
        "UPDATE recipient_status SET is_read = 1 WHERE event_id = ? AND actor_id = ?",
        (event_id, actor_id),
    )


def _apply_ack_update(conn: sqlite3.Connection, event_id: str, actor_id: str) -> None:
    conn.execute(
        "UPDATE recipient_status SET is_acked = 1 WHERE event_id = ? AND actor_id = ?",
        (event_id, actor_id),
    )


def _apply_reply_update(conn: sqlite3.Connection, event_id: str, actor_id: str) -> None:
    conn.execute(
        """
        UPDATE recipient_status
        SET is_replied = 1, is_acked = 1
        WHERE event_id = ? AND actor_id = ?
        """,
        (event_id, actor_id),
    )


def update_message_status_cache_on_append(event: Dict[str, Any]) -> None:
    group_id = str(event.get("group_id") or "").strip()
    kind = str(event.get("kind") or "").strip()
    if not group_id or kind not in {"chat.message", "chat.read", "chat.ack"}:
        return
    group = load_group(group_id)
    if group is None:
        return
    conn = _connect(_status_index_path(group))
    try:
        _ensure_schema(conn)
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if kind == "chat.message":
            read_status: Dict[str, bool] = {}
            ack_status: Dict[str, bool] = {}
            obligation_status: Dict[str, Dict[str, bool]] = {}
            recipients = _recipient_actor_ids(group, event)
            is_attention = str(data.get("priority") or "normal").strip() == "attention"
            reply_required = bool(data.get("reply_required") is True)
            for actor_id in recipients:
                read_status[actor_id] = False
                if is_attention:
                    ack_status[actor_id] = False
                obligation_status[actor_id] = {
                    "read": False,
                    "acked": not is_attention,
                    "replied": False,
                    "reply_required": reply_required,
                }
            _write_event_status_rows(
                conn,
                group,
                event,
                read_status=read_status,
                ack_status=ack_status,
                obligation_status=obligation_status,
            )
            reply_to = str(data.get("reply_to") or "").strip()
            by = str(event.get("by") or "").strip()
            if reply_to and by:
                _apply_reply_update(conn, reply_to, by)
        elif kind == "chat.read":
            actor_id = str(data.get("actor_id") or "").strip()
            event_id = str(data.get("event_id") or "").strip()
            if actor_id and event_id:
                _apply_read_update(conn, event_id, actor_id)
        elif kind == "chat.ack":
            actor_id = str(data.get("actor_id") or "").strip()
            event_id = str(data.get("event_id") or "").strip()
            if actor_id and event_id:
                _apply_ack_update(conn, event_id, actor_id)
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
    update_message_status_cache_on_append(event)
