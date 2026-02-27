"""Tests for memory_meta KV table + Schema v1→v2 migration (Step 1)."""

import os
import sqlite3
import tempfile
import unittest

from cccc.kernel.memory import (
    MemoryStore,
    SCHEMA_VERSION,
    MEMORY_KINDS,
    MEMORY_SOURCE_TYPES,
    MEMORY_STATUSES,
    MEMORY_CONFIDENCE_LEVELS,
)


class MemoryMetaTestBase(unittest.TestCase):
    """Base class with temp DB setup."""

    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.db_path = os.path.join(self._td.name, "memory.db")
        self.group_id = "g_test_meta"
        self.store = MemoryStore(self.db_path, group_id=self.group_id)

    def tearDown(self):
        self.store.close()
        self._td.cleanup()


class TestSchemaV2(MemoryMetaTestBase):
    """Schema v3: summary column + PRAGMA user_version=3."""

    def test_schema_version_is_3(self):
        """New DB has SCHEMA_VERSION == 3."""
        self.assertEqual(SCHEMA_VERSION, 3)
        assert self.store._conn is not None
        v = self.store._conn.execute("PRAGMA user_version").fetchone()[0]
        self.assertEqual(v, 3)

    def test_memory_meta_table_exists(self):
        """memory_meta table is created."""
        assert self.store._conn is not None
        tables = {
            r[0]
            for r in self.store._conn.execute(
                "SELECT name FROM sqlite_master WHERE type='table'"
            ).fetchall()
        }
        self.assertIn("memory_meta", tables)

    def test_memory_meta_columns(self):
        """memory_meta has key, value, updated_at columns."""
        assert self.store._conn is not None
        cols = self.store._conn.execute("PRAGMA table_info(memory_meta)").fetchall()
        col_names = {c[1] for c in cols}
        self.assertEqual(col_names, {"key", "value", "updated_at"})


class TestSchemaV1Migration(unittest.TestCase):
    """Migrate existing v1 DB to v2."""

    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.db_path = os.path.join(self._td.name, "memory.db")
        self.group_id = "g_test_migrate"

    def tearDown(self):
        self._td.cleanup()

    def _create_v1_db(self):
        """Create a minimal v1 database."""
        conn = sqlite3.connect(self.db_path)
        conn.execute("PRAGMA journal_mode=WAL")
        conn.execute("PRAGMA foreign_keys=ON")
        # Create memories table (v1 schema)
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'observation',
                source_type TEXT NOT NULL DEFAULT 'manual',
                source_ref TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'draft',
                confidence TEXT NOT NULL DEFAULT 'medium',
                group_id TEXT NOT NULL,
                scope_key TEXT NOT NULL DEFAULT '',
                actor_id TEXT NOT NULL DEFAULT '',
                task_id TEXT NOT NULL DEFAULT '',
                milestone_id TEXT NOT NULL DEFAULT '',
                event_ts TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                content_hash TEXT NOT NULL DEFAULT '',
                hit_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS memory_relations (
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                relation TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (from_id, to_id, relation)
            );
            CREATE TABLE IF NOT EXISTS memory_tags (
                memory_id TEXT NOT NULL,
                tag TEXT NOT NULL,
                PRIMARY KEY (memory_id, tag)
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                content, content=memories, content_rowid=rowid
            );
            CREATE TRIGGER IF NOT EXISTS memory_fts_insert AFTER INSERT ON memories BEGIN
                INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memory_fts_delete BEFORE DELETE ON memories BEGIN
                INSERT INTO memory_fts(memory_fts, rowid, content)
                VALUES ('delete', old.rowid, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS memory_fts_update AFTER UPDATE OF content ON memories BEGIN
                INSERT INTO memory_fts(memory_fts, rowid, content)
                VALUES ('delete', old.rowid, old.content);
                INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
        """)
        # Insert a test memory
        conn.execute(
            "INSERT INTO memories (id, content, group_id, created_at, updated_at) "
            "VALUES ('m1', 'test memory', ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            (self.group_id,),
        )
        conn.execute(f"PRAGMA user_version = 1")
        conn.commit()
        conn.close()

    def test_v1_to_v2_migration(self):
        """Opening a v1 DB auto-migrates to latest (v3)."""
        self._create_v1_db()
        store = MemoryStore(self.db_path, group_id=self.group_id)
        try:
            assert store._conn is not None
            v = store._conn.execute("PRAGMA user_version").fetchone()[0]
            self.assertEqual(v, 3)
            # memory_meta table exists
            tables = {
                r[0]
                for r in store._conn.execute(
                    "SELECT name FROM sqlite_master WHERE type='table'"
                ).fetchall()
            }
            self.assertIn("memory_meta", tables)
            # Existing data preserved
            mem = store.get("m1")
            self.assertIsNotNone(mem)
            self.assertEqual(mem["content"], "test memory")
        finally:
            store.close()

    def test_v2_db_no_double_migrate(self):
        """Opening a v2 DB doesn't re-run migration."""
        store = MemoryStore(self.db_path, group_id=self.group_id)
        store.set_meta("test_key", "test_value")
        store.close()
        # Re-open — should not lose meta data
        store2 = MemoryStore(self.db_path, group_id=self.group_id)
        try:
            val = store2.get_meta("test_key")
            self.assertEqual(val, "test_value")
        finally:
            store2.close()


class TestMetaCRUD(MemoryMetaTestBase):
    """set_meta / get_meta / delete_meta operations."""

    def test_set_and_get_meta(self):
        """Basic set and get."""
        self.store.set_meta("foo", "bar")
        self.assertEqual(self.store.get_meta("foo"), "bar")

    def test_get_meta_missing_returns_none(self):
        """get_meta returns None for missing keys."""
        self.assertIsNone(self.store.get_meta("nonexistent"))

    def test_get_meta_default(self):
        """get_meta returns default for missing keys when provided."""
        self.assertEqual(self.store.get_meta("nonexistent", "default"), "default")

    def test_set_meta_overwrites(self):
        """set_meta overwrites existing values."""
        self.store.set_meta("key", "val1")
        self.store.set_meta("key", "val2")
        self.assertEqual(self.store.get_meta("key"), "val2")

    def test_delete_meta(self):
        """delete_meta removes the key."""
        self.store.set_meta("key", "val")
        deleted = self.store.delete_meta("key")
        self.assertTrue(deleted)
        self.assertIsNone(self.store.get_meta("key"))

    def test_delete_meta_nonexistent(self):
        """delete_meta returns False for missing key."""
        self.assertFalse(self.store.delete_meta("nonexistent"))

    def test_meta_updated_at_set(self):
        """set_meta records updated_at."""
        self.store.set_meta("key", "val")
        assert self.store._conn is not None
        row = self.store._conn.execute(
            "SELECT updated_at FROM memory_meta WHERE key = ?", ("key",)
        ).fetchone()
        self.assertIsNotNone(row)
        self.assertTrue(row[0].endswith("Z"))  # ISO UTC format


class TestEnumValidation(MemoryMetaTestBase):
    """Enum strong validation (P1 constraint from peer-reviewer)."""

    def test_store_invalid_kind_rejected(self):
        """store() rejects invalid kind."""
        with self.assertRaises(ValueError):
            self.store.store("content", kind="invalid_kind")

    def test_store_invalid_status_rejected(self):
        """store() rejects invalid status."""
        with self.assertRaises(ValueError):
            self.store.store("content", status="invalid_status")

    def test_store_invalid_confidence_rejected(self):
        """store() rejects invalid confidence."""
        with self.assertRaises(ValueError):
            self.store.store("content", confidence="invalid_conf")

    def test_store_invalid_source_type_rejected(self):
        """store() rejects invalid source_type."""
        with self.assertRaises(ValueError):
            self.store.store("content", source_type="invalid_source")

    def test_store_invalid_strategy_rejected(self):
        """store() rejects invalid strategy."""
        with self.assertRaises(ValueError):
            self.store.store("content", strategy="invalid_strat")

    def test_store_valid_enums_accepted(self):
        """store() accepts all valid enum values."""
        for kind in MEMORY_KINDS:
            result = self.store.store(f"content_{kind}", kind=kind)
            self.assertFalse(result.get("deduplicated"))

    def test_update_invalid_kind_rejected(self):
        """update() rejects invalid kind."""
        result = self.store.store("content")
        with self.assertRaises(ValueError):
            self.store.update(result["id"], kind="invalid_kind")

    def test_update_invalid_status_rejected(self):
        """update() rejects invalid status."""
        result = self.store.store("content2")
        with self.assertRaises(ValueError):
            self.store.update(result["id"], status="invalid_status")

    def test_update_invalid_confidence_rejected(self):
        """update() rejects invalid confidence."""
        result = self.store.store("content3")
        with self.assertRaises(ValueError):
            self.store.update(result["id"], confidence="bad_conf")

    def test_update_invalid_source_type_rejected(self):
        """update() rejects invalid source_type."""
        result = self.store.store("content4")
        with self.assertRaises(ValueError):
            self.store.update(result["id"], source_type="invalid_source")

    def test_store_valid_source_type_reflection_accepted(self):
        """store() accepts reflection source_type for existing data compatibility."""
        self.assertIn("reflection", MEMORY_SOURCE_TYPES)
        result = self.store.store("reflection memory", source_type="reflection")
        mem = self.store.get(result["id"])
        self.assertEqual(mem["source_type"], "reflection")

    def test_recall_invalid_kind_rejected(self):
        """recall() rejects invalid kind filter."""
        with self.assertRaises(ValueError):
            self.store.recall(kind="invalid_kind")

    def test_recall_invalid_status_rejected(self):
        """recall() rejects invalid status filter."""
        with self.assertRaises(ValueError):
            self.store.recall(status="bad_status")

    def test_recall_invalid_confidence_rejected(self):
        """recall() rejects invalid confidence filter."""
        with self.assertRaises(ValueError):
            self.store.recall(confidence="bad_conf")


class TestDeduplicationSemantics(MemoryMetaTestBase):
    """Dedup semantics (P2 constraint from peer-reviewer)."""

    def test_same_source_ref_idempotent(self):
        """Same source_ref retry is idempotent (dedup by content_hash)."""
        r1 = self.store.store("same content", source_ref="ref1")
        r2 = self.store.store("same content", source_ref="ref1")
        self.assertTrue(r2.get("deduplicated"))
        self.assertEqual(r1["id"], r2["id"])

    def test_different_source_ref_same_content_not_deduped(self):
        """Different source_ref + same content = separate records (not deduped)."""
        r1 = self.store.store("same content", source_ref="ref_a")
        r2 = self.store.store("same content", source_ref="ref_b")
        # Different source_ref → separate records
        self.assertFalse(r2.get("deduplicated"))
        self.assertNotEqual(r1["id"], r2["id"])

    def test_different_content_different_source_ref_kept(self):
        """Different content with different source_ref → two records."""
        r1 = self.store.store("content alpha", source_ref="ref1")
        r2 = self.store.store("content beta", source_ref="ref2")
        self.assertFalse(r1.get("deduplicated"))
        self.assertFalse(r2.get("deduplicated"))
        self.assertNotEqual(r1["id"], r2["id"])


class TestLastRecalledAt(MemoryMetaTestBase):
    """last_recalled_at column + recall updates it."""

    def test_last_recalled_at_column_exists(self):
        """memories table has last_recalled_at column."""
        assert self.store._conn is not None
        cols = self.store._conn.execute("PRAGMA table_info(memories)").fetchall()
        col_names = {c[1] for c in cols}
        self.assertIn("last_recalled_at", col_names)

    def test_last_recalled_at_index_exists(self):
        """idx_memories_last_recalled_at index exists."""
        assert self.store._conn is not None
        indexes = self.store._conn.execute(
            "SELECT name FROM sqlite_master WHERE type='index' AND name = 'idx_memories_last_recalled_at'"
        ).fetchall()
        self.assertEqual(len(indexes), 1)

    def test_last_recalled_at_default_empty(self):
        """New memory has empty last_recalled_at."""
        r = self.store.store("test content last recalled")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["last_recalled_at"], "")

    def test_recall_updates_last_recalled_at_with_track_hit(self):
        """recall(track_hit=True) updates last_recalled_at on returned memories."""
        r = self.store.store("searchable last recalled content")
        self.store.recall("searchable last recalled", track_hit=True)
        mem = self.store.get(r["id"])
        self.assertNotEqual(mem["last_recalled_at"], "")
        self.assertTrue(mem["last_recalled_at"].endswith("Z"))

    def test_recall_does_not_update_last_recalled_at_by_default(self):
        """recall() without track_hit does NOT update last_recalled_at."""
        r = self.store.store("searchable last recalled default")
        self.store.recall("searchable last recalled")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["last_recalled_at"], "")

    def test_v1_migration_adds_last_recalled_at(self):
        """v1→v2 migration adds last_recalled_at column."""
        self.store.close()
        # Create a v1 DB manually
        conn = sqlite3.connect(self.db_path)
        conn.executescript("""
            DROP TABLE IF EXISTS memory_fts;
            DROP TABLE IF EXISTS memory_tags;
            DROP TABLE IF EXISTS memory_relations;
            DROP TABLE IF EXISTS memories;
            DROP TABLE IF EXISTS memory_meta;
            CREATE TABLE memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'observation',
                source_type TEXT NOT NULL DEFAULT 'manual',
                source_ref TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'draft',
                confidence TEXT NOT NULL DEFAULT 'medium',
                group_id TEXT NOT NULL,
                scope_key TEXT NOT NULL DEFAULT '',
                actor_id TEXT NOT NULL DEFAULT '',
                task_id TEXT NOT NULL DEFAULT '',
                milestone_id TEXT NOT NULL DEFAULT '',
                event_ts TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                content_hash TEXT NOT NULL DEFAULT '',
                hit_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE memory_relations (
                from_id TEXT NOT NULL, to_id TEXT NOT NULL,
                relation TEXT NOT NULL, created_at TEXT NOT NULL,
                PRIMARY KEY (from_id, to_id, relation)
            );
            CREATE TABLE memory_tags (
                memory_id TEXT NOT NULL, tag TEXT NOT NULL,
                PRIMARY KEY (memory_id, tag)
            );
            CREATE VIRTUAL TABLE memory_fts USING fts5(
                content, content=memories, content_rowid=rowid
            );
            CREATE TRIGGER memory_fts_insert AFTER INSERT ON memories BEGIN
                INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            CREATE TRIGGER memory_fts_delete BEFORE DELETE ON memories BEGIN
                INSERT INTO memory_fts(memory_fts, rowid, content)
                VALUES ('delete', old.rowid, old.content);
            END;
            CREATE TRIGGER memory_fts_update AFTER UPDATE OF content ON memories BEGIN
                INSERT INTO memory_fts(memory_fts, rowid, content)
                VALUES ('delete', old.rowid, old.content);
                INSERT INTO memory_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
        """)
        conn.execute(
            "INSERT INTO memories (id, content, group_id, created_at, updated_at) "
            "VALUES ('m1', 'v1 memory', ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            (self.group_id,),
        )
        conn.execute("PRAGMA user_version = 1")
        conn.commit()
        conn.close()
        # Open with MemoryStore — should auto-migrate
        store2 = MemoryStore(self.db_path, group_id=self.group_id)
        try:
            mem = store2.get("m1")
            self.assertIsNotNone(mem)
            self.assertEqual(mem["last_recalled_at"], "")
        finally:
            store2.close()
        self.store = MemoryStore(self.db_path, group_id=self.group_id)


class TestRecallSideEffects(MemoryMetaTestBase):
    """Recall side-effect control (P2 constraint from peer-reviewer).

    Default recall() has NO side effects. track_hit=True enables hit tracking.
    """

    def test_recall_no_side_effects_by_default(self):
        """recall() without track_hit does NOT change hit_count."""
        r = self.store.store("searchable content for recall")
        self.store.recall("searchable content")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["hit_count"], 0)

    def test_recall_increments_hit_count_with_track_hit(self):
        """recall(track_hit=True) increments hit_count on returned memories."""
        r = self.store.store("searchable content for recall")
        self.store.recall("searchable content", track_hit=True)
        mem = self.store.get(r["id"])
        self.assertEqual(mem["hit_count"], 1)

    def test_recall_auto_solidify_at_threshold_with_track_hit(self):
        """Auto-solidify triggers at hit_count >= threshold (with track_hit)."""
        r = self.store.store("auto solidify test content")
        for _ in range(3):
            self.store.recall("auto solidify test", track_hit=True)
        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "solid")

    def test_recall_no_auto_solidify_without_track_hit(self):
        """recall() without track_hit does NOT trigger auto-solidify."""
        r = self.store.store("auto solidify test content default")
        for _ in range(5):
            self.store.recall("auto solidify test")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["status"], "draft")
        self.assertEqual(mem["hit_count"], 0)

    def test_recall_no_query_with_track_hit(self):
        """recall(track_hit=True) without query still increments hit_count."""
        r = self.store.store("content for no-query recall")
        self.store.recall(track_hit=True)  # no query
        mem = self.store.get(r["id"])
        self.assertEqual(mem["hit_count"], 1)


class TestSummaryField(MemoryMetaTestBase):
    """Summary column store/retrieve (T118 Step 5)."""

    def test_store_with_summary(self):
        """store() accepts summary parameter."""
        r = self.store.store("full content here", summary="short summary")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["summary"], "short summary")
        self.assertEqual(mem["content"], "full content here")

    def test_store_without_summary_defaults_empty(self):
        """store() without summary defaults to empty string."""
        r = self.store.store("content without summary")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["summary"], "")

    def test_update_summary(self):
        """update() can set summary on existing memory."""
        r = self.store.store("original content")
        self.store.update(r["id"], summary="added summary")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["summary"], "added summary")
        self.assertEqual(mem["content"], "original content")

    def test_update_summary_to_empty(self):
        """update() can clear summary."""
        r = self.store.store("content", summary="has summary")
        self.store.update(r["id"], summary="")
        mem = self.store.get(r["id"])
        self.assertEqual(mem["summary"], "")


class TestDepthProjection(MemoryMetaTestBase):
    """Depth L0/L2 projection in recall() (T118 Step 5)."""

    def test_recall_default_depth_is_l0(self):
        """recall() default depth is L0."""
        self.store.store("searchable depth default", summary="the summary")
        results = self.store.recall("searchable depth default")
        self.assertTrue(len(results) > 0)
        m = results[0]
        self.assertEqual(m["depth"], "L0")
        self.assertEqual(m["summary"], "the summary")
        self.assertNotIn("content", m)

    def test_recall_l0_strips_content(self):
        """L0 depth strips content, returns summary."""
        self.store.store("full content for l0 test", summary="l0 summary")
        results = self.store.recall("full content for l0 test", depth="L0")
        m = results[0]
        self.assertNotIn("content", m)
        self.assertEqual(m["summary"], "l0 summary")

    def test_recall_l2_keeps_both(self):
        """L2 depth keeps both content and summary."""
        self.store.store("full content for l2 test", summary="l2 summary")
        results = self.store.recall("full content for l2 test", depth="L2")
        m = results[0]
        self.assertEqual(m["content"], "full content for l2 test")
        self.assertEqual(m["summary"], "l2 summary")
        self.assertEqual(m["depth"], "L2")

    def test_recall_l0_empty_summary_fallback_short(self):
        """L0 with empty summary and short content returns content as-is."""
        self.store.store("short content")
        results = self.store.recall("short content", depth="L0")
        m = results[0]
        self.assertEqual(m["summary"], "short content")
        self.assertNotIn("content", m)

    def test_recall_l0_empty_summary_fallback_long(self):
        """L0 with empty summary and long content truncates to 150 chars + ellipsis."""
        long_content = "A" * 200
        self.store.store(long_content, tags=["long_test"])
        results = self.store.recall(tags=["long_test"], depth="L0")
        m = results[0]
        self.assertEqual(len(m["summary"]), 151)  # 150 + ellipsis char
        self.assertTrue(m["summary"].endswith("\u2026"))
        self.assertEqual(m["summary"][:150], "A" * 150)
        self.assertNotIn("content", m)

    def test_recall_invalid_depth_rejected(self):
        """recall() rejects invalid depth value."""
        with self.assertRaises(ValueError):
            self.store.recall(depth="L1")

    def test_recall_l0_no_query_with_summary(self):
        """L0 works with no-query recall (list all)."""
        self.store.store("no query l0 content", summary="nq summary", tags=["nq_l0"])
        results = self.store.recall(tags=["nq_l0"], depth="L0")
        self.assertTrue(len(results) > 0)
        m = results[0]
        self.assertEqual(m["summary"], "nq summary")
        self.assertNotIn("content", m)


class TestSchemaV2ToV3Migration(unittest.TestCase):
    """Migrate v2 DB (no summary column) to v3."""

    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.db_path = os.path.join(self._td.name, "memory.db")
        self.group_id = "g_test_v2_to_v3"

    def tearDown(self):
        self._td.cleanup()

    def _create_v2_db(self):
        """Create a v2 database (has last_recalled_at, memory_meta, but no summary)."""
        conn = sqlite3.connect(self.db_path)
        conn.execute("PRAGMA journal_mode=WAL")
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                kind TEXT NOT NULL DEFAULT 'observation',
                source_type TEXT NOT NULL DEFAULT 'manual',
                source_ref TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'draft',
                confidence TEXT NOT NULL DEFAULT 'medium',
                group_id TEXT NOT NULL,
                scope_key TEXT NOT NULL DEFAULT '',
                actor_id TEXT NOT NULL DEFAULT '',
                task_id TEXT NOT NULL DEFAULT '',
                milestone_id TEXT NOT NULL DEFAULT '',
                event_ts TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                content_hash TEXT NOT NULL DEFAULT '',
                hit_count INTEGER NOT NULL DEFAULT 0,
                last_recalled_at TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS memory_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL DEFAULT '',
                updated_at TEXT NOT NULL DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS memory_relations (
                from_id TEXT NOT NULL, to_id TEXT NOT NULL,
                relation TEXT NOT NULL, created_at TEXT NOT NULL,
                PRIMARY KEY (from_id, to_id, relation)
            );
            CREATE TABLE IF NOT EXISTS memory_tags (
                memory_id TEXT NOT NULL, tag TEXT NOT NULL,
                PRIMARY KEY (memory_id, tag)
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                content, content=memories, content_rowid=rowid
            );
        """)
        conn.execute(
            "INSERT INTO memories (id, content, group_id, created_at, updated_at) "
            "VALUES ('m_v2', 'v2 memory content', ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            (self.group_id,),
        )
        conn.execute("PRAGMA user_version = 2")
        conn.commit()
        conn.close()

    def test_v2_to_v3_migration_adds_summary(self):
        """Opening a v2 DB auto-migrates to v3 with summary column."""
        self._create_v2_db()
        store = MemoryStore(self.db_path, group_id=self.group_id)
        try:
            assert store._conn is not None
            v = store._conn.execute("PRAGMA user_version").fetchone()[0]
            self.assertEqual(v, 3)
            # summary column exists
            cols = store._conn.execute("PRAGMA table_info(memories)").fetchall()
            col_names = {c[1] for c in cols}
            self.assertIn("summary", col_names)
            # Existing data preserved with empty summary
            mem = store.get("m_v2")
            self.assertIsNotNone(mem)
            self.assertEqual(mem["content"], "v2 memory content")
            self.assertEqual(mem["summary"], "")
        finally:
            store.close()


if __name__ == "__main__":
    unittest.main()
