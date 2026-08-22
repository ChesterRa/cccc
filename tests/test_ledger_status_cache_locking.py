from __future__ import annotations

import sqlite3
import tempfile
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

from cccc.kernel import ledger_status_cache, ledger_status_db


class TestLedgerStatusCacheLocking(unittest.TestCase):
    def test_concurrent_first_use_initializes_schema_once(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "status.sqlite3"
            start = threading.Barrier(4)

            def initialize() -> int:
                start.wait(timeout=2)
                conn = ledger_status_db.connect_status_db(path)
                try:
                    ledger_status_db.ensure_status_schema(conn)
                    row = conn.execute(
                        "SELECT value FROM meta WHERE key = 'schema_version'"
                    ).fetchone()
                    return int(row[0]) if row else 0
                finally:
                    conn.close()

            with ThreadPoolExecutor(max_workers=4) as executor:
                versions = list(executor.map(lambda _: initialize(), range(4)))

            self.assertEqual(versions, [ledger_status_db.SCHEMA_VERSION] * 4)

    def test_current_schema_read_stays_available_during_write_lock(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            group = SimpleNamespace(path=Path(temp_dir), group_id="group-1")
            path = ledger_status_cache._status_index_path(group)
            conn = ledger_status_db.connect_status_db(path)
            try:
                ledger_status_db.ensure_status_schema(conn)
                conn.execute(
                    """
                    INSERT INTO message_status_meta(
                        event_id, ts, has_obligation, has_read_status
                    ) VALUES('event-1', '', 0, 1)
                    """
                )
                conn.commit()
                conn.execute("BEGIN IMMEDIATE")

                result = ledger_status_cache.get_cached_message_status_batch(
                    group, ["event-1"]
                )
            finally:
                conn.rollback()
                conn.close()

            self.assertEqual(result, {"event-1": {"read_status": {}}})

    def test_schema_migration_lock_failure_is_a_cache_miss(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            group = SimpleNamespace(path=Path(temp_dir), group_id="group-1")
            path = ledger_status_cache._status_index_path(group)
            conn = ledger_status_db.connect_status_db(path)
            try:
                ledger_status_db.ensure_status_schema(conn)
                conn.execute("UPDATE meta SET value = '1' WHERE key = 'schema_version'")
                conn.commit()
                conn.execute("BEGIN IMMEDIATE")

                with (
                    patch.object(ledger_status_db, "DEFAULT_TIMEOUT_SECONDS", 0.01),
                    self.assertLogs("cccc.ledger.status_cache", level="WARNING"),
                ):
                    result = ledger_status_cache.get_cached_message_status_batch(
                        group, ["event-1"]
                    )
            finally:
                conn.rollback()
                conn.close()

            self.assertEqual(result, {})

    def test_non_lock_operational_error_is_not_hidden(self) -> None:
        group = SimpleNamespace(path=Path("unused"), group_id="group-1")
        error = sqlite3.OperationalError("disk I/O error")

        with (
            patch.object(ledger_status_cache, "connect_status_db", side_effect=error),
            self.assertRaisesRegex(sqlite3.OperationalError, "disk I/O error"),
        ):
            ledger_status_cache.get_cached_message_status_batch(group, ["event-1"])


if __name__ == "__main__":
    unittest.main()
