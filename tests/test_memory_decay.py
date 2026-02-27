"""Tests for memory decay candidate discovery."""

import tempfile
import unittest
from datetime import datetime, timedelta, timezone

from cccc.kernel.memory import MemoryStore


def _iso_days_ago(days: int) -> str:
    return (datetime.now(timezone.utc) - timedelta(days=days)).isoformat().replace("+00:00", "Z")


class MemoryDecayTest(unittest.TestCase):
    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.db_path = f"{self._td.name}/memory.db"
        self.store = MemoryStore(self.db_path, group_id="g_decay")

    def tearDown(self):
        self.store.close()
        self._td.cleanup()

    def _set_age(self, memory_id: str, *, created_days: int, recalled_days: int | None = None) -> None:
        assert self.store._conn is not None
        created_at = _iso_days_ago(created_days)
        last_recalled = _iso_days_ago(recalled_days) if recalled_days is not None else ""
        self.store._conn.execute(
            "UPDATE memories SET created_at = ?, updated_at = ?, last_recalled_at = ? "
            "WHERE id = ? AND group_id = ?",
            (created_at, created_at, last_recalled, memory_id, self.store.group_id),
        )
        self.store._conn.commit()

    def test_draft_zero_hit_is_high_priority_delete_candidate(self):
        mid = self.store.store("stale draft", status="draft")["id"]
        self._set_age(mid, created_days=45)

        result = self.store.find_stale(draft_days=30, zero_hit_days=14)
        by_id = {c["id"]: c for c in result["candidates"]}
        self.assertIn(mid, by_id)
        self.assertEqual(by_id[mid]["recommended_action"], "delete_candidate")
        self.assertEqual(by_id[mid]["priority"], "high")

    def test_recent_draft_not_in_candidates(self):
        mid = self.store.store("recent draft", status="draft")["id"]
        self._set_age(mid, created_days=3)

        result = self.store.find_stale(draft_days=30, zero_hit_days=14)
        ids = [c["id"] for c in result["candidates"]]
        self.assertNotIn(mid, ids)

    def test_old_solid_is_review_candidate_not_delete(self):
        mid = self.store.store("old solid", status="solid")["id"]
        self._set_age(mid, created_days=180)

        result = self.store.find_stale(solid_review_days=120, solid_max_hit=1)
        by_id = {c["id"]: c for c in result["candidates"]}
        self.assertIn(mid, by_id)
        self.assertEqual(by_id[mid]["status"], "solid")
        self.assertEqual(by_id[mid]["recommended_action"], "review_candidate")

    def test_limit_applies(self):
        mids = [self.store.store(f"draft {i}", status="draft")["id"] for i in range(3)]
        for mid in mids:
            self._set_age(mid, created_days=60)
        result = self.store.find_stale(limit=2)
        self.assertEqual(result["count"], 2)
        self.assertEqual(len(result["candidates"]), 2)

    def test_invalid_limit_raises(self):
        with self.assertRaises(ValueError):
            self.store.find_stale(limit=0)


if __name__ == "__main__":
    unittest.main()
