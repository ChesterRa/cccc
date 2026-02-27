"""Tests for solidify + strategy system (T096)."""

import os
import tempfile
import unittest

from cccc.kernel.memory import (
    MemoryStore,
    MEMORY_STRATEGIES,
    AUTO_SOLIDIFY_HIT_THRESHOLD,
)


class SolidifyTestBase(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.db_path = os.path.join(self._td.name, "memory.db")
        self.group_id = "g_solidify_test"
        self.store = MemoryStore(self.db_path, group_id=self.group_id)

    def tearDown(self):
        self.store.close()
        self._td.cleanup()


class TestSolidify(SolidifyTestBase):
    """Test manual solidify()."""

    def test_solidify_draft_to_solid(self):
        r = self.store.store("solidify me", status="draft")
        mem = self.store.solidify(r["id"])
        self.assertIsNotNone(mem)
        self.assertEqual(mem["status"], "solid")

    def test_solidify_already_solid(self):
        r = self.store.store("already solid", status="solid")
        mem = self.store.solidify(r["id"])
        self.assertIsNotNone(mem)
        self.assertEqual(mem["status"], "solid")

    def test_solidify_nonexistent(self):
        result = self.store.solidify("nonexistent_id")
        self.assertIsNone(result)

    def test_solidify_updates_updated_at(self):
        r = self.store.store("time check")
        before = self.store.get(r["id"])
        self.store.solidify(r["id"])
        after = self.store.get(r["id"])
        self.assertGreaterEqual(after["updated_at"], before["updated_at"])

    def test_solidify_persists(self):
        """solidify result persists in DB."""
        r = self.store.store("persist check")
        self.store.solidify(r["id"])
        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "solid")


class TestAutoSolidify(SolidifyTestBase):
    """Test auto-solidify via hit_count threshold in recall(track_hit=True)."""

    def test_auto_solidify_at_threshold(self):
        """Draft memory auto-solidifies when hit_count reaches threshold via track_hit."""
        r = self.store.store("auto solidify target")
        # Recall with track_hit=True enough times to reach threshold
        for _ in range(AUTO_SOLIDIFY_HIT_THRESHOLD):
            self.store.recall("solidify", track_hit=True)

        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "solid")
        self.assertGreaterEqual(mem["hit_count"], AUTO_SOLIDIFY_HIT_THRESHOLD)

    def test_no_auto_solidify_below_threshold(self):
        """Draft memory stays draft below threshold."""
        r = self.store.store("below threshold")
        for _ in range(AUTO_SOLIDIFY_HIT_THRESHOLD - 1):
            self.store.recall("threshold", track_hit=True)

        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "draft")

    def test_auto_solidify_only_affects_draft(self):
        """Already solid memory is not affected by auto-solidify logic."""
        r = self.store.store("already solid", status="solid")
        for _ in range(AUTO_SOLIDIFY_HIT_THRESHOLD + 1):
            self.store.recall("solid", track_hit=True)

        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "solid")

    def test_auto_solidify_in_results(self):
        """recall(track_hit=True) results reflect auto-solidified status."""
        self.store.store("results check")
        for _ in range(AUTO_SOLIDIFY_HIT_THRESHOLD - 1):
            self.store.recall("results", track_hit=True)

        # One more recall should trigger auto-solidify
        results = self.store.recall("results", track_hit=True)
        self.assertEqual(len(results), 1)
        self.assertEqual(results[0]["status"], "solid")

    def test_no_auto_solidify_without_track_hit(self):
        """recall() without track_hit does NOT trigger auto-solidify."""
        r = self.store.store("no auto solidify")
        for _ in range(AUTO_SOLIDIFY_HIT_THRESHOLD + 2):
            self.store.recall("auto solidify")

        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "draft")
        self.assertEqual(mem["hit_count"], 0)


class TestStrategy(SolidifyTestBase):
    """Test MEMORY_STRATEGIES with store()."""

    def test_strategies_defined(self):
        """All three strategies exist."""
        self.assertIn("aggressive", MEMORY_STRATEGIES)
        self.assertIn("conservative", MEMORY_STRATEGIES)
        self.assertIn("milestone-only", MEMORY_STRATEGIES)

    def test_aggressive_strategy(self):
        """aggressive: store as solid, medium confidence."""
        r = self.store.store("aggressive test", strategy="aggressive")
        self.assertEqual(r["status"], "solid")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "solid")
        self.assertEqual(mem["confidence"], "medium")

    def test_conservative_strategy(self):
        """conservative: store as draft, high confidence."""
        r = self.store.store("conservative test", strategy="conservative")
        self.assertEqual(r["status"], "draft")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "draft")
        self.assertEqual(mem["confidence"], "high")

    def test_milestone_only_strategy(self):
        """milestone-only: store as solid, high confidence."""
        r = self.store.store("milestone test", strategy="milestone-only")
        self.assertEqual(r["status"], "solid")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "solid")
        self.assertEqual(mem["confidence"], "high")

    def test_no_strategy(self):
        """No strategy: uses explicit status/confidence params."""
        r = self.store.store("no strategy", status="draft", confidence="low")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "draft")
        self.assertEqual(mem["confidence"], "low")

    def test_unknown_strategy_rejected(self):
        """Unknown strategy name raises ValueError (enum strong validation)."""
        with self.assertRaises(ValueError):
            self.store.store("unknown strategy", strategy="nonexistent")

    def test_strategy_overrides_explicit_status(self):
        """Strategy overrides explicit status parameter."""
        r = self.store.store("override test", status="draft", strategy="aggressive")
        self.assertEqual(r["status"], "solid")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "solid")

    def test_strategy_overrides_explicit_confidence(self):
        """Strategy overrides explicit confidence parameter."""
        r = self.store.store("conf override", confidence="low", strategy="conservative")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["confidence"], "high")


class TestAutoSolidifyThreshold(SolidifyTestBase):
    """Verify the threshold constant is reasonable."""

    def test_threshold_value(self):
        self.assertEqual(AUTO_SOLIDIFY_HIT_THRESHOLD, 3)


if __name__ == "__main__":
    unittest.main()
