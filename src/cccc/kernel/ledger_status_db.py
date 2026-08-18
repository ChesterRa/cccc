from __future__ import annotations

import sqlite3
import threading
import time
from pathlib import Path

SCHEMA_VERSION = 2
DEFAULT_TIMEOUT_SECONDS = 5.0

_SCHEMA_LOCK = threading.Lock()
_SCHEMA_STATEMENTS = (
    """
    CREATE TABLE IF NOT EXISTS meta (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    )
    """,
    """
    CREATE TABLE IF NOT EXISTS message_status_meta (
        event_id TEXT PRIMARY KEY,
        ts TEXT NOT NULL,
        is_attention INTEGER NOT NULL,
        has_obligation INTEGER NOT NULL
    )
    """,
    """
    CREATE TABLE IF NOT EXISTS recipient_status (
        event_id TEXT NOT NULL,
        actor_id TEXT NOT NULL,
        is_read INTEGER NOT NULL,
        is_acked INTEGER NOT NULL,
        is_replied INTEGER NOT NULL,
        reply_required INTEGER NOT NULL,
        PRIMARY KEY (event_id, actor_id)
    )
    """,
    "CREATE INDEX IF NOT EXISTS idx_message_status_meta_ts ON message_status_meta(ts, event_id)",
    "CREATE INDEX IF NOT EXISTS idx_recipient_status_event_id ON recipient_status(event_id)",
)


def is_database_busy_error(exc: sqlite3.OperationalError) -> bool:
    error_code = getattr(exc, "sqlite_errorcode", None)
    if isinstance(error_code, int) and error_code & 0xFF in {
        sqlite3.SQLITE_BUSY,
        sqlite3.SQLITE_LOCKED,
    }:
        return True
    message = str(exc).lower()
    return "locked" in message or "database is busy" in message


def _enable_wal(conn: sqlite3.Connection) -> None:
    deadline = time.monotonic() + DEFAULT_TIMEOUT_SECONDS
    while True:
        try:
            current = conn.execute("PRAGMA journal_mode").fetchone()
            if current and str(current[0]).lower() == "wal":
                return
            updated = conn.execute("PRAGMA journal_mode=WAL").fetchone()
            if updated and str(updated[0]).lower() == "wal":
                return
        except sqlite3.OperationalError as exc:
            if not is_database_busy_error(exc) or time.monotonic() >= deadline:
                raise
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise sqlite3.OperationalError("database is locked while enabling WAL")
        time.sleep(min(0.01, remaining))


def connect_status_db(path: Path) -> sqlite3.Connection:
    path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(path), timeout=DEFAULT_TIMEOUT_SECONDS)
    try:
        timeout_ms = max(0, int(DEFAULT_TIMEOUT_SECONDS * 1000))
        conn.execute(f"PRAGMA busy_timeout = {timeout_ms}")
        _enable_wal(conn)
        conn.execute("PRAGMA synchronous=NORMAL")
        conn.execute("PRAGMA temp_store=MEMORY")
        return conn
    except Exception:
        conn.close()
        raise


def _schema_version(conn: sqlite3.Connection) -> int:
    try:
        row = conn.execute(
            "SELECT value FROM meta WHERE key = ?", ("schema_version",)
        ).fetchone()
    except sqlite3.OperationalError as exc:
        if "no such table: meta" in str(exc).lower():
            return 0
        raise
    if row is None:
        return 0
    try:
        return int(row[0] or 0)
    except (TypeError, ValueError):
        return 0


def ensure_status_schema(conn: sqlite3.Connection) -> None:
    # Keep the common read path read-only. Schema work is needed only for a new
    # database or a version migration.
    if _schema_version(conn) == SCHEMA_VERSION:
        return

    with _SCHEMA_LOCK:
        if _schema_version(conn) == SCHEMA_VERSION:
            return
        try:
            # SQLite serializes this transaction across processes. Rechecking
            # the version inside it prevents two processes from migrating the
            # same database after one waiter acquires the lock.
            conn.execute("BEGIN IMMEDIATE")
            for statement in _SCHEMA_STATEMENTS:
                conn.execute(statement)
            if _schema_version(conn) != SCHEMA_VERSION:
                conn.execute("DELETE FROM recipient_status")
                conn.execute("DELETE FROM message_status_meta")
                conn.execute(
                    """
                    INSERT INTO meta(key, value) VALUES(?, ?)
                    ON CONFLICT(key) DO UPDATE SET value=excluded.value
                    """,
                    ("schema_version", str(SCHEMA_VERSION)),
                )
            conn.commit()
        except Exception:
            if conn.in_transaction:
                conn.rollback()
            raise
