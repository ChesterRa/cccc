import os
import sqlite3
import tempfile
import threading
import unittest
from unittest.mock import patch


class TestLedgerSearchIndex(unittest.TestCase):
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

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group_with_messages(self, title: str, count: int = 3) -> str:
        create, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        group_id = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)

        for idx in range(count):
            sent, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "text": f"{title} {idx}",
                    "by": "user",
                    "to": ["user"],
                },
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))
        return group_id

    def _corrupt_index_root_page(self, index_path, index_name: str) -> None:
        conn = sqlite3.connect(str(index_path))
        try:
            page_size = int(conn.execute("PRAGMA page_size").fetchone()[0])
            row = conn.execute(
                "SELECT rootpage FROM sqlite_master WHERE name = ?",
                (index_name,),
            ).fetchone()
            self.assertIsNotNone(row)
            conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        finally:
            conn.close()
        assert row is not None

        with index_path.open("r+b") as handle:
            handle.seek((int(row[0]) - 1) * page_size)
            handle.write(b"\x00")

    def test_catch_up_ledger_index_waits_for_index_lock(self) -> None:
        _, cleanup = self._with_home()
        lock_handle = None
        try:
            from cccc.kernel import ledger_index
            from cccc.kernel.group import load_group
            from cccc.util.file_lock import acquire_lockfile, release_lockfile

            group_id = self._create_group_with_messages("search-index-lock")
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            lock_path = group.path / "state" / "ledger" / "index.lock"
            lock_handle = acquire_lockfile(lock_path, blocking=True)
            attempted = threading.Event()
            finished = threading.Event()
            errors: list[BaseException] = []
            real_acquire = ledger_index.acquire_lockfile

            def observed_acquire(path, *, blocking: bool = True):
                attempted.set()
                return real_acquire(path, blocking=blocking)

            def run_catch_up() -> None:
                try:
                    ledger_index.catch_up_ledger_index(group.ledger_path)
                except BaseException as exc:  # pragma: no cover - surfaced by assertion below
                    errors.append(exc)
                finally:
                    finished.set()

            with patch.object(ledger_index, "acquire_lockfile", side_effect=observed_acquire):
                thread = threading.Thread(target=run_catch_up)
                thread.start()
                self.assertTrue(attempted.wait(timeout=1.0))
                self.assertFalse(finished.wait(timeout=0.15))
                release_lockfile(lock_handle)
                lock_handle = None
                thread.join(timeout=2.0)

            self.assertTrue(finished.is_set())
            self.assertEqual(errors, [])
        finally:
            if lock_handle is not None:
                from cccc.util.file_lock import release_lockfile

                release_lockfile(lock_handle)
            cleanup()

    def test_append_event_to_index_skips_when_index_lock_busy(self) -> None:
        _, cleanup = self._with_home()
        lock_handle = None
        try:
            from cccc.kernel import ledger_index
            from cccc.kernel.group import load_group
            from cccc.util.file_lock import acquire_lockfile, release_lockfile

            group_id = self._create_group_with_messages("append-index-lock", count=1)
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            from cccc.kernel.inbox import search_messages

            events, _ = search_messages(group, query="", kind_filter="chat", limit=1)
            self.assertTrue(events)
            indexed_event = events[-1]

            lock_path = group.path / "state" / "ledger" / "index.lock"
            lock_handle = acquire_lockfile(lock_path, blocking=True)

            with patch.object(ledger_index, "_connect", side_effect=AssertionError("busy append index update should skip")):
                ledger_index.append_event_to_index(
                    group.ledger_path,
                    indexed_event,
                    next_offset_bytes=group.ledger_path.stat().st_size,
                )

            release_lockfile(lock_handle)
            lock_handle = None
        finally:
            if lock_handle is not None:
                from cccc.util.file_lock import release_lockfile

                release_lockfile(lock_handle)
            cleanup()

    def test_search_rebuilds_corrupt_derived_index(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages

            group_id = self._create_group_with_messages("corrupt-search-index")
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            index_path = group.path / "state" / "ledger" / "index.sqlite3"
            with index_path.open("r+b") as handle:
                handle.write(b"x")

            events, has_more = search_messages(group, query="", kind_filter="chat", limit=10)

            self.assertFalse(has_more)
            self.assertEqual(
                [str((event.get("data") or {}).get("text") or "") for event in events],
                [f"corrupt-search-index {idx}" for idx in range(3)],
            )
            conn = sqlite3.connect(str(index_path))
            try:
                self.assertEqual(conn.execute("PRAGMA integrity_check").fetchone()[0], "ok")
            finally:
                conn.close()
        finally:
            cleanup()

    def test_search_rebuilds_when_corruption_is_only_reached_by_query(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages

            title = "query-page-corruption"
            group_id = self._create_group_with_messages(title, count=250)
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            index_path = group.path / "state" / "ledger" / "index.sqlite3"
            self._corrupt_index_root_page(index_path, "sqlite_autoindex_event_search_1")

            events, has_more = search_messages(group, query=title, kind_filter="chat", limit=10)

            self.assertTrue(has_more)
            self.assertEqual(len(events), 10)
            self.assertTrue(
                all(title in str((event.get("data") or {}).get("text") or "") for event in events)
            )
            conn = sqlite3.connect(str(index_path))
            try:
                self.assertEqual(conn.execute("PRAGMA integrity_check").fetchone()[0], "ok")
            finally:
                conn.close()
        finally:
            cleanup()

    def test_direct_lookups_rebuild_when_corruption_is_only_reached_by_query(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel import ledger_index
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages

            group_id = self._create_group_with_messages("lookup-page-corruption", count=3)
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            events, _ = search_messages(group, query="", kind_filter="chat", limit=10)
            event_id = str(events[0].get("id") or "")
            self.assertTrue(event_id)

            index_path = group.path / "state" / "ledger" / "index.sqlite3"
            self._corrupt_index_root_page(index_path, "sqlite_autoindex_events_1")

            event = ledger_index.lookup_event_by_id(group.ledger_path, event_id)

            self.assertIsNotNone(event)
            self.assertEqual(str((event or {}).get("id") or ""), event_id)
            self._corrupt_index_root_page(index_path, "sqlite_autoindex_events_1")

            batch = ledger_index.lookup_events_by_ids(group.ledger_path, [event_id])

            self.assertEqual(len(batch), 1)
            self.assertEqual(str((batch[0] or {}).get("id") or ""), event_id)
            conn = sqlite3.connect(str(index_path))
            try:
                self.assertEqual(conn.execute("PRAGMA integrity_check").fetchone()[0], "ok")
            finally:
                conn.close()
        finally:
            cleanup()

    def test_query_does_not_rebuild_for_non_corruption_error(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel import ledger_index
            from cccc.kernel.group import load_group

            group_id = self._create_group_with_messages("non-corrupt-query-error", count=1)
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            error = sqlite3.OperationalError("database is locked")

            def fail_query(_conn):
                raise error

            with patch.object(ledger_index, "_discard_index_files") as discard:
                with self.assertRaisesRegex(sqlite3.OperationalError, "database is locked"):
                    ledger_index._query_ledger_index(group.ledger_path, fail_query)

            discard.assert_not_called()
        finally:
            cleanup()

    def test_catch_up_does_not_discard_index_for_non_corruption_error(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel import ledger_index
            from cccc.kernel.group import load_group

            group_id = self._create_group_with_messages("non-corrupt-index-error", count=1)
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            index_path = group.path / "state" / "ledger" / "index.sqlite3"

            error = sqlite3.OperationalError("unable to open database file")
            with patch.object(ledger_index, "_connect", side_effect=error):
                with self.assertRaisesRegex(sqlite3.OperationalError, "unable to open database file"):
                    ledger_index.catch_up_ledger_index(group.ledger_path)

            self.assertTrue(index_path.exists())
        finally:
            cleanup()

    def test_rebuildable_index_error_supports_legacy_sqlite_exceptions(self) -> None:
        from cccc.kernel.ledger_index import _is_rebuildable_index_error

        self.assertTrue(_is_rebuildable_index_error(sqlite3.DatabaseError("file is not a database")))
        self.assertTrue(_is_rebuildable_index_error(sqlite3.DatabaseError("database disk image is malformed")))
        self.assertFalse(_is_rebuildable_index_error(sqlite3.OperationalError("unable to open database file")))

    def test_search_messages_without_query_uses_index_path(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages

            create, _ = self._call("group_create", {"title": "search-index", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            for idx in range(5):
                sent, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "text": f"hello {idx}",
                        "by": "user",
                        "to": ["user"],
                    },
                )
                self.assertTrue(sent.ok, getattr(sent, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            with patch("cccc.kernel.inbox.iter_events", side_effect=AssertionError("indexed search should avoid ledger scan")):
                events, has_more = search_messages(group, query="", kind_filter="all", limit=3)
            self.assertEqual(len(events), 3)
            self.assertTrue(has_more)
            self.assertEqual([str(ev.get("kind") or "") for ev in events], ["chat.message", "chat.message", "chat.message"])
        finally:
            cleanup()

    def test_search_messages_with_query_uses_indexed_text_path(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages

            create, _ = self._call("group_create", {"title": "search-index-query", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            for text in ("alpha hello", "beta world", "gamma hello world"):
                sent, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "text": text,
                        "by": "user",
                        "to": ["user"],
                    },
                )
                self.assertTrue(sent.ok, getattr(sent, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            with patch("cccc.kernel.inbox.iter_events", side_effect=AssertionError("indexed text search should avoid ledger scan")):
                events, has_more = search_messages(group, query="hello", kind_filter="all", limit=10)
            self.assertFalse(has_more)
            self.assertEqual(len(events), 2)
            texts = [str((ev.get("data") if isinstance(ev.get("data"), dict) else {}).get("text") or "") for ev in events]
            self.assertTrue(all("hello" in text.lower() for text in texts))
        finally:
            cleanup()

    def test_search_messages_indexes_insight_text_without_ledger_scan(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages

            create, _ = self._call("group_create", {"title": "search-insight", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            sent, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "text": "ordinary body",
                    "insight": "The rollback boundary remains unverified.",
                    "by": "user",
                    "to": ["user"],
                },
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            with patch("cccc.kernel.inbox.iter_events", side_effect=AssertionError("indexed search should avoid ledger scan")):
                events, has_more = search_messages(group, query="rollback", kind_filter="all", limit=10)

            self.assertFalse(has_more)
            self.assertEqual(len(events), 1)
            self.assertEqual((events[0].get("data") or {}).get("insight"), "The rollback boundary remains unverified.")
        finally:
            cleanup()

    def test_search_messages_avoids_per_event_lookup_round_trips(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages

            create, _ = self._call("group_create", {"title": "search-batch-lookup", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            for idx in range(6):
                sent, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "text": f"batch {idx}",
                        "by": "user",
                        "to": ["user"],
                    },
                )
                self.assertTrue(sent.ok, getattr(sent, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            with patch("cccc.kernel.inbox.lookup_event_by_id", side_effect=AssertionError("search should use batched event lookup")):
                events, has_more = search_messages(group, query="", kind_filter="all", limit=4)
            self.assertEqual(len(events), 4)
            self.assertTrue(has_more)
        finally:
            cleanup()

    def test_search_messages_repairs_stale_plain_source_index_bounds(self) -> None:
        _, cleanup = self._with_home()
        try:
            import sqlite3

            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages
            from cccc.kernel.ledger_index import catch_up_ledger_index

            create, _ = self._call("group_create", {"title": "search-repair", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            for idx in range(12):
                sent, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "text": f"repair {idx}",
                        "by": "user",
                        "to": ["user"],
                    },
                )
                self.assertTrue(sent.ok, getattr(sent, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            catch_up_ledger_index(group.ledger_path)

            index_path = group.path / "state" / "ledger" / "index.sqlite3"
            file_size = group.ledger_path.stat().st_size
            conn = sqlite3.connect(str(index_path))
            try:
                conn.execute("DELETE FROM events WHERE source_path = 'ledger.jsonl' AND line_no > 3")
                conn.execute("DELETE FROM event_search WHERE event_id NOT IN (SELECT event_id FROM events)")
                conn.execute("UPDATE events SET line_no = line_no + 20 WHERE source_path = 'ledger.jsonl'")
                conn.execute("UPDATE source_state SET file_size = ?, last_offset_bytes = ?, last_line_no = 3 WHERE source_path = 'ledger.jsonl'", (file_size, file_size))
                conn.commit()
            finally:
                conn.close()

            events, has_more = search_messages(group, query="", kind_filter="chat", limit=20)

            self.assertFalse(has_more)
            self.assertEqual(len(events), 12)
            self.assertEqual(
                [str((event.get("data") or {}).get("text") or "") for event in events],
                [f"repair {idx}" for idx in range(12)],
            )
        finally:
            cleanup()

    def test_search_messages_default_tail_preserves_chronological_order_for_history_paging(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import search_messages

            create, _ = self._call("group_create", {"title": "search-tail-order", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            for idx in range(6):
                sent, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "text": f"ordered {idx}",
                        "by": "user",
                        "to": ["user"],
                    },
                )
                self.assertTrue(sent.ok, getattr(sent, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            events, has_more = search_messages(group, query="", kind_filter="chat", limit=3)

            texts = [
                str((ev.get("data") if isinstance(ev.get("data"), dict) else {}).get("text") or "")
                for ev in events
            ]
            self.assertEqual(texts, ["ordered 3", "ordered 4", "ordered 5"])
            self.assertTrue(has_more)

            older, older_has_more = search_messages(
                group,
                query="",
                kind_filter="chat",
                before_id=str(events[0].get("id") or ""),
                limit=3,
            )
            older_texts = [
                str((ev.get("data") if isinstance(ev.get("data"), dict) else {}).get("text") or "")
                for ev in older
            ]
            self.assertEqual(older_texts, ["ordered 0", "ordered 1", "ordered 2"])
            self.assertFalse(older_has_more)
        finally:
            cleanup()

    def test_lookup_events_by_ids_batches_compressed_source_reads(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.contracts.v1 import ChatMessageData
            from cccc.kernel import ledger_index
            from cccc.kernel.group import create_group
            from cccc.kernel.ledger import append_event
            from cccc.kernel.ledger_segments import compress_sealed_segments, rotate_active_ledger
            from cccc.kernel.registry import load_registry

            reg = load_registry()
            group = create_group(reg, title="compressed-lookup")
            event_ids: list[str] = []
            for idx in range(30):
                event = append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data=ChatMessageData(text=f"gz {idx}", to=["user"]).model_dump(),
                )
                event_ids.append(str(event.get("id") or ""))

            rotation = rotate_active_ledger(group.path, reason="test")
            self.assertTrue(rotation.get("rotated"))
            compressed = compress_sealed_segments(group.path, keep_recent=0, force=True)
            self.assertEqual(int(compressed.get("count") or 0), 1)
            active_event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data=ChatMessageData(text="active tail", to=["user"]).model_dump(),
            )

            ledger_index.catch_up_ledger_index(group.ledger_path)
            positions = ledger_index.lookup_event_positions(
                group.ledger_path,
                [event_ids[3], event_ids[17], str(active_event.get("id") or ""), "missing"],
            )
            self.assertIsNotNone(positions[0])
            self.assertIsNotNone(positions[1])
            self.assertIsNotNone(positions[2])
            assert positions[0] is not None and positions[1] is not None and positions[2] is not None
            self.assertLess(positions[0], positions[1])
            self.assertLess(positions[1], positions[2])
            self.assertIsNone(positions[3])
            wanted = [event_ids[3], event_ids[17], event_ids[7], event_ids[29]]
            with patch.object(ledger_index, "iter_source_lines", wraps=ledger_index.iter_source_lines) as iter_source_lines:
                events = ledger_index.lookup_events_by_ids(group.ledger_path, wanted)

            self.assertEqual([str((ev or {}).get("id") or "") for ev in events], wanted)
            self.assertEqual(iter_source_lines.call_count, 1)
        finally:
            cleanup()

    def test_chat_ack_index_survives_target_message_compression(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.contracts.v1 import ChatMessageData
            from cccc.kernel import ledger_index
            from cccc.kernel.group import create_group
            from cccc.kernel.ledger import append_event
            from cccc.kernel.ledger_segments import compress_sealed_segments, rotate_active_ledger
            from cccc.kernel.registry import load_registry

            reg = load_registry()
            group = create_group(reg, title="compressed-ack-index")
            msg = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data=ChatMessageData(
                    text="ack after rotate",
                    to=["peer1"],
                    priority="attention",
                    reply_required=True,
                ).model_dump(),
            )
            msg_id = str(msg.get("id") or "")
            self.assertTrue(msg_id)

            rotation = rotate_active_ledger(group.path, reason="test")
            self.assertTrue(rotation.get("rotated"))
            ledger_index.catch_up_ledger_index(group.ledger_path)

            append_event(
                group.ledger_path,
                kind="chat.ack",
                group_id=group.group_id,
                scope_key="",
                by="peer1",
                data={"actor_id": "peer1", "event_id": msg_id},
            )

            compressed = compress_sealed_segments(group.path, keep_recent=0, force=True)
            self.assertEqual(int(compressed.get("count") or 0), 1)

            acks = ledger_index.lookup_chat_ack_actor_ids(group.ledger_path, {msg_id})

            self.assertEqual(acks, {msg_id: {"peer1"}})
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
