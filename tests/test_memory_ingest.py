"""Tests for memory_ingest (T098) - chat ingest + signal/raw modes."""

import json
import os
import pathlib
import tempfile
import unittest
from unittest.mock import patch, MagicMock

from cccc.kernel.memory import MEMORY_KINDS
from cccc.daemon.memory.memory_ops import (
    handle_memory_ingest,
    handle_memory_search,
    _parse_chat_events,
    _filter_after_watermark,
    _extract_key_phrases,
    _suggest_kind,
    _ingest_signal,
    _WATERMARK_META_KEY,
    _get_memory_store,
    close_all_stores,
)


def _make_chat_event(event_id: str, by: str, text: str, ts: str = "2026-02-25T00:00:00Z") -> dict:
    """Create a minimal chat.message event."""
    return {
        "v": 1,
        "id": event_id,
        "ts": ts,
        "kind": "chat.message",
        "group_id": "g_test",
        "by": by,
        "data": {"text": text, "format": "plain"},
    }


def _make_non_chat_event(event_id: str, kind: str = "context.sync") -> dict:
    return {"v": 1, "id": event_id, "ts": "2026-02-25T00:00:00Z", "kind": kind, "group_id": "g_test", "by": "system", "data": {}}


class TestParseChatEvents(unittest.TestCase):
    """Test _parse_chat_events helper."""

    def test_parses_chat_messages(self):
        lines = [
            json.dumps(_make_chat_event("e1", "alice", "hello")),
            json.dumps(_make_chat_event("e2", "bob", "world")),
        ]
        events = _parse_chat_events(lines)
        self.assertEqual(len(events), 2)
        self.assertEqual(events[0]["id"], "e1")
        self.assertEqual(events[1]["id"], "e2")

    def test_filters_non_chat_events(self):
        lines = [
            json.dumps(_make_chat_event("e1", "alice", "hello")),
            json.dumps(_make_non_chat_event("e2")),
            json.dumps(_make_chat_event("e3", "bob", "world")),
        ]
        events = _parse_chat_events(lines)
        self.assertEqual(len(events), 2)

    def test_handles_invalid_json(self):
        lines = [
            json.dumps(_make_chat_event("e1", "alice", "hello")),
            "not valid json",
            "",
            json.dumps(_make_chat_event("e2", "bob", "world")),
        ]
        events = _parse_chat_events(lines)
        self.assertEqual(len(events), 2)

    def test_empty_lines(self):
        events = _parse_chat_events([])
        self.assertEqual(events, [])


class TestFilterAfterWatermark(unittest.TestCase):
    """Test _filter_after_watermark helper."""

    def test_no_watermark_returns_all(self):
        events = [{"id": "e1"}, {"id": "e2"}, {"id": "e3"}]
        result, found = _filter_after_watermark(events, "")
        self.assertEqual(len(result), 3)
        self.assertTrue(found)

    def test_watermark_filters_before(self):
        events = [{"id": "e1"}, {"id": "e2"}, {"id": "e3"}]
        result, found = _filter_after_watermark(events, "e1")
        self.assertEqual(len(result), 2)
        self.assertEqual(result[0]["id"], "e2")
        self.assertEqual(result[1]["id"], "e3")
        self.assertTrue(found)

    def test_watermark_at_end_returns_empty(self):
        events = [{"id": "e1"}, {"id": "e2"}, {"id": "e3"}]
        result, found = _filter_after_watermark(events, "e3")
        self.assertEqual(len(result), 0)
        self.assertTrue(found)

    def test_watermark_not_found_returns_all(self):
        """Stale watermark: return all events, found=False."""
        events = [{"id": "e1"}, {"id": "e2"}]
        result, found = _filter_after_watermark(events, "stale_id")
        self.assertEqual(len(result), 2)
        self.assertFalse(found)

    def test_watermark_middle(self):
        events = [{"id": "e1"}, {"id": "e2"}, {"id": "e3"}, {"id": "e4"}]
        result, found = _filter_after_watermark(events, "e2")
        self.assertEqual(len(result), 2)
        self.assertEqual(result[0]["id"], "e3")
        self.assertTrue(found)


class TestExtractKeyPhrases(unittest.TestCase):
    """Test _extract_key_phrases helper."""

    def test_extracts_frequent_words(self):
        text = "SQLite database performance SQLite indexing database optimization"
        phrases = _extract_key_phrases(text)
        self.assertIn("sqlite", phrases)
        self.assertIn("database", phrases)

    def test_filters_stop_words(self):
        text = "the and for are but not you all can had"
        phrases = _extract_key_phrases(text)
        self.assertEqual(phrases, [])

    def test_cjk_extraction(self):
        text = "记忆系统使用SQLite进行存储，记忆系统性能优异"
        phrases = _extract_key_phrases(text)
        # CJK tokens are extracted as continuous runs; at least some phrases found
        self.assertGreater(len(phrases), 0)
        # The repeated "记忆系统" substring should cause a phrase containing it
        found = any("记忆系统" in p for p in phrases)
        self.assertTrue(found, f"Expected phrase containing '记忆系统', got: {phrases}")

    def test_max_phrases_limit(self):
        text = " ".join(f"word{i}" for i in range(20))
        phrases = _extract_key_phrases(text, max_phrases=3)
        self.assertLessEqual(len(phrases), 3)

    def test_empty_text(self):
        phrases = _extract_key_phrases("")
        self.assertEqual(phrases, [])


class TestSuggestKind(unittest.TestCase):
    """Test _suggest_kind helper."""

    def test_decision_keywords(self):
        self.assertEqual(_suggest_kind("We decided to use FTS5"), "decision")
        self.assertEqual(_suggest_kind("决定采用 SQLite"), "decision")

    def test_instruction_keywords(self):
        """Plan-like words map to 'instruction' (valid MEMORY_KINDS)."""
        self.assertEqual(_suggest_kind("The plan is to refactor"), "instruction")
        self.assertEqual(_suggest_kind("下一步需要实现搜索"), "instruction")

    def test_fact_keywords(self):
        self.assertEqual(_suggest_kind("We discovered a bug"), "fact")
        self.assertEqual(_suggest_kind("确认该方案可行"), "fact")

    def test_default_observation(self):
        self.assertEqual(_suggest_kind("just a regular message"), "observation")

    def test_all_returned_kinds_are_valid(self):
        """T101: _suggest_kind always returns a value in MEMORY_KINDS."""
        test_texts = [
            "decided to use FTS5",
            "The plan is to refactor the code",
            "found a critical bug",
            "just chatting about things",
            "we agreed on the approach",
            "next step is deployment",
            "confirmed the fix works",
            "",
        ]
        for text in test_texts:
            kind = _suggest_kind(text)
            self.assertIn(kind, MEMORY_KINDS, f"_suggest_kind({text!r}) returned {kind!r} not in MEMORY_KINDS")


class TestIngestSignal(unittest.TestCase):
    """Test _ingest_signal helper."""

    def test_empty_events(self):
        result = _ingest_signal([])
        self.assertEqual(result["signals"], [])
        self.assertEqual(result["events_processed"], 0)

    def test_groups_by_actor(self):
        events = [
            _make_chat_event("e1", "alice", "message one", "2026-02-25T00:00:00Z"),
            _make_chat_event("e2", "bob", "message two", "2026-02-25T00:01:00Z"),
            _make_chat_event("e3", "alice", "message three", "2026-02-25T00:02:00Z"),
        ]
        result = _ingest_signal(events)
        self.assertEqual(result["events_processed"], 3)
        actors = {s["actor_id"] for s in result["signals"]}
        self.assertEqual(actors, {"alice", "bob"})

    def test_signal_structure(self):
        events = [_make_chat_event("e1", "alice", "We decided to use SQLite for search")]
        result = _ingest_signal(events)
        self.assertEqual(len(result["signals"]), 1)
        signal = result["signals"][0]
        self.assertEqual(signal["actor_id"], "alice")
        self.assertEqual(signal["messages_count"], 1)
        self.assertIn("suggested_kind", signal)
        self.assertIn("key_phrases", signal)
        self.assertIn("time_range", signal)
        self.assertIn("topic", signal)

    def test_time_range(self):
        events = [
            _make_chat_event("e1", "alice", "first", "2026-02-25T00:00:00Z"),
            _make_chat_event("e2", "alice", "last", "2026-02-25T01:00:00Z"),
        ]
        result = _ingest_signal(events)
        signal = result["signals"][0]
        self.assertEqual(signal["time_range"]["first"], "2026-02-25T00:00:00Z")
        self.assertEqual(signal["time_range"]["last"], "2026-02-25T01:00:00Z")

    def test_skips_empty_text(self):
        events = [_make_chat_event("e1", "alice", "")]
        result = _ingest_signal(events)
        self.assertEqual(len(result["signals"]), 0)


class IngestOpsTestBase(unittest.TestCase):
    """Base with temp group + ledger for ingest tests."""

    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.group_id = "g_ingest_test"
        self.group_path = pathlib.Path(self._td.name)

        # Patch load_group
        self._patcher = patch("cccc.daemon.memory.memory_ops.load_group")
        self.mock_load_group = self._patcher.start()
        mock_group = MagicMock()
        mock_group.path = self.group_path
        mock_group.ledger_path = self.group_path / "ledger.jsonl"
        self.mock_load_group.return_value = mock_group

        # Clear watermarks by closing stores (watermark now persisted in meta)
        close_all_stores()

    def tearDown(self):
        close_all_stores()
        self._patcher.stop()
        self._td.cleanup()

    def _write_ledger(self, events: list):
        """Write events to the test ledger."""
        ledger = self.group_path / "ledger.jsonl"
        with open(ledger, "w", encoding="utf-8") as f:
            for ev in events:
                f.write(json.dumps(ev, ensure_ascii=False) + "\n")


class TestHandleMemoryIngestValidation(IngestOpsTestBase):
    """Test input validation."""

    def test_missing_group_id(self):
        resp = handle_memory_ingest({})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "missing_group_id")

    def test_group_not_found(self):
        self.mock_load_group.return_value = None
        resp = handle_memory_ingest({"group_id": "unknown"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "group_not_found")

    def test_invalid_mode(self):
        self._write_ledger([])
        resp = handle_memory_ingest({"group_id": self.group_id, "mode": "invalid"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "invalid_mode")


class TestHandleMemoryIngestSignal(IngestOpsTestBase):
    """Test signal mode."""

    def test_signal_empty_ledger(self):
        self._write_ledger([])
        resp = handle_memory_ingest({"group_id": self.group_id, "mode": "signal"})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["signals"], [])
        self.assertEqual(resp.result["mode"], "signal")

    def test_signal_default_mode(self):
        """Default mode is signal."""
        self._write_ledger([_make_chat_event("e1", "alice", "hello")])
        resp = handle_memory_ingest({"group_id": self.group_id})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["mode"], "signal")

    def test_signal_with_events(self):
        events = [
            _make_chat_event("e1", "alice", "We decided to use FTS5 for search"),
            _make_chat_event("e2", "bob", "Good plan, let me implement it"),
            _make_chat_event("e3", "alice", "The decision was approved by the team"),
        ]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id, "mode": "signal"})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["events_processed"], 3)
        self.assertGreater(len(resp.result["signals"]), 0)

    def test_signal_returns_watermark(self):
        events = [_make_chat_event("e1", "alice", "hello")]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["watermark"], "e1")

    def test_signal_filters_non_chat(self):
        events = [
            _make_chat_event("e1", "alice", "hello"),
            _make_non_chat_event("e2"),
            _make_chat_event("e3", "bob", "world"),
        ]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["events_processed"], 2)


class TestHandleMemoryIngestRaw(IngestOpsTestBase):
    """Test raw mode."""

    def test_raw_empty_ledger(self):
        self._write_ledger([])
        resp = handle_memory_ingest({"group_id": self.group_id, "mode": "raw"})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["imported"], 0)
        self.assertEqual(resp.result["skipped"], 0)
        self.assertEqual(resp.result["mode"], "raw")

    def test_raw_imports_messages(self):
        events = [
            _make_chat_event("e1", "alice", "important observation about architecture"),
            _make_chat_event("e2", "bob", "another useful message"),
        ]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id, "mode": "raw"})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["imported"], 2)
        self.assertEqual(resp.result["skipped"], 0)

    def test_raw_different_source_ref_not_deduped(self):
        """Different source_ref (event_id) + same content = separate records."""
        events = [
            _make_chat_event("e1", "alice", "same message"),
            _make_chat_event("e2", "alice", "same message"),
        ]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id, "mode": "raw"})
        self.assertTrue(resp.ok)
        # Different event_ids → different source_refs → both imported
        self.assertEqual(resp.result["imported"], 2)
        self.assertEqual(resp.result["skipped"], 0)

    def test_raw_same_source_ref_idempotent(self):
        """Same source_ref (event_id) + same content = idempotent (deduped)."""
        events = [_make_chat_event("e1", "alice", "same message")]
        self._write_ledger(events)

        # First ingest
        resp1 = handle_memory_ingest({"group_id": self.group_id, "mode": "raw", "reset_watermark": True})
        self.assertTrue(resp1.ok)
        self.assertEqual(resp1.result["imported"], 1)

        # Second ingest with reset_watermark to re-process the same event
        resp2 = handle_memory_ingest({"group_id": self.group_id, "mode": "raw", "reset_watermark": True})
        self.assertTrue(resp2.ok)
        # Same event_id e1 → same source_ref → deduped
        self.assertEqual(resp2.result["imported"], 0)
        self.assertEqual(resp2.result["skipped"], 1)

    def test_raw_actor_filter(self):
        events = [
            _make_chat_event("e1", "alice", "alice message"),
            _make_chat_event("e2", "bob", "bob message"),
            _make_chat_event("e3", "alice", "another alice message"),
        ]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id, "mode": "raw", "actor_id": "alice"})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["imported"], 2)
        self.assertEqual(resp.result["skipped"], 1)

    def test_raw_skips_empty_text(self):
        events = [
            _make_chat_event("e1", "alice", ""),
            _make_chat_event("e2", "bob", "valid message"),
        ]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id, "mode": "raw"})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["imported"], 1)
        self.assertEqual(resp.result["skipped"], 1)

    def test_raw_memories_searchable(self):
        """Imported memories are searchable via memory_search."""
        events = [_make_chat_event("e1", "alice", "SQLite FTS5 search performance")]
        self._write_ledger(events)
        handle_memory_ingest({"group_id": self.group_id, "mode": "raw"})

        search_resp = handle_memory_search({"group_id": self.group_id, "query": "SQLite"})
        self.assertTrue(search_resp.ok)
        self.assertGreater(search_resp.result["count"], 0)

    def test_raw_source_type_and_ref(self):
        """Imported memories have correct source_type and source_ref."""
        events = [_make_chat_event("e1", "alice", "test memory")]
        self._write_ledger(events)
        handle_memory_ingest({"group_id": self.group_id, "mode": "raw"})

        search_resp = handle_memory_search({"group_id": self.group_id})
        self.assertTrue(search_resp.ok)
        mem = search_resp.result["memories"][0]
        self.assertEqual(mem["source_type"], "chat_ingest")
        self.assertEqual(mem["source_ref"], "e1")
        self.assertEqual(mem["actor_id"], "alice")


class TestWatermark(IngestOpsTestBase):
    """Test watermark (last_ingest_event_id) behavior."""

    def test_watermark_skips_processed(self):
        events = [
            _make_chat_event("e1", "alice", "first message"),
            _make_chat_event("e2", "bob", "second message"),
            _make_chat_event("e3", "alice", "third message"),
        ]
        self._write_ledger(events)

        # First ingest: processes all 3
        resp1 = handle_memory_ingest({"group_id": self.group_id, "mode": "signal"})
        self.assertTrue(resp1.ok)
        self.assertEqual(resp1.result["events_processed"], 3)
        self.assertEqual(resp1.result["watermark"], "e3")

        # Second ingest: watermark at e3, nothing new
        resp2 = handle_memory_ingest({"group_id": self.group_id, "mode": "signal"})
        self.assertTrue(resp2.ok)
        self.assertEqual(resp2.result["events_processed"], 0)

    def test_watermark_incremental(self):
        """New events after watermark are processed."""
        events = [_make_chat_event("e1", "alice", "first")]
        self._write_ledger(events)

        resp1 = handle_memory_ingest({"group_id": self.group_id})
        self.assertEqual(resp1.result["watermark"], "e1")

        # Add more events
        events.append(_make_chat_event("e2", "bob", "second"))
        events.append(_make_chat_event("e3", "alice", "third"))
        self._write_ledger(events)

        resp2 = handle_memory_ingest({"group_id": self.group_id})
        self.assertTrue(resp2.ok)
        self.assertEqual(resp2.result["events_processed"], 2)
        self.assertEqual(resp2.result["watermark"], "e3")

    def test_reset_watermark(self):
        events = [_make_chat_event("e1", "alice", "hello")]
        self._write_ledger(events)

        handle_memory_ingest({"group_id": self.group_id})
        store = _get_memory_store(self.group_id)
        self.assertIsNotNone(store.get_meta(_WATERMARK_META_KEY))

        # Reset and re-process
        resp = handle_memory_ingest({"group_id": self.group_id, "reset_watermark": True})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["events_processed"], 1)

    def test_watermark_persists_across_modes(self):
        """Watermark set in signal mode is respected in raw mode."""
        events = [
            _make_chat_event("e1", "alice", "first message"),
            _make_chat_event("e2", "bob", "second message"),
        ]
        self._write_ledger(events)

        # Signal mode sets watermark
        resp1 = handle_memory_ingest({"group_id": self.group_id, "mode": "signal"})
        self.assertEqual(resp1.result["watermark"], "e2")

        # Raw mode: nothing new
        resp2 = handle_memory_ingest({"group_id": self.group_id, "mode": "raw"})
        self.assertEqual(resp2.result["imported"], 0)

    def test_watermark_per_group(self):
        """Watermarks are per-group."""
        events = [_make_chat_event("e1", "alice", "hello")]
        self._write_ledger(events)

        handle_memory_ingest({"group_id": self.group_id})
        store = _get_memory_store(self.group_id)
        self.assertEqual(store.get_meta(_WATERMARK_META_KEY), "e1")


class TestIngestLimit(IngestOpsTestBase):
    """Test limit parameter."""

    def test_default_limit(self):
        events = [_make_chat_event(f"e{i}", "alice", f"message {i}") for i in range(60)]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id})
        self.assertTrue(resp.ok)
        # Default limit is 50 ledger lines, all are chat events
        self.assertLessEqual(resp.result["events_processed"], 50)

    def test_custom_limit(self):
        events = [_make_chat_event(f"e{i}", "alice", f"message {i}") for i in range(20)]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id, "limit": 5})
        self.assertTrue(resp.ok)
        # Only last 5 lines read
        self.assertLessEqual(resp.result["events_processed"], 5)


class TestIngestStaleWatermark(IngestOpsTestBase):
    """T100: Test that stale watermark does not skip messages."""

    def test_stale_watermark_auto_expands_window(self):
        """When watermark is stale in current window, ingest auto-expands and advances safely."""
        # Write initial events and ingest
        events = [_make_chat_event(f"e{i}", "alice", f"msg {i}") for i in range(5)]
        self._write_ledger(events)
        resp1 = handle_memory_ingest({"group_id": self.group_id, "limit": 5})
        self.assertEqual(resp1.result["watermark"], "e4")

        # Now write 10 new events (e5-e14), but only read last 5 (e10-e14).
        # Watermark e4 is NOT in the last 5 lines => stale.
        all_events = events + [_make_chat_event(f"e{i}", "bob", f"msg {i}") for i in range(5, 15)]
        self._write_ledger(all_events)

        resp2 = handle_memory_ingest({"group_id": self.group_id, "limit": 5})
        self.assertTrue(resp2.ok)
        # Auto expansion finds e4 and processes all unseen events (e5-e14)
        self.assertEqual(resp2.result["events_processed"], 10)
        self.assertEqual(resp2.result["watermark"], "e14")
        self.assertFalse(resp2.result.get("watermark_stale", False))

    def test_stale_watermark_recovers_with_larger_limit(self):
        """Stale watermark can be recovered by increasing limit."""
        events = [_make_chat_event(f"e{i}", "alice", f"msg {i}") for i in range(5)]
        self._write_ledger(events)
        handle_memory_ingest({"group_id": self.group_id, "limit": 5})
        # watermark is now e4

        # Add 10 new events
        all_events = events + [_make_chat_event(f"e{i}", "bob", f"msg {i}") for i in range(5, 15)]
        self._write_ledger(all_events)

        # With larger limit, watermark e4 is found in the window
        resp = handle_memory_ingest({"group_id": self.group_id, "limit": 15})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["events_processed"], 10)
        self.assertEqual(resp.result["watermark"], "e14")
        self.assertFalse(resp.result.get("watermark_stale", False))

    def test_stale_watermark_reset_recovers(self):
        """Reset watermark allows processing from scratch."""
        events = [_make_chat_event(f"e{i}", "alice", f"msg {i}") for i in range(5)]
        self._write_ledger(events)
        handle_memory_ingest({"group_id": self.group_id, "limit": 5})

        # Stale scenario
        all_events = events + [_make_chat_event(f"e{i}", "bob", f"msg {i}") for i in range(5, 15)]
        self._write_ledger(all_events)

        # Reset watermark and re-process
        resp = handle_memory_ingest({"group_id": self.group_id, "limit": 5, "reset_watermark": True})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["events_processed"], 5)
        self.assertEqual(resp.result["watermark"], "e14")
        self.assertFalse(resp.result.get("watermark_stale", False))

    def test_watermark_stale_field_false_when_normal(self):
        """watermark_stale is False in normal operation."""
        events = [_make_chat_event("e1", "alice", "hello")]
        self._write_ledger(events)
        resp = handle_memory_ingest({"group_id": self.group_id})
        self.assertTrue(resp.ok)
        self.assertFalse(resp.result.get("watermark_stale", False))


if __name__ == "__main__":
    unittest.main()
