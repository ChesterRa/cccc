"""Tests for watermark persistence (Step 5)."""

import json
import os
import tempfile
import unittest
from unittest.mock import patch

from cccc.daemon.memory.memory_ops import (
    handle_memory_ingest,
    _get_memory_store,
    close_all_stores,
)


class WatermarkPersistenceTestBase(unittest.TestCase):
    """Base that mocks load_group + ledger for ingest tests."""

    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.group_dir = os.path.join(self._td.name, "group")
        os.makedirs(self.group_dir, exist_ok=True)
        self.ledger_path = os.path.join(self.group_dir, "ledger.jsonl")
        self.group_id = "g_test_wm"

        # Write some ledger lines
        events = [
            {"v": 1, "id": "ev1", "ts": "2026-01-01T00:00:00Z", "kind": "chat.message",
             "group_id": self.group_id, "by": "alice",
             "data": {"text": "hello world"}},
            {"v": 1, "id": "ev2", "ts": "2026-01-01T00:01:00Z", "kind": "chat.message",
             "group_id": self.group_id, "by": "bob",
             "data": {"text": "hi there"}},
            {"v": 1, "id": "ev3", "ts": "2026-01-01T00:02:00Z", "kind": "chat.message",
             "group_id": self.group_id, "by": "alice",
             "data": {"text": "testing persistence"}},
        ]
        with open(self.ledger_path, "w") as f:
            for ev in events:
                f.write(json.dumps(ev) + "\n")

        # Mock load_group to return a fake group
        self._group_patcher = patch("cccc.daemon.memory.memory_ops.load_group")
        self._mock_load = self._group_patcher.start()

        class FakeGroup:
            def __init__(self, path, ledger_path):
                self.path = type("P", (), {"__truediv__": lambda s, k: os.path.join(str(s), k), "__str__": lambda s: path})()
                self.path = type("Path", (), {
                    "__truediv__": lambda s, k: os.path.join(path, k),
                    "__str__": lambda s: path,
                })()
                self.ledger_path = ledger_path

        self._mock_load.return_value = FakeGroup(self.group_dir, self.ledger_path)

        # Patch read_last_lines to use real file
        self._lines_patcher = patch("cccc.daemon.memory.memory_ops.read_last_lines")
        self._mock_lines = self._lines_patcher.start()
        with open(self.ledger_path) as f:
            self._all_lines = f.readlines()
        self._mock_lines.return_value = self._all_lines

    def tearDown(self):
        close_all_stores()
        self._group_patcher.stop()
        self._lines_patcher.stop()
        self._td.cleanup()


class TestWatermarkPersistence(WatermarkPersistenceTestBase):
    """Watermark survives simulated restart (close + reopen store)."""

    def test_watermark_persisted_in_meta(self):
        """After ingest, watermark is saved to memory_meta."""
        result = handle_memory_ingest({
            "group_id": self.group_id,
            "mode": "signal",
        })
        self.assertTrue(result.ok)
        wm = result.result.get("watermark", "")
        self.assertTrue(wm)  # Non-empty watermark

        # Check watermark in meta
        store = _get_memory_store(self.group_id)
        self.assertIsNotNone(store)
        meta_wm = store.get_meta("ingest_watermark")
        self.assertEqual(meta_wm, wm)

    def test_watermark_survives_restart(self):
        """Watermark survives close_all_stores (simulated restart)."""
        # First ingest
        r1 = handle_memory_ingest({
            "group_id": self.group_id,
            "mode": "signal",
        })
        self.assertTrue(r1.ok)
        wm1 = r1.result["watermark"]

        # Simulate restart: close all stores
        close_all_stores()

        # Second ingest: should resume from watermark
        r2 = handle_memory_ingest({
            "group_id": self.group_id,
            "mode": "signal",
        })
        self.assertTrue(r2.ok)
        # Should have 0 new events (all already processed)
        self.assertEqual(r2.result["events_processed"], 0)

    def test_reset_watermark_still_works(self):
        """reset_watermark=true clears persistent watermark."""
        # First ingest
        handle_memory_ingest({
            "group_id": self.group_id,
            "mode": "signal",
        })

        # Simulate restart
        close_all_stores()

        # Reset and re-ingest
        r = handle_memory_ingest({
            "group_id": self.group_id,
            "mode": "signal",
            "reset_watermark": True,
        })
        self.assertTrue(r.ok)
        # Should process all events again
        self.assertGreater(r.result["events_processed"], 0)

    def test_no_module_level_watermark_dict(self):
        """Module-level _ingest_watermarks dict should be removed."""
        import cccc.daemon.memory.memory_ops as mod
        # The old _ingest_watermarks dict should no longer exist
        self.assertFalse(hasattr(mod, "_ingest_watermarks"),
                         "_ingest_watermarks module-level dict should be removed")


if __name__ == "__main__":
    unittest.main()
