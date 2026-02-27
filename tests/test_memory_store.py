"""Tests for MemoryStore core + Schema + CRUD (T094)."""

import os
import tempfile
import unittest

from cccc.kernel.memory import MemoryStore, content_hash, SCHEMA_VERSION


class MemoryStoreTestBase(unittest.TestCase):
    """Base class with temp DB setup."""

    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.db_path = os.path.join(self._td.name, "memory.db")
        self.group_id = "g_test123"
        self.store = MemoryStore(self.db_path, group_id=self.group_id)

    def tearDown(self):
        self.store.close()
        self._td.cleanup()


class TestSchema(MemoryStoreTestBase):
    def test_tables_created(self):
        """All required tables exist after init."""
        assert self.store._conn is not None
        tables = {
            r[0]
            for r in self.store._conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ).fetchall()
        }
        self.assertIn("memories", tables)
        self.assertIn("memory_tags", tables)
        self.assertIn("memory_fts", tables)

    def test_user_version_set(self):
        """PRAGMA user_version is set to SCHEMA_VERSION."""
        assert self.store._conn is not None
        ver = self.store._conn.execute("PRAGMA user_version").fetchone()[0]
        self.assertEqual(ver, SCHEMA_VERSION)

    def test_indexes_created(self):
        """12+ indexes exist."""
        assert self.store._conn is not None
        indexes = self.store._conn.execute(
            "SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%'"
        ).fetchall()
        index_names = {r[0] for r in indexes}
        expected = {
            "idx_memories_group_id",
            "idx_memories_status",
            "idx_memories_actor_id",
            "idx_memories_task_id",
            "idx_memories_milestone_id",
            "idx_memories_kind",
            "idx_memories_source_type",
            "idx_memories_event_ts",
            "idx_memories_created_at",
            "idx_memories_content_hash",
            "idx_memories_confidence",
            "idx_memory_tags_memory_id",
        }
        for idx in expected:
            self.assertIn(idx, index_names, f"Missing index: {idx}")

    def test_wal_mode(self):
        """WAL journal mode is set."""
        assert self.store._conn is not None
        mode = self.store._conn.execute("PRAGMA journal_mode").fetchone()[0]
        self.assertEqual(mode, "wal")

    def test_foreign_keys_enabled(self):
        """Foreign keys are enabled."""
        assert self.store._conn is not None
        fk = self.store._conn.execute("PRAGMA foreign_keys").fetchone()[0]
        self.assertEqual(fk, 1)


class TestContentHash(unittest.TestCase):
    def test_deterministic(self):
        h1 = content_hash("hello world")
        h2 = content_hash("hello world")
        self.assertEqual(h1, h2)

    def test_different_content(self):
        h1 = content_hash("hello")
        h2 = content_hash("world")
        self.assertNotEqual(h1, h2)

    def test_no_strip(self):
        """content_hash does NOT strip whitespace."""
        h1 = content_hash("hello ")
        h2 = content_hash("hello")
        self.assertNotEqual(h1, h2)


class TestHitCount(MemoryStoreTestBase):
    def test_hit_count_default_zero(self):
        """New memories have hit_count=0."""
        result = self.store.store("hit count test")
        mem = self.store.get(result["id"])
        self.assertEqual(mem["hit_count"], 0)

    def test_hit_count_column_exists(self):
        """hit_count column exists in the schema."""
        assert self.store._conn is not None
        info = self.store._conn.execute("PRAGMA table_info(memories)").fetchall()
        col_names = {r["name"] for r in info}
        self.assertIn("hit_count", col_names)


class TestStore(MemoryStoreTestBase):
    def test_store_basic(self):
        result = self.store.store("test memory content")
        self.assertFalse(result["deduplicated"])
        self.assertIn("id", result)
        self.assertIn("content_hash", result)
        self.assertIn("created_at", result)

    def test_store_with_all_fields(self):
        result = self.store.store(
            "detailed memory",
            kind="decision",
            source_type="chat_ingest",
            source_ref="evt_123",
            status="solid",
            confidence="high",
            scope_key="s_abc",
            actor_id="peer-impl",
            task_id="T094",
            milestone_id="M7",
            event_ts="2026-02-25T00:00:00Z",
            tags=["architecture", "memory"],
        )
        self.assertFalse(result["deduplicated"])
        mem = self.store.get(result["id"])
        self.assertIsNotNone(mem)
        self.assertEqual(mem["content"], "detailed memory")
        self.assertEqual(mem["kind"], "decision")
        self.assertEqual(mem["source_type"], "chat_ingest")
        self.assertEqual(mem["source_ref"], "evt_123")
        self.assertEqual(mem["status"], "solid")
        self.assertEqual(mem["confidence"], "high")
        self.assertEqual(mem["scope_key"], "s_abc")
        self.assertEqual(mem["actor_id"], "peer-impl")
        self.assertEqual(mem["task_id"], "T094")
        self.assertEqual(mem["milestone_id"], "M7")
        self.assertEqual(mem["event_ts"], "2026-02-25T00:00:00Z")
        self.assertEqual(mem["group_id"], self.group_id)
        self.assertEqual(sorted(mem["tags"]), ["architecture", "memory"])

    def test_store_custom_id(self):
        result = self.store.store("custom id", memory_id="my_custom_id")
        self.assertEqual(result["id"], "my_custom_id")

    def test_store_dedup(self):
        r1 = self.store.store("same content")
        r2 = self.store.store("same content")
        self.assertFalse(r1["deduplicated"])
        self.assertTrue(r2["deduplicated"])
        self.assertEqual(r1["id"], r2["id"])

    def test_store_dedup_different_groups(self):
        """Dedup is per-group."""
        r1 = self.store.store("shared content")
        store2 = MemoryStore(self.db_path, group_id="g_other")
        try:
            r2 = store2.store("shared content")
            self.assertFalse(r2["deduplicated"])
            self.assertNotEqual(r1["id"], r2["id"])
        finally:
            store2.close()

    def test_store_tags_stripped(self):
        """Tags are stripped of whitespace."""
        result = self.store.store("tagged", tags=["  foo ", "bar", "  ", "baz"])
        mem = self.store.get(result["id"])
        self.assertEqual(sorted(mem["tags"]), ["bar", "baz", "foo"])


class TestDedupSemantics(MemoryStoreTestBase):
    """Bug 1 fix: dedup respects source_ref — different source_ref = separate records."""

    def test_different_source_ref_same_content_not_deduped(self):
        """Different source_ref + same content should create two records."""
        r1 = self.store.store("same content", source_ref="ref_a")
        r2 = self.store.store("same content", source_ref="ref_b")
        self.assertFalse(r1["deduplicated"])
        self.assertFalse(r2["deduplicated"])
        self.assertNotEqual(r1["id"], r2["id"])

    def test_same_source_ref_same_content_deduped(self):
        """Same source_ref + same content = idempotent retry (dedup)."""
        r1 = self.store.store("same content", source_ref="ref_a")
        r2 = self.store.store("same content", source_ref="ref_a")
        self.assertFalse(r1["deduplicated"])
        self.assertTrue(r2["deduplicated"])
        self.assertEqual(r1["id"], r2["id"])

    def test_empty_source_ref_dedup_by_content(self):
        """Empty source_ref + same content = dedup (backward compat)."""
        r1 = self.store.store("same content")
        r2 = self.store.store("same content")
        self.assertFalse(r1["deduplicated"])
        self.assertTrue(r2["deduplicated"])
        self.assertEqual(r1["id"], r2["id"])

    def test_empty_and_nonempty_source_ref_not_deduped(self):
        """Empty source_ref and non-empty source_ref with same content = separate records."""
        r1 = self.store.store("same content")  # source_ref=""
        r2 = self.store.store("same content", source_ref="ref_a")
        self.assertFalse(r1["deduplicated"])
        self.assertFalse(r2["deduplicated"])
        self.assertNotEqual(r1["id"], r2["id"])


class TestGet(MemoryStoreTestBase):
    def test_get_existing(self):
        result = self.store.store("get me")
        mem = self.store.get(result["id"])
        self.assertIsNotNone(mem)
        self.assertEqual(mem["content"], "get me")

    def test_get_nonexistent(self):
        mem = self.store.get("nonexistent_id")
        self.assertIsNone(mem)


class TestUpdate(MemoryStoreTestBase):
    def test_update_content(self):
        result = self.store.store("original")
        updated = self.store.update(result["id"], content="modified")
        self.assertIsNotNone(updated)
        self.assertEqual(updated["content"], "modified")
        self.assertNotEqual(updated["content_hash"], content_hash("original"))
        self.assertEqual(updated["content_hash"], content_hash("modified"))

    def test_update_kind(self):
        result = self.store.store("obs")
        updated = self.store.update(result["id"], kind="decision")
        self.assertEqual(updated["kind"], "decision")

    def test_update_status(self):
        result = self.store.store("draft mem")
        updated = self.store.update(result["id"], status="solid")
        self.assertEqual(updated["status"], "solid")

    def test_update_tags(self):
        result = self.store.store("tagged", tags=["a", "b"])
        updated = self.store.update(result["id"], tags=["c", "d"])
        self.assertEqual(sorted(updated["tags"]), ["c", "d"])

    def test_update_tags_to_empty(self):
        result = self.store.store("tagged", tags=["a"])
        updated = self.store.update(result["id"], tags=[])
        self.assertEqual(updated["tags"], [])

    def test_update_nonexistent(self):
        result = self.store.update("nonexistent", content="new")
        self.assertIsNone(result)

    def test_update_multiple_fields(self):
        result = self.store.store("multi")
        updated = self.store.update(
            result["id"],
            kind="fact",
            confidence="high",
            actor_id="peer-1",
            task_id="T001",
            milestone_id="M1",
        )
        self.assertEqual(updated["kind"], "fact")
        self.assertEqual(updated["confidence"], "high")
        self.assertEqual(updated["actor_id"], "peer-1")
        self.assertEqual(updated["task_id"], "T001")
        self.assertEqual(updated["milestone_id"], "M1")

    def test_update_updated_at_changes(self):
        result = self.store.store("timekeep")
        mem_before = self.store.get(result["id"])
        updated = self.store.update(result["id"], kind="fact")
        self.assertGreaterEqual(updated["updated_at"], mem_before["updated_at"])


class TestDelete(MemoryStoreTestBase):
    def test_delete_existing(self):
        result = self.store.store("to delete")
        self.assertTrue(self.store.delete(result["id"]))
        self.assertIsNone(self.store.get(result["id"]))

    def test_delete_nonexistent(self):
        self.assertFalse(self.store.delete("nonexistent"))

    def test_delete_cascades_tags(self):
        result = self.store.store("tagged", tags=["x", "y"])
        self.store.delete(result["id"])
        assert self.store._conn is not None
        count = self.store._conn.execute(
            "SELECT COUNT(*) as c FROM memory_tags WHERE memory_id = ?",
            (result["id"],),
        ).fetchone()["c"]
        self.assertEqual(count, 0)

    def test_delete_many_mixed_ids(self):
        m1 = self.store.store("to delete 1")["id"]
        m2 = self.store.store("to delete 2")["id"]
        result = self.store.delete_many([m1, "missing", m2, m1])
        self.assertEqual(result["deleted"], 2)
        self.assertEqual(result["ids"], [m1, m2])
        self.assertIsNone(self.store.get(m1))
        self.assertIsNone(self.store.get(m2))

    def test_delete_many_empty(self):
        result = self.store.delete_many([])
        self.assertEqual(result["deleted"], 0)
        self.assertEqual(result["ids"], [])


class TestListMemories(MemoryStoreTestBase):
    def test_list_empty(self):
        self.assertEqual(self.store.list_memories(), [])

    def test_list_basic(self):
        self.store.store("a")
        self.store.store("b")
        mems = self.store.list_memories()
        self.assertEqual(len(mems), 2)

    def test_list_filter_status(self):
        self.store.store("draft one", status="draft")
        self.store.store("solid one", status="solid")
        drafts = self.store.list_memories(status="draft")
        self.assertEqual(len(drafts), 1)
        self.assertEqual(drafts[0]["status"], "draft")

    def test_list_filter_kind(self):
        self.store.store("obs", kind="observation")
        self.store.store("dec", kind="decision")
        obs = self.store.list_memories(kind="observation")
        self.assertEqual(len(obs), 1)
        self.assertEqual(obs[0]["kind"], "observation")

    def test_list_limit_offset(self):
        for i in range(5):
            self.store.store(f"mem {i}")
        page1 = self.store.list_memories(limit=2, offset=0)
        page2 = self.store.list_memories(limit=2, offset=2)
        self.assertEqual(len(page1), 2)
        self.assertEqual(len(page2), 2)
        ids1 = {m["id"] for m in page1}
        ids2 = {m["id"] for m in page2}
        self.assertTrue(ids1.isdisjoint(ids2))

    def test_list_scoped_to_group(self):
        """list_memories only returns memories for this group."""
        self.store.store("mine")
        store2 = MemoryStore(self.db_path, group_id="g_other")
        try:
            store2.store("theirs")
            mine = self.store.list_memories()
            theirs = store2.list_memories()
            self.assertEqual(len(mine), 1)
            self.assertEqual(len(theirs), 1)
        finally:
            store2.close()


class TestFindByHash(MemoryStoreTestBase):
    def test_find_existing(self):
        result = self.store.store("hashable")
        h = content_hash("hashable")
        found = self.store.find_by_hash(h)
        self.assertIsNotNone(found)
        self.assertEqual(found["id"], result["id"])

    def test_find_nonexistent(self):
        found = self.store.find_by_hash("0" * 64)
        self.assertIsNone(found)


class TestStats(MemoryStoreTestBase):
    def test_stats_empty(self):
        s = self.store.stats()
        self.assertEqual(s["total"], 0)
        self.assertEqual(s["by_status"], {})
        self.assertEqual(s["by_kind"], {})
        self.assertEqual(s["tag_count"], 0)

    def test_stats_with_data(self):
        self.store.store("a", kind="observation", status="draft", tags=["t1"])
        self.store.store("b", kind="decision", status="solid", tags=["t2", "t3"])
        self.store.store("c", kind="observation", status="draft")
        s = self.store.stats()
        self.assertEqual(s["total"], 3)
        self.assertEqual(s["by_status"], {"draft": 2, "solid": 1})
        self.assertEqual(s["by_kind"], {"observation": 2, "decision": 1})
        self.assertEqual(s["tag_count"], 3)


class TestContextManager(MemoryStoreTestBase):
    def test_context_manager_closes(self):
        with MemoryStore(self.db_path, group_id=self.group_id) as store:
            store.store("cm test")
            self.assertIsNotNone(store._conn)
        self.assertIsNone(store._conn)

    def test_context_manager_usable(self):
        with MemoryStore(self.db_path, group_id=self.group_id) as store:
            result = store.store("usable")
            mem = store.get(result["id"])
            self.assertEqual(mem["content"], "usable")


class TestFTS5Triggers(MemoryStoreTestBase):
    """Verify FTS5 triggers sync content correctly."""

    def test_insert_populates_fts(self):
        """Storing a memory makes it findable via FTS5."""
        self.store.store("unique quantum entanglement observation")
        assert self.store._conn is not None
        rows = self.store._conn.execute(
            "SELECT * FROM memory_fts WHERE memory_fts MATCH ?",
            ('"quantum entanglement"',),
        ).fetchall()
        self.assertEqual(len(rows), 1)

    def test_delete_removes_from_fts(self):
        """Deleting a memory removes it from FTS5."""
        result = self.store.store("ephemeral data point")
        self.store.delete(result["id"])
        assert self.store._conn is not None
        rows = self.store._conn.execute(
            "SELECT * FROM memory_fts WHERE memory_fts MATCH ?",
            ('"ephemeral data"',),
        ).fetchall()
        self.assertEqual(len(rows), 0)

    def test_update_content_updates_fts(self):
        """Updating content updates the FTS5 index."""
        result = self.store.store("old indexed content")
        self.store.update(result["id"], content="new indexed content")
        assert self.store._conn is not None
        old = self.store._conn.execute(
            "SELECT * FROM memory_fts WHERE memory_fts MATCH ?",
            ('"old indexed"',),
        ).fetchall()
        new = self.store._conn.execute(
            "SELECT * FROM memory_fts WHERE memory_fts MATCH ?",
            ('"new indexed"',),
        ).fetchall()
        self.assertEqual(len(old), 0)
        self.assertEqual(len(new), 1)


class TestGroupIdDefense(MemoryStoreTestBase):
    """T103: get/update/delete/solidify enforce group_id constraint."""

    def _make_other_store(self):
        """Create a store for a different group on the same DB."""
        return MemoryStore(self.db_path, group_id="g_other")

    def test_get_cannot_cross_group(self):
        """get() from store A cannot read store B's memory."""
        result = self.store.store("group A data")
        other = self._make_other_store()
        try:
            self.assertIsNone(other.get(result["id"]))
        finally:
            other.close()

    def test_update_cannot_cross_group(self):
        """update() from store A cannot modify store B's memory."""
        result = self.store.store("original")
        other = self._make_other_store()
        try:
            updated = other.update(result["id"], content="hacked")
            self.assertIsNone(updated)
            # Verify original is unchanged
            mem = self.store.get(result["id"])
            self.assertEqual(mem["content"], "original")
        finally:
            other.close()

    def test_delete_cannot_cross_group(self):
        """delete() from store A cannot remove store B's memory."""
        result = self.store.store("protected")
        other = self._make_other_store()
        try:
            self.assertFalse(other.delete(result["id"]))
            # Verify still exists in original group
            self.assertIsNotNone(self.store.get(result["id"]))
        finally:
            other.close()

    def test_solidify_cannot_cross_group(self):
        """solidify() from store A cannot solidify store B's memory."""
        result = self.store.store("draft memory", status="draft")
        other = self._make_other_store()
        try:
            solidified = other.solidify(result["id"])
            self.assertIsNone(solidified)
            # Verify still draft in original group
            mem = self.store.get(result["id"])
            self.assertEqual(mem["status"], "draft")
        finally:
            other.close()

    def test_same_group_operations_work(self):
        """Verify normal operations still work for the correct group."""
        result = self.store.store("normal", status="draft")
        mid = result["id"]

        # get works
        self.assertIsNotNone(self.store.get(mid))

        # update works
        updated = self.store.update(mid, content="updated")
        self.assertIsNotNone(updated)
        self.assertEqual(updated["content"], "updated")

        # solidify works
        solidified = self.store.solidify(mid)
        self.assertIsNotNone(solidified)
        self.assertEqual(solidified["status"], "solid")

        # delete works
        self.assertTrue(self.store.delete(mid))
        self.assertIsNone(self.store.get(mid))


if __name__ == "__main__":
    unittest.main()
