"""Tests for solidify_batch() + milestone hook (Step 2)."""

import os
import tempfile
import unittest

from cccc.kernel.memory import MemoryStore


class SolidifyBatchTestBase(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.db_path = os.path.join(self._td.name, "memory.db")
        self.group_id = "g_test_batch"
        self.store = MemoryStore(self.db_path, group_id=self.group_id)

    def tearDown(self):
        self.store.close()
        self._td.cleanup()


class TestSolidifyBatch(SolidifyBatchTestBase):
    """solidify_batch() method on MemoryStore."""

    def test_solidify_batch_no_filter(self):
        """solidify_batch() solidifies all draft memories."""
        self.store.store("mem1")
        self.store.store("mem2")
        self.store.store("mem3", status="solid")
        result = self.store.solidify_batch()
        self.assertEqual(result["solidified"], 2)
        # Check all are solid now
        mems = self.store.list_memories()
        for m in mems:
            self.assertEqual(m["status"], "solid")

    def test_solidify_batch_by_kind(self):
        """solidify_batch(kind=...) only solidifies that kind."""
        self.store.store("decision1", kind="decision")
        self.store.store("fact1", kind="fact")
        result = self.store.solidify_batch(kind="decision")
        self.assertEqual(result["solidified"], 1)

    def test_solidify_batch_by_milestone(self):
        """solidify_batch(milestone_id=...) only solidifies that milestone."""
        self.store.store("m1 mem", milestone_id="M1")
        self.store.store("m2 mem", milestone_id="M2")
        result = self.store.solidify_batch(milestone_id="M1")
        self.assertEqual(result["solidified"], 1)

    def test_solidify_batch_empty(self):
        """solidify_batch() on empty store returns 0."""
        result = self.store.solidify_batch()
        self.assertEqual(result["solidified"], 0)

    def test_solidify_batch_already_solid(self):
        """solidify_batch() on all-solid memories returns 0."""
        self.store.store("already solid", status="solid")
        result = self.store.solidify_batch()
        self.assertEqual(result["solidified"], 0)

    def test_solidify_batch_returns_ids(self):
        """solidify_batch() returns list of solidified IDs."""
        r1 = self.store.store("mem1")
        r2 = self.store.store("mem2")
        result = self.store.solidify_batch()
        self.assertEqual(set(result["ids"]), {r1["id"], r2["id"]})

    def test_solidify_batch_records_milestone(self):
        """solidify_batch(milestone_id=...) records milestone in meta."""
        self.store.store("content", milestone_id="M5")
        result = self.store.solidify_batch(milestone_id="M5")
        self.assertEqual(result["solidified"], 1)
        # Check meta records last solidify event
        meta_val = self.store.get_meta("last_solidify_batch")
        self.assertIsNotNone(meta_val)


if __name__ == "__main__":
    unittest.main()
