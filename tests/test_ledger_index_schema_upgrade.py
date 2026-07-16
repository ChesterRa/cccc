import os
import sqlite3
import tempfile
import unittest
from pathlib import Path


class TestLedgerIndexSchemaUpgrade(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return Path(td), cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_search_rebuilds_legacy_index_with_incompatible_events_schema(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages

            create, _ = self._call("group_create", {"title": "legacy-index", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            sent, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "text": "legacy hello",
                    "by": "user",
                    "to": ["user"],
                },
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            index_path = group.path / "state" / "ledger" / "index.sqlite3"
            index_path.parent.mkdir(parents=True, exist_ok=True)
            conn = sqlite3.connect(str(index_path))
            try:
                conn.executescript(
                    """
                    DROP TABLE IF EXISTS meta;
                    DROP TABLE IF EXISTS events;
                    DROP TABLE IF EXISTS chat_ack;
                    DROP TABLE IF EXISTS source_state;
                    DROP TABLE IF EXISTS event_search;
                    DROP INDEX IF EXISTS idx_events_kind_ts;
                    DROP INDEX IF EXISTS idx_events_by_ts;
                    DROP INDEX IF EXISTS idx_events_reply_to;
                    DROP INDEX IF EXISTS idx_events_ts;
                    DROP INDEX IF EXISTS idx_events_source_line;

                    CREATE TABLE meta (
                        key TEXT PRIMARY KEY,
                        value TEXT NOT NULL
                    );

                    CREATE TABLE events (
                        event_id TEXT PRIMARY KEY,
                        ts TEXT NOT NULL,
                        kind TEXT NOT NULL,
                        by_actor TEXT NOT NULL,
                        reply_to TEXT NOT NULL,
                        offset_bytes INTEGER NOT NULL,
                        source_seq INTEGER NOT NULL DEFAULT 0
                    );

                    CREATE TABLE chat_ack (
                        event_id TEXT NOT NULL,
                        actor_id TEXT NOT NULL,
                        PRIMARY KEY (event_id, actor_id)
                    );

                    CREATE TABLE source_state (
                        source_path TEXT PRIMARY KEY,
                        compressed INTEGER NOT NULL,
                        file_size INTEGER NOT NULL,
                        mtime_ns INTEGER NOT NULL,
                        last_offset_bytes INTEGER NOT NULL,
                        last_line_no INTEGER NOT NULL
                    );

                    CREATE TABLE event_search (
                        event_id TEXT PRIMARY KEY,
                        searchable_text TEXT NOT NULL
                    );

                    INSERT INTO meta(key, value) VALUES ('schema_version', '1');
                    """
                )
                conn.commit()
            finally:
                conn.close()

            events, has_more = search_messages(group, query="legacy", kind_filter="all", limit=10)
            self.assertFalse(has_more)
            self.assertEqual(len(events), 1)
            self.assertEqual(
                str((events[0].get("data") if isinstance(events[0].get("data"), dict) else {}).get("text") or ""),
                "legacy hello",
            )

            conn = sqlite3.connect(str(index_path))
            try:
                columns = [str(row[1] or "") for row in conn.execute("PRAGMA table_info(events)").fetchall()]
                self.assertIn("source_seq", columns)
                ack_columns = [str(row[1] or "") for row in conn.execute("PRAGMA table_info(chat_ack)").fetchall()]
                self.assertIn("ack_event_id", ack_columns)
                self.assertIn("source_path", ack_columns)
                schema_row = conn.execute("SELECT value FROM meta WHERE key = 'schema_version'").fetchone()
                self.assertIsNotNone(schema_row)
                self.assertEqual(str(schema_row[0] or ""), "5")
            finally:
                conn.close()
        finally:
            cleanup()

    def test_schema_ensure_does_not_rebuild_indexes_when_current(self) -> None:
        from cccc.kernel import ledger_index

        conn = sqlite3.connect(":memory:")
        statements: list[str] = []
        try:
            ledger_index._ensure_schema(conn)
            conn.commit()
            conn.set_trace_callback(statements.append)
            ledger_index._ensure_schema(conn)
            conn.commit()
        finally:
            conn.close()

        dropped_indexes = [stmt for stmt in statements if stmt.strip().upper().startswith("DROP INDEX")]
        self.assertEqual(dropped_indexes, [])


if __name__ == "__main__":
    unittest.main()
