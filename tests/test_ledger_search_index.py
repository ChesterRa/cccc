import os
import tempfile
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

            ledger_index.catch_up_ledger_index(group.ledger_path)
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
