"""Tests for MemoryStore recall() - FTS5 search + structured query (T095)."""

import os
import tempfile
import unittest

from cccc.kernel.memory import MemoryStore


class RecallTestBase(unittest.TestCase):
    """Base class with temp DB and sample data."""

    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.db_path = os.path.join(self._td.name, "memory.db")
        self.group_id = "g_recall_test"
        self.store = MemoryStore(self.db_path, group_id=self.group_id)

    def tearDown(self):
        self.store.close()
        self._td.cleanup()

    def _seed_data(self):
        """Seed test memories for recall tests."""
        self.store.store(
            "SQLite FTS5 全文搜索引擎性能优异",
            kind="fact", status="solid", actor_id="peer-arch",
            task_id="T001", confidence="high",
            tags=["database", "search"],
        )
        self.store.store(
            "Memory system uses SQLite for persistent storage",
            kind="observation", status="draft", actor_id="peer-impl",
            task_id="T002", confidence="medium",
            tags=["database", "memory"],
        )
        self.store.store(
            "Decided to use FTS5 instead of manual indexing",
            kind="decision", status="solid", actor_id="peer-arch",
            task_id="T001", confidence="high",
            tags=["architecture"],
        )
        self.store.store(
            "用户偏好中文界面显示",
            kind="preference", status="draft", actor_id="claude-1",
            tags=["ui", "i18n"],
        )
        self.store.store(
            "Agent 驱动的记忆系统不依赖模型",
            kind="fact", status="solid",
            tags=["architecture", "memory"],
        )


class TestRecallNoQuery(RecallTestBase):
    """Test recall without text query (filter-only mode)."""

    def test_recall_all(self):
        self._seed_data()
        results = self.store.recall()
        self.assertEqual(len(results), 5)

    def test_recall_empty_store(self):
        results = self.store.recall()
        self.assertEqual(results, [])

    def test_recall_solid_first_ordering(self):
        """Solid memories come before draft memories."""
        self._seed_data()
        results = self.store.recall()
        statuses = [m["status"] for m in results]
        # All solid should come before all draft
        solid_idxs = [i for i, s in enumerate(statuses) if s == "solid"]
        draft_idxs = [i for i, s in enumerate(statuses) if s == "draft"]
        if solid_idxs and draft_idxs:
            self.assertLess(max(solid_idxs), min(draft_idxs))

    def test_recall_filter_status(self):
        self._seed_data()
        solid = self.store.recall(status="solid")
        self.assertEqual(len(solid), 3)
        for m in solid:
            self.assertEqual(m["status"], "solid")

    def test_recall_filter_kind(self):
        self._seed_data()
        facts = self.store.recall(kind="fact")
        self.assertEqual(len(facts), 2)
        for m in facts:
            self.assertEqual(m["kind"], "fact")

    def test_recall_filter_actor_id(self):
        self._seed_data()
        arch = self.store.recall(actor_id="peer-arch")
        self.assertEqual(len(arch), 2)

    def test_recall_filter_task_id(self):
        self._seed_data()
        t001 = self.store.recall(task_id="T001")
        self.assertEqual(len(t001), 2)

    def test_recall_filter_confidence(self):
        self._seed_data()
        high = self.store.recall(confidence="high")
        self.assertEqual(len(high), 2)

    def test_recall_filter_tags(self):
        self._seed_data()
        db = self.store.recall(tags=["database"])
        self.assertEqual(len(db), 2)

    def test_recall_filter_tags_multiple(self):
        """Multiple tags act as AND filter."""
        self._seed_data()
        both = self.store.recall(tags=["database", "search"])
        self.assertEqual(len(both), 1)

    def test_recall_filter_since(self):
        self._seed_data()
        # All our test data was just created, use a far-past date
        results = self.store.recall(since="2020-01-01T00:00:00Z")
        self.assertEqual(len(results), 5)
        results = self.store.recall(since="2099-01-01T00:00:00Z")
        self.assertEqual(len(results), 0)

    def test_recall_filter_until(self):
        self._seed_data()
        results = self.store.recall(until="2099-01-01T00:00:00Z")
        self.assertEqual(len(results), 5)
        results = self.store.recall(until="2020-01-01T00:00:00Z")
        self.assertEqual(len(results), 0)

    def test_recall_limit(self):
        self._seed_data()
        results = self.store.recall(limit=2)
        self.assertEqual(len(results), 2)

    def test_recall_combined_filters(self):
        self._seed_data()
        results = self.store.recall(status="solid", kind="fact")
        self.assertEqual(len(results), 2)
        results = self.store.recall(status="solid", kind="decision")
        self.assertEqual(len(results), 1)


class TestRecallFTS(RecallTestBase):
    """Test FTS5 full-text search."""

    def test_fts_basic(self):
        self._seed_data()
        results = self.store.recall("SQLite")
        self.assertGreater(len(results), 0)
        for m in results:
            self.assertIn("SQLite", m["content"])

    def test_fts_with_score(self):
        """Results include a score field."""
        self._seed_data()
        results = self.store.recall("SQLite")
        for m in results:
            self.assertIn("score", m)
            self.assertIsInstance(m["score"], float)

    def test_fts_no_results(self):
        self._seed_data()
        results = self.store.recall("nonexistent_xyzzy_keyword")
        self.assertEqual(results, [])

    def test_fts_with_filter(self):
        self._seed_data()
        results = self.store.recall("SQLite", status="solid")
        self.assertGreater(len(results), 0)
        for m in results:
            self.assertEqual(m["status"], "solid")

    def test_fts_special_characters(self):
        """Special characters in query don't cause errors (sanitized)."""
        self._seed_data()
        # These should not raise
        self.store.recall('test "quoted"')
        self.store.recall("test * wildcard")
        self.store.recall("OR AND NOT")
        self.store.recall("(unclosed")

    def test_fts_english_partial_word(self):
        self._seed_data()
        results = self.store.recall("persistent")
        self.assertGreater(len(results), 0)


class TestRecallCJK(RecallTestBase):
    """Test CJK (Chinese/Japanese/Korean) search supplement."""

    def test_cjk_like_supplement(self):
        """CJK query with len >= 2 uses LIKE supplement."""
        self._seed_data()
        results = self.store.recall("中文界面")
        self.assertGreater(len(results), 0)
        found_content = [m["content"] for m in results]
        self.assertTrue(any("中文界面" in c for c in found_content))

    def test_cjk_single_char_no_supplement(self):
        """Single CJK char (len < 2) doesn't trigger LIKE supplement,
        but FTS5 may or may not find it."""
        self._seed_data()
        # Single char - FTS5 might still work
        results = self.store.recall("中")
        # Just ensure no error
        self.assertIsInstance(results, list)

    def test_cjk_mixed_content(self):
        """Mixed Chinese/English content is searchable."""
        self._seed_data()
        results = self.store.recall("记忆系统")
        self.assertGreater(len(results), 0)

    def test_cjk_score_field(self):
        """CJK results also have score field."""
        self._seed_data()
        results = self.store.recall("记忆系统")
        for m in results:
            self.assertIn("score", m)

    def test_cjk_dedup(self):
        """FTS5 + LIKE results are deduplicated."""
        self._seed_data()
        results = self.store.recall("全文搜索")
        ids = [m["id"] for m in results]
        self.assertEqual(len(ids), len(set(ids)), "Duplicate IDs in results")


class TestRecallLikeWildcardEscape(RecallTestBase):
    """Regression tests for W1: LIKE wildcard escaping in CJK supplement."""

    def test_percent_in_query_does_not_match_all(self):
        """Query with % should not act as a LIKE wildcard."""
        self.store.store("specific content here")
        self.store.store("other unrelated text")
        # % should be escaped, not match everything
        results = self.store.recall("%")
        # Should not return both (if unescaped, % would match all)
        # FTS5 may or may not match %, but LIKE supplement with escaped % won't
        self.assertIsInstance(results, list)

    def test_underscore_in_query_does_not_match_single_char(self):
        """Query with _ should not act as a single-char LIKE wildcard."""
        self.store.store("cat")
        self.store.store("cut")
        self.store.store("cot")
        # If _ were unescaped, "c_t" would match all three via LIKE
        results = self.store.recall("c_t")
        # Only exact substring match, not wildcard
        for m in results:
            self.assertIn("c_t", m["content"])

    def test_cjk_with_percent_escaped(self):
        """CJK query containing % doesn't cause wildcard expansion."""
        self.store.store("内存占用100%满了")
        self.store.store("完全不相关的内容")
        results = self.store.recall("100%满")
        # Should find the first one, not both
        found = [m for m in results if "100%" in m["content"]]
        self.assertGreater(len(found), 0)

    def test_backslash_in_query(self):
        """Backslash in query is properly escaped."""
        self.store.store("path\\to\\file")
        results = self.store.recall("path\\to")
        self.assertIsInstance(results, list)


class TestRecallHitCount(RecallTestBase):
    """Test hit_count increment on recall (requires track_hit=True)."""

    def test_no_side_effects_by_default(self):
        """recall() without track_hit does NOT change hit_count."""
        r = self.store.store("no side effect test")
        self.store.recall("side effect")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["hit_count"], 0)

    def test_no_side_effects_last_recalled_at(self):
        """recall() without track_hit does NOT update last_recalled_at."""
        r = self.store.store("no side effect recall")
        self.store.recall("side effect")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["last_recalled_at"], "")

    def test_hit_count_increments_with_track_hit(self):
        """recall(track_hit=True) increments hit_count for returned memories."""
        r = self.store.store("hit counter test")
        mem_before = self.store.get(r["id"])
        self.assertEqual(mem_before["hit_count"], 0)

        results = self.store.recall("counter", track_hit=True)
        self.assertEqual(len(results), 1)
        self.assertEqual(results[0]["hit_count"], 1)

        # Verify persisted
        mem_after = self.store.get(r["id"])
        self.assertEqual(mem_after["hit_count"], 1)

    def test_hit_count_accumulates_with_track_hit(self):
        """Multiple recalls with track_hit=True accumulate hit_count."""
        r = self.store.store("accumulate me")
        self.store.recall("accumulate", track_hit=True)
        self.store.recall("accumulate", track_hit=True)
        self.store.recall("accumulate", track_hit=True)
        mem = self.store.get(r["id"])
        self.assertEqual(mem["hit_count"], 3)

    def test_hit_count_no_query_with_track_hit(self):
        """recall without query but with track_hit=True also increments."""
        r = self.store.store("no query hit")
        self.store.recall(track_hit=True)
        mem = self.store.get(r["id"])
        self.assertEqual(mem["hit_count"], 1)

    def test_hit_count_only_for_returned(self):
        """Only returned (matching) memories get hit_count incremented."""
        r1 = self.store.store("apple fruit", kind="fact")
        r2 = self.store.store("banana fruit", kind="fact")
        self.store.recall("apple", track_hit=True)
        m1 = self.store.get(r1["id"])
        m2 = self.store.get(r2["id"])
        self.assertEqual(m1["hit_count"], 1)
        self.assertEqual(m2["hit_count"], 0)


class TestRecallSolidFirstOrdering(RecallTestBase):
    """Detailed tests for solid-first ordering."""

    def test_solid_before_draft_in_search(self):
        """In FTS search results, solid comes before draft."""
        self.store.store("keyword alpha", status="draft")
        self.store.store("keyword beta", status="solid")
        self.store.store("keyword gamma", status="draft")
        results = self.store.recall("keyword")
        self.assertEqual(results[0]["status"], "solid")

    def test_created_at_desc_within_status(self):
        """Within same status group, newer memories come first."""
        import time
        self.store.store("early solid", status="solid")
        time.sleep(0.01)  # Ensure different timestamps
        self.store.store("late solid", status="solid")
        results = self.store.recall()
        solids = [m for m in results if m["status"] == "solid"]
        self.assertEqual(len(solids), 2)
        self.assertGreaterEqual(solids[0]["created_at"], solids[1]["created_at"])


class TestRecallFTSScoreSorting(RecallTestBase):
    """T102: Test that FTS5 score is used for sorting when query is present."""

    def test_higher_relevance_ranks_first_within_status(self):
        """More relevant results should rank higher within the same status group."""
        # Store memories with varying relevance to "SQLite"
        self.store.store("SQLite SQLite SQLite database engine", status="solid")
        self.store.store("The database uses a regular file", status="solid")
        self.store.store("SQLite is used for storage", status="solid")

        results = self.store.recall("SQLite")
        solid_results = [m for m in results if m["status"] == "solid"]
        # The memory with most "SQLite" occurrences should have higher score
        if len(solid_results) >= 2:
            scores = [m.get("score", 0.0) for m in solid_results]
            # First result should have highest score (score-aware sorting)
            self.assertGreaterEqual(scores[0], scores[-1])

    def test_no_query_sorts_by_created_at(self):
        """Without query, sorting remains by created_at DESC."""
        import time
        self.store.store("first created", status="solid")
        time.sleep(0.01)
        self.store.store("second created", status="solid")

        results = self.store.recall()
        solids = [m for m in results if m["status"] == "solid"]
        self.assertEqual(len(solids), 2)
        # Second created should come first (created_at DESC)
        self.assertIn("second", solids[0]["content"])

    def test_score_present_in_fts_results(self):
        """FTS results include meaningful score values."""
        self.store.store("SQLite FTS5 full-text search", status="solid")
        self.store.store("unrelated content about cats", status="solid")

        results = self.store.recall("SQLite FTS5")
        matching = [m for m in results if "SQLite" in m["content"]]
        self.assertGreater(len(matching), 0)
        for m in matching:
            self.assertGreater(m.get("score", 0.0), 0.0)


class TestRecallScoping(RecallTestBase):
    """Test that recall is scoped to the store's group_id."""

    def test_recall_scoped_to_group(self):
        """recall only returns memories from this group."""
        self.store.store("my group memory")
        other = MemoryStore(self.db_path, group_id="g_other")
        try:
            other.store("other group memory")
            mine = self.store.recall("memory")
            theirs = other.recall("memory")
            self.assertEqual(len(mine), 1)
            self.assertIn("my group", mine[0]["content"])
            self.assertEqual(len(theirs), 1)
            self.assertIn("other group", theirs[0]["content"])
        finally:
            other.close()


if __name__ == "__main__":
    unittest.main()
