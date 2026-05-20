from __future__ import annotations

import json
import sqlite3
from pathlib import Path
from typing import Any, Dict, Optional

from .ledger_segments import ACTIVE_SOURCE_SEQ, iter_source_lines, list_ledger_sources, open_ledger_source_text


_SCHEMA_VERSION = 4
_DEFAULT_TIMEOUT_SECONDS = 5.0
_EVENTS_REQUIRED_COLUMNS = {
    "event_id",
    "ts",
    "kind",
    "by_actor",
    "reply_to",
    "source_seq",
    "source_path",
    "line_no",
    "offset_bytes",
}


def _index_path_for_ledger(ledger_path: Path) -> Path:
    return ledger_path.parent / "state" / "ledger" / "index.sqlite3"


def _connect(index_path: Path) -> sqlite3.Connection:
    index_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(index_path), timeout=_DEFAULT_TIMEOUT_SECONDS)
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


def _table_exists(conn: sqlite3.Connection, table_name: str) -> bool:
    row = conn.execute(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ? LIMIT 1",
        (str(table_name or "").strip(),),
    ).fetchone()
    return row is not None


def _table_columns(conn: sqlite3.Connection, table_name: str) -> set[str]:
    if not _table_exists(conn, table_name):
        return set()
    rows = conn.execute(f"PRAGMA table_info({table_name})").fetchall()
    return {str(row[1] or "").strip() for row in rows if len(row) > 1 and str(row[1] or "").strip()}


def _reset_legacy_schema(conn: sqlite3.Connection) -> bool:
    columns = _table_columns(conn, "events")
    if not columns:
        return False
    if _EVENTS_REQUIRED_COLUMNS.issubset(columns):
        return False
    conn.execute("DROP INDEX IF EXISTS idx_events_reply_to")
    conn.execute("DROP INDEX IF EXISTS idx_events_ts")
    conn.execute("DROP INDEX IF EXISTS idx_events_kind_ts")
    conn.execute("DROP INDEX IF EXISTS idx_events_by_ts")
    conn.execute("DROP INDEX IF EXISTS idx_events_source_line")
    conn.execute("DROP TABLE IF EXISTS chat_ack")
    conn.execute("DROP TABLE IF EXISTS event_search")
    conn.execute("DROP TABLE IF EXISTS source_state")
    conn.execute("DROP TABLE IF EXISTS events")
    return True


def _rebuild_events_indexes(conn: sqlite3.Connection) -> None:
    conn.execute("CREATE INDEX IF NOT EXISTS idx_events_reply_to ON events(reply_to)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_events_kind_ts ON events(kind, ts, source_seq, line_no)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_events_by_ts ON events(by_actor, ts, source_seq, line_no)")
    conn.execute("CREATE INDEX IF NOT EXISTS idx_events_source_line ON events(source_path, line_no)")


def _ensure_schema(conn: sqlite3.Connection) -> None:
    rebuilt = _reset_legacy_schema(conn)
    conn.executescript(
        """
        CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS events (
            event_id TEXT PRIMARY KEY,
            ts TEXT NOT NULL,
            kind TEXT NOT NULL,
            by_actor TEXT NOT NULL,
            reply_to TEXT NOT NULL,
            source_seq INTEGER NOT NULL,
            source_path TEXT NOT NULL,
            line_no INTEGER NOT NULL,
            offset_bytes INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS chat_ack (
            event_id TEXT NOT NULL,
            actor_id TEXT NOT NULL,
            PRIMARY KEY (event_id, actor_id)
        );

        CREATE TABLE IF NOT EXISTS source_state (
            source_path TEXT PRIMARY KEY,
            compressed INTEGER NOT NULL,
            file_size INTEGER NOT NULL,
            mtime_ns INTEGER NOT NULL,
            last_offset_bytes INTEGER NOT NULL,
            last_line_no INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS event_search (
            event_id TEXT PRIMARY KEY,
            searchable_text TEXT NOT NULL
        );
        """
    )
    _rebuild_events_indexes(conn)
    current = _meta_int(conn, "schema_version")
    if current != _SCHEMA_VERSION or rebuilt:
        conn.execute("DELETE FROM chat_ack")
        conn.execute("DELETE FROM events")
        conn.execute("DELETE FROM source_state")
        conn.execute("DELETE FROM event_search")
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?, ?) "
            "ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            ("schema_version", str(_SCHEMA_VERSION)),
        )


def _source_stat(path: Path) -> tuple[int, int]:
    try:
        st = path.stat()
        return max(0, int(st.st_size)), max(0, int(getattr(st, "st_mtime_ns", 0) or 0))
    except Exception:
        return 0, 0


def _delete_source_rows(conn: sqlite3.Connection, source_path: str) -> None:
    conn.execute("DELETE FROM event_search WHERE event_id IN (SELECT event_id FROM events WHERE source_path = ?)", (source_path,))
    conn.execute("DELETE FROM chat_ack WHERE event_id IN (SELECT event_id FROM events WHERE source_path = ?)", (source_path,))
    conn.execute("DELETE FROM events WHERE source_path = ?", (source_path,))
    conn.execute("DELETE FROM source_state WHERE source_path = ?", (source_path,))


def _searchable_text(event: Dict[str, Any]) -> str:
    kind = str(event.get("kind") or "").strip()
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    parts: list[str] = [kind]
    if isinstance(data, dict):
        for key in ("text", "title", "message", "quote_text"):
            value = data.get(key)
            if isinstance(value, str) and value.strip():
                parts.append(value.strip())
    return "\n".join(parts).lower()


def _index_event(
    conn: sqlite3.Connection,
    event: Dict[str, Any],
    *,
    source_seq: int,
    source_path: str,
    line_no: int,
    offset_bytes: int,
) -> None:
    event_id = str(event.get("id") or "").strip()
    if not event_id:
        return
    kind = str(event.get("kind") or "").strip()
    by_actor = str(event.get("by") or "").strip()
    ts = str(event.get("ts") or "").strip()
    data = event.get("data") if isinstance(event.get("data"), dict) else {}
    reply_to = str(data.get("reply_to") or "").strip() if isinstance(data, dict) else ""
    conn.execute(
        """
        INSERT INTO events(event_id, ts, kind, by_actor, reply_to, source_seq, source_path, line_no, offset_bytes)
        VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT(event_id) DO UPDATE SET
            ts=excluded.ts,
            kind=excluded.kind,
            by_actor=excluded.by_actor,
            reply_to=excluded.reply_to,
            source_seq=excluded.source_seq,
            source_path=excluded.source_path,
            line_no=excluded.line_no,
            offset_bytes=excluded.offset_bytes
        """,
        (event_id, ts, kind, by_actor, reply_to, int(source_seq or 0), source_path, int(line_no or 0), int(offset_bytes or 0)),
    )
    conn.execute(
        """
        INSERT INTO event_search(event_id, searchable_text)
        VALUES(?, ?)
        ON CONFLICT(event_id) DO UPDATE SET searchable_text=excluded.searchable_text
        """,
        (event_id, _searchable_text(event)),
    )
    if kind == "chat.ack" and isinstance(data, dict):
        ack_event_id = str(data.get("event_id") or "").strip()
        ack_actor_id = str(data.get("actor_id") or "").strip()
        if ack_event_id and ack_actor_id:
            conn.execute(
                "INSERT OR IGNORE INTO chat_ack(event_id, actor_id) VALUES(?, ?)",
                (ack_event_id, ack_actor_id),
            )


def _reindex_source(conn: sqlite3.Connection, ledger_path: Path, source: Dict[str, Any]) -> None:
    source_path = str(source.get("path") or "").strip()
    abs_path = source.get("abs_path")
    compressed = bool(source.get("compressed"))
    source_seq = int(source.get("seq") or 0)
    if not source_path or not isinstance(abs_path, Path) or not abs_path.exists():
        return
    _delete_source_rows(conn, source_path)
    line_no = 0
    if compressed:
        for raw_line in iter_source_lines(abs_path):
            line_no += 1
            line = raw_line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line)
            except Exception:
                continue
            if isinstance(obj, dict):
                _index_event(conn, obj, source_seq=source_seq, source_path=source_path, line_no=line_no, offset_bytes=0)
        size_bytes, mtime_ns = _source_stat(abs_path)
        conn.execute(
            """
            INSERT INTO source_state(source_path, compressed, file_size, mtime_ns, last_offset_bytes, last_line_no)
            VALUES(?, ?, ?, ?, ?, ?)
            ON CONFLICT(source_path) DO UPDATE SET
                compressed=excluded.compressed,
                file_size=excluded.file_size,
                mtime_ns=excluded.mtime_ns,
                last_offset_bytes=excluded.last_offset_bytes,
                last_line_no=excluded.last_line_no
            """,
            (source_path, 1, size_bytes, mtime_ns, size_bytes, line_no),
        )
        return

    offset_bytes = 0
    with abs_path.open("rb") as handle:
        while True:
            line_start = handle.tell()
            raw_line = handle.readline()
            if not raw_line:
                break
            offset_bytes = int(handle.tell())
            line_no += 1
            line = raw_line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line.decode("utf-8", errors="replace"))
            except Exception:
                continue
            if isinstance(obj, dict):
                _index_event(conn, obj, source_seq=source_seq, source_path=source_path, line_no=line_no, offset_bytes=line_start)
    size_bytes, mtime_ns = _source_stat(abs_path)
    conn.execute(
        """
        INSERT INTO source_state(source_path, compressed, file_size, mtime_ns, last_offset_bytes, last_line_no)
        VALUES(?, ?, ?, ?, ?, ?)
        ON CONFLICT(source_path) DO UPDATE SET
            compressed=excluded.compressed,
            file_size=excluded.file_size,
            mtime_ns=excluded.mtime_ns,
            last_offset_bytes=excluded.last_offset_bytes,
            last_line_no=excluded.last_line_no
        """,
        (source_path, 0, size_bytes, mtime_ns, offset_bytes, line_no),
    )


def _catch_up_plain_source(conn: sqlite3.Connection, ledger_path: Path, source: Dict[str, Any]) -> None:
    source_path = str(source.get("path") or "").strip()
    abs_path = source.get("abs_path")
    source_seq = int(source.get("seq") or 0)
    if not source_path or not isinstance(abs_path, Path) or not abs_path.exists():
        return
    row = conn.execute(
        "SELECT compressed, file_size, mtime_ns, last_offset_bytes, last_line_no FROM source_state WHERE source_path = ?",
        (source_path,),
    ).fetchone()
    size_bytes, mtime_ns = _source_stat(abs_path)
    if row is None:
        _reindex_source(conn, ledger_path, source)
        return
    try:
        compressed = int(row[0] or 0)
        prev_size = int(row[1] or 0)
        prev_mtime_ns = int(row[2] or 0)
        last_offset = int(row[3] or 0)
        last_line_no = int(row[4] or 0)
    except Exception:
        _reindex_source(conn, abs_path, source)
        return
    if compressed or size_bytes < last_offset or prev_mtime_ns > mtime_ns:
        _reindex_source(conn, abs_path, source)
        return
    if size_bytes == prev_size and mtime_ns == prev_mtime_ns:
        return
    line_no = last_line_no
    with abs_path.open("rb") as handle:
        handle.seek(max(0, last_offset))
        while True:
            line_start = handle.tell()
            raw_line = handle.readline()
            if not raw_line:
                break
            line_no += 1
            line = raw_line.strip()
            if not line:
                continue
            try:
                obj = json.loads(line.decode("utf-8", errors="replace"))
            except Exception:
                continue
            if isinstance(obj, dict):
                _index_event(conn, obj, source_seq=source_seq, source_path=source_path, line_no=line_no, offset_bytes=line_start)
        last_offset = int(handle.tell())
    conn.execute(
        """
        INSERT INTO source_state(source_path, compressed, file_size, mtime_ns, last_offset_bytes, last_line_no)
        VALUES(?, ?, ?, ?, ?, ?)
        ON CONFLICT(source_path) DO UPDATE SET
            compressed=excluded.compressed,
            file_size=excluded.file_size,
            mtime_ns=excluded.mtime_ns,
            last_offset_bytes=excluded.last_offset_bytes,
            last_line_no=excluded.last_line_no
        """,
        (source_path, 0, size_bytes, mtime_ns, last_offset, line_no),
    )


def catch_up_ledger_index(ledger_path: Path) -> None:
    index_path = _index_path_for_ledger(ledger_path)
    conn = _connect(index_path)
    try:
        _ensure_schema(conn)
        sources = list_ledger_sources(ledger_path.parent)
        current_paths = {str(source.get("path") or "").strip() for source in sources}
        stale_rows = conn.execute("SELECT source_path FROM source_state").fetchall()
        for row in stale_rows:
            source_path = str(row[0] or "").strip()
            if source_path and source_path not in current_paths:
                _delete_source_rows(conn, source_path)

        for source in sources:
            source_path = str(source.get("path") or "").strip()
            abs_path = source.get("abs_path")
            compressed = bool(source.get("compressed"))
            if not source_path or not isinstance(abs_path, Path) or not abs_path.exists():
                continue
            row = conn.execute(
                "SELECT compressed, file_size, mtime_ns, last_offset_bytes, last_line_no FROM source_state WHERE source_path = ?",
                (source_path,),
            ).fetchone()
            size_bytes, mtime_ns = _source_stat(abs_path)
            if row is None:
                _reindex_source(conn, ledger_path, source)
                continue
            try:
                prev_compressed = bool(int(row[0] or 0))
                prev_size = int(row[1] or 0)
                prev_mtime_ns = int(row[2] or 0)
            except Exception:
                _reindex_source(conn, ledger_path, source)
                continue
            if compressed:
                if prev_compressed and prev_size == size_bytes and prev_mtime_ns == mtime_ns:
                    continue
                _reindex_source(conn, ledger_path, source)
                continue
            if prev_compressed:
                _reindex_source(conn, ledger_path, source)
                continue
            _catch_up_plain_source(conn, ledger_path, source)
        conn.commit()
    finally:
        conn.close()


def append_event_to_index(ledger_path: Path, event: Dict[str, Any], *, next_offset_bytes: int) -> None:
    index_path = _index_path_for_ledger(ledger_path)
    conn = _connect(index_path)
    try:
        _ensure_schema(conn)
        source_path = "ledger.jsonl"
        row = conn.execute(
            "SELECT last_line_no FROM source_state WHERE source_path = ?",
            (source_path,),
        ).fetchone()
        last_line_no = int(row[0] or 0) if row is not None else 0
        encoded = (json.dumps(event, ensure_ascii=False) + "\n").encode("utf-8", errors="replace")
        start_offset = max(0, int(next_offset_bytes or 0) - len(encoded))
        _index_event(
            conn,
            event,
            source_seq=ACTIVE_SOURCE_SEQ,
            source_path=source_path,
            line_no=last_line_no + 1,
            offset_bytes=start_offset,
        )
        size_bytes, mtime_ns = _source_stat(ledger_path)
        conn.execute(
            """
            INSERT INTO source_state(source_path, compressed, file_size, mtime_ns, last_offset_bytes, last_line_no)
            VALUES(?, ?, ?, ?, ?, ?)
            ON CONFLICT(source_path) DO UPDATE SET
                compressed=excluded.compressed,
                file_size=excluded.file_size,
                mtime_ns=excluded.mtime_ns,
                last_offset_bytes=excluded.last_offset_bytes,
                last_line_no=excluded.last_line_no
            """,
            (source_path, 0, size_bytes, mtime_ns, int(next_offset_bytes or 0), last_line_no + 1),
        )
        conn.commit()
    finally:
        conn.close()


def _read_event_from_source(group_path: Path, *, source_path: str, line_no: int, offset_bytes: int) -> Optional[Dict[str, Any]]:
    abs_path = group_path / source_path
    if not abs_path.exists():
        return None
    if str(abs_path.name).endswith(".gz"):
        current_line = 0
        for raw_line in iter_source_lines(abs_path):
            current_line += 1
            if current_line != max(1, int(line_no or 0)):
                continue
            try:
                obj = json.loads(raw_line)
            except Exception:
                return None
            return obj if isinstance(obj, dict) else None
        return None
    try:
        with abs_path.open("rb") as handle:
            handle.seek(max(0, int(offset_bytes or 0)))
            raw_line = handle.readline()
    except Exception:
        raw_line = b""
    if raw_line:
        try:
            obj = json.loads(raw_line.decode("utf-8", errors="replace"))
        except Exception:
            obj = None
        if isinstance(obj, dict):
            return obj
    current_line = 0
    with open_ledger_source_text(abs_path) as handle:
        for raw_line in handle:
            current_line += 1
            if current_line != max(1, int(line_no or 0)):
                continue
            try:
                obj = json.loads(raw_line)
            except Exception:
                return None
            return obj if isinstance(obj, dict) else None
    return None


def lookup_event_by_id(ledger_path: Path, event_id: str) -> Optional[Dict[str, Any]]:
    wanted = str(event_id or "").strip()
    if not wanted:
        return None
    catch_up_ledger_index(ledger_path)
    index_path = _index_path_for_ledger(ledger_path)
    conn = _connect(index_path)
    try:
        _ensure_schema(conn)
        row = conn.execute(
            "SELECT source_path, line_no, offset_bytes FROM events WHERE event_id = ?",
            (wanted,),
        ).fetchone()
        if row is None:
            return None
        source_path = str(row[0] or "").strip()
        line_no = int(row[1] or 0)
        offset_bytes = int(row[2] or 0)
    finally:
        conn.close()
    return _read_event_from_source(ledger_path.parent, source_path=source_path, line_no=line_no, offset_bytes=offset_bytes)


def lookup_event_with_chat_ack_indexed(
    ledger_path: Path,
    *,
    event_id: str,
    actor_id: str,
) -> tuple[Optional[Dict[str, Any]], bool]:
    wanted = str(event_id or "").strip()
    actor = str(actor_id or "").strip()
    if not wanted:
        return None, False

    catch_up_ledger_index(ledger_path)
    index_path = _index_path_for_ledger(ledger_path)
    conn = _connect(index_path)
    try:
        _ensure_schema(conn)
        event_row = conn.execute(
            "SELECT source_path, line_no, offset_bytes FROM events WHERE event_id = ?",
            (wanted,),
        ).fetchone()
        if actor:
            ack_row = conn.execute(
                "SELECT 1 FROM chat_ack WHERE event_id = ? AND actor_id = ? LIMIT 1",
                (wanted, actor),
            ).fetchone()
        else:
            ack_row = conn.execute(
                "SELECT 1 FROM chat_ack WHERE event_id = ? LIMIT 1",
                (wanted,),
            ).fetchone()
    finally:
        conn.close()

    found_ack = ack_row is not None
    if event_row is None:
        return None, found_ack
    source_path = str(event_row[0] or "").strip()
    line_no = int(event_row[1] or 0)
    offset_bytes = int(event_row[2] or 0)
    event = _read_event_from_source(ledger_path.parent, source_path=source_path, line_no=line_no, offset_bytes=offset_bytes)
    return event, found_ack


def lookup_events_by_ids(ledger_path: Path, event_ids: list[str]) -> list[Optional[Dict[str, Any]]]:
    wanted_ids = [str(event_id or "").strip() for event_id in event_ids]
    if not wanted_ids:
        return []

    unique_ids = [event_id for event_id in dict.fromkeys(wanted_ids) if event_id]
    if not unique_ids:
        return [None for _ in wanted_ids]

    catch_up_ledger_index(ledger_path)
    index_path = _index_path_for_ledger(ledger_path)
    conn = _connect(index_path)
    try:
        _ensure_schema(conn)
        placeholders = ", ".join("?" for _ in unique_ids)
        rows = conn.execute(
            f"SELECT event_id, source_path, line_no, offset_bytes FROM events WHERE event_id IN ({placeholders})",
            tuple(unique_ids),
        ).fetchall()
    finally:
        conn.close()

    found: dict[str, Optional[Dict[str, Any]]] = {}
    for row in rows:
        event_id = str(row[0] or "").strip()
        if not event_id:
            continue
        source_path = str(row[1] or "").strip()
        line_no = int(row[2] or 0)
        offset_bytes = int(row[3] or 0)
        found[event_id] = _read_event_from_source(
            ledger_path.parent,
            source_path=source_path,
            line_no=line_no,
            offset_bytes=offset_bytes,
        )
    return [found.get(event_id) if event_id else None for event_id in wanted_ids]


def has_chat_ack_indexed(ledger_path: Path, *, event_id: str, actor_id: str) -> bool:
    wanted = str(event_id or "").strip()
    actor = str(actor_id or "").strip()
    if not wanted:
        return False
    catch_up_ledger_index(ledger_path)
    index_path = _index_path_for_ledger(ledger_path)
    conn = _connect(index_path)
    try:
        _ensure_schema(conn)
        if actor:
            row = conn.execute(
                "SELECT 1 FROM chat_ack WHERE event_id = ? AND actor_id = ? LIMIT 1",
                (wanted, actor),
            ).fetchone()
        else:
            row = conn.execute(
                "SELECT 1 FROM chat_ack WHERE event_id = ? LIMIT 1",
                (wanted,),
            ).fetchone()
        return row is not None
    finally:
        conn.close()


def search_event_ids_indexed(
    ledger_path: Path,
    *,
    allowed_kinds: set[str],
    query: str = "",
    by_filter: str = "",
    before_id: str = "",
    after_id: str = "",
    limit: int = 50,
) -> tuple[list[str], bool]:
    catch_up_ledger_index(ledger_path)
    index_path = _index_path_for_ledger(ledger_path)
    conn = _connect(index_path)
    try:
        _ensure_schema(conn)
        params: list[Any] = []
        where: list[str] = []
        if allowed_kinds:
            where.append("kind IN (%s)" % ", ".join("?" for _ in allowed_kinds))
            params.extend(sorted(allowed_kinds))
        if by_filter:
            where.append("by_actor = ?")
            params.append(str(by_filter or "").strip())
        query_lower = str(query or "").strip().lower()
        join_sql = ""
        if query_lower:
            join_sql = "JOIN event_search es ON es.event_id = events.event_id"
            where.append("es.searchable_text LIKE ?")
            params.append(f"%{query_lower}%")

        anchor_id = str(before_id or after_id or "").strip()
        comparator = ""
        order_dir = "DESC"
        if anchor_id:
            anchor = conn.execute(
                "SELECT ts, source_seq, line_no FROM events WHERE event_id = ?",
                (anchor_id,),
            ).fetchone()
            if anchor is None:
                return [], False
            anchor_ts = str(anchor[0] or "").strip()
            anchor_seq = int(anchor[1] or 0)
            anchor_line = int(anchor[2] or 0)
            if before_id:
                comparator = "(ts < ? OR (ts = ? AND (source_seq < ? OR (source_seq = ? AND line_no < ?))))"
                params.extend([anchor_ts, anchor_ts, anchor_seq, anchor_seq, anchor_line])
                order_dir = "DESC"
            else:
                comparator = "(ts > ? OR (ts = ? AND (source_seq > ? OR (source_seq = ? AND line_no > ?))))"
                params.extend([anchor_ts, anchor_ts, anchor_seq, anchor_seq, anchor_line])
                order_dir = "ASC"
            where.append(comparator)

        where_sql = ("WHERE " + " AND ".join(where)) if where else ""
        sql = (
            f"SELECT events.event_id FROM events {join_sql} {where_sql} "
            f"ORDER BY events.ts {order_dir}, events.source_seq {order_dir}, events.line_no {order_dir} "
            "LIMIT ?"
        )
        query_params = [*params, max(1, int(limit or 50)) + 1]
        rows = conn.execute(sql, tuple(query_params)).fetchall()
        event_ids = [str(row[0] or "").strip() for row in rows if str(row[0] or "").strip()]
        has_more = len(event_ids) > max(1, int(limit or 50))
        event_ids = event_ids[: max(1, int(limit or 50))]
        if before_id:
            event_ids.reverse()
        return event_ids, has_more
    finally:
        conn.close()
