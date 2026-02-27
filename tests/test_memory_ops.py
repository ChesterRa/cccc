"""Tests for daemon memory_ops (T097)."""

from concurrent.futures import ThreadPoolExecutor
import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from unittest.mock import patch, MagicMock

from cccc.contracts.v1 import DaemonResponse
from cccc.daemon.memory.memory_ops import (
    handle_memory_store,
    handle_memory_search,
    handle_memory_stats,
    handle_memory_ingest,
    handle_memory_delete,
    handle_memory_decay,
    try_handle_memory_op,
    _get_memory_store,
    close_all_stores,
    _store_cache,
    _MAX_CACHED_STORES,
)


class MemoryOpsTestBase(unittest.TestCase):
    """Base with a real temp group for memory ops."""

    def setUp(self):
        self._td = tempfile.TemporaryDirectory()
        self.group_id = "g_ops_test"
        # Patch load_group to return a mock group pointing to temp dir
        self._patcher = patch("cccc.daemon.memory.memory_ops.load_group")
        self.mock_load_group = self._patcher.start()
        import pathlib
        mock_group = MagicMock()
        mock_group.path = pathlib.Path(self._td.name)
        mock_group.ledger_path = pathlib.Path(self._td.name) / "ledger.jsonl"
        self.mock_load_group.return_value = mock_group

    def tearDown(self):
        close_all_stores()
        self._patcher.stop()
        self._td.cleanup()


class TestConnectionPoolCache(MemoryOpsTestBase):
    """Test _get_memory_store connection pool."""

    def test_get_store_creates_new(self):
        store = _get_memory_store(self.group_id)
        self.assertIsNotNone(store)
        self.assertIn(self.group_id, _store_cache)

    def test_get_store_returns_cached(self):
        store1 = _get_memory_store(self.group_id)
        store2 = _get_memory_store(self.group_id)
        self.assertIs(store1, store2)

    def test_get_store_none_for_unknown_group(self):
        self.mock_load_group.return_value = None
        store = _get_memory_store("nonexistent")
        self.assertIsNone(store)

    def test_lru_eviction(self):
        """Evicts oldest when cache is full."""
        for i in range(_MAX_CACHED_STORES):
            gid = f"g_evict_{i}"
            _get_memory_store(gid)
        self.assertEqual(len(_store_cache), _MAX_CACHED_STORES)

        # One more should evict the first
        _get_memory_store("g_evict_overflow")
        self.assertEqual(len(_store_cache), _MAX_CACHED_STORES)
        self.assertNotIn("g_evict_0", _store_cache)
        self.assertIn("g_evict_overflow", _store_cache)

    def test_close_all_stores(self):
        _get_memory_store(self.group_id)
        self.assertGreater(len(_store_cache), 0)
        close_all_stores()
        self.assertEqual(len(_store_cache), 0)

    def test_concurrent_get_same_group_returns_single_cached_instance(self):
        """Thread-safe cache access: concurrent gets should converge to one instance."""
        close_all_stores()
        with patch("cccc.daemon.memory.memory_ops.MemoryStore") as mock_store_cls:
            mock_store_cls.side_effect = lambda *_a, **_k: MagicMock()
            with ThreadPoolExecutor(max_workers=8) as pool:
                stores = list(pool.map(lambda _: _get_memory_store(self.group_id), range(16)))
        self.assertTrue(all(s is not None for s in stores))
        first = stores[0]
        self.assertTrue(all(s is first for s in stores))
        self.assertEqual(len(_store_cache), 1)


class TestHandleMemoryStore(MemoryOpsTestBase):
    """Test handle_memory_store create/update/solidify."""

    def test_missing_group_id(self):
        resp = handle_memory_store({})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "missing_group_id")

    def test_group_not_found(self):
        self.mock_load_group.return_value = None
        resp = handle_memory_store({"group_id": "unknown"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "group_not_found")

    def test_create_basic(self):
        resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "test memory",
        })
        self.assertTrue(resp.ok)
        self.assertIn("id", resp.result)
        self.assertFalse(resp.result.get("deduplicated"))

    def test_create_missing_content(self):
        resp = handle_memory_store({"group_id": self.group_id})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "missing_content")

    def test_create_with_all_fields(self):
        resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "detailed memory",
            "kind": "decision",
            "status": "solid",
            "confidence": "high",
            "source_type": "chat_ingest",
            "source_ref": "evt_123",
            "scope_key": "s_test",
            "actor_id": "peer-impl",
            "task_id": "T097",
            "milestone_id": "M7",
            "event_ts": "2026-02-25T00:00:00Z",
            "tags": ["test", "memory"],
            "strategy": "aggressive",
        })
        self.assertTrue(resp.ok)

    def test_create_with_solidify(self):
        resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "solidify on create",
            "solidify": True,
        })
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result.get("status"), "solid")

    def test_update_existing(self):
        # Create first
        create_resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "original",
        })
        self.assertTrue(create_resp.ok)
        mem_id = create_resp.result["id"]

        # Update
        update_resp = handle_memory_store({
            "group_id": self.group_id,
            "id": mem_id,
            "content": "modified",
            "kind": "fact",
        })
        self.assertTrue(update_resp.ok)
        self.assertTrue(update_resp.result.get("updated"))
        self.assertEqual(update_resp.result["memory"]["content"], "modified")
        self.assertEqual(update_resp.result["memory"]["kind"], "fact")

    def test_update_nonexistent(self):
        resp = handle_memory_store({
            "group_id": self.group_id,
            "id": "nonexistent_id",
            "content": "fail",
        })
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "memory_not_found")

    def test_update_with_solidify(self):
        create_resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "to solidify",
        })
        mem_id = create_resp.result["id"]

        update_resp = handle_memory_store({
            "group_id": self.group_id,
            "id": mem_id,
            "solidify": True,
        })
        self.assertTrue(update_resp.ok)
        self.assertEqual(update_resp.result["memory"]["status"], "solid")

    def test_update_tags(self):
        create_resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "tagged",
            "tags": ["a", "b"],
        })
        mem_id = create_resp.result["id"]

        update_resp = handle_memory_store({
            "group_id": self.group_id,
            "id": mem_id,
            "tags": ["c", "d"],
        })
        self.assertTrue(update_resp.ok)
        self.assertEqual(sorted(update_resp.result["memory"]["tags"]), ["c", "d"])

    def test_dedup(self):
        resp1 = handle_memory_store({
            "group_id": self.group_id,
            "content": "same content",
        })
        resp2 = handle_memory_store({
            "group_id": self.group_id,
            "content": "same content",
        })
        self.assertTrue(resp1.ok)
        self.assertTrue(resp2.ok)
        self.assertTrue(resp2.result.get("deduplicated"))
        self.assertEqual(resp1.result["id"], resp2.result["id"])

    def test_create_invalid_enum_returns_validation_error(self):
        resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "bad enum",
            "kind": "INVALID_KIND",
        })
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "validation_error")

    def test_update_invalid_enum_returns_validation_error(self):
        create_resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "original",
        })
        mem_id = create_resp.result["id"]
        resp = handle_memory_store({
            "group_id": self.group_id,
            "id": mem_id,
            "status": "INVALID_STATUS",
        })
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "validation_error")

    def test_create_invalid_source_type_returns_validation_error(self):
        resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "bad source type",
            "source_type": "INVALID_SOURCE",
        })
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "validation_error")

    def test_update_invalid_source_type_returns_validation_error(self):
        create_resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "original source type",
        })
        mem_id = create_resp.result["id"]
        resp = handle_memory_store({
            "group_id": self.group_id,
            "id": mem_id,
            "source_type": "INVALID_SOURCE",
        })
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "validation_error")


class TestHandleMemorySearch(MemoryOpsTestBase):
    """Test handle_memory_search."""

    def test_missing_group_id(self):
        resp = handle_memory_search({})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "missing_group_id")

    def test_search_empty(self):
        resp = handle_memory_search({"group_id": self.group_id})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["count"], 0)
        self.assertEqual(resp.result["memories"], [])

    def test_search_with_data(self):
        handle_memory_store({
            "group_id": self.group_id,
            "content": "SQLite full-text search",
            "kind": "fact",
        })
        resp = handle_memory_search({
            "group_id": self.group_id,
            "query": "SQLite",
        })
        self.assertTrue(resp.ok)
        self.assertGreater(resp.result["count"], 0)

    def test_search_with_filters(self):
        handle_memory_store({
            "group_id": self.group_id,
            "content": "solid fact",
            "status": "solid",
            "kind": "fact",
        })
        handle_memory_store({
            "group_id": self.group_id,
            "content": "draft observation",
            "kind": "observation",
        })

        resp = handle_memory_search({
            "group_id": self.group_id,
            "status": "solid",
        })
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["count"], 1)

    def test_search_with_limit(self):
        for i in range(5):
            handle_memory_store({
                "group_id": self.group_id,
                "content": f"memory {i}",
            })
        resp = handle_memory_search({
            "group_id": self.group_id,
            "limit": 2,
        })
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["count"], 2)

    def test_search_with_tags(self):
        handle_memory_store({
            "group_id": self.group_id,
            "content": "tagged memory",
            "tags": ["architecture", "search"],
        })
        resp = handle_memory_search({
            "group_id": self.group_id,
            "tags": ["architecture"],
        })
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["count"], 1)

    def test_search_limit_clamped(self):
        """Limit is clamped to 1-100."""
        resp = handle_memory_search({
            "group_id": self.group_id,
            "limit": 999,
        })
        self.assertTrue(resp.ok)

    def test_search_memory_dict_shape(self):
        """Search results have normalized memory shape (L2 includes all fields)."""
        handle_memory_store({
            "group_id": self.group_id,
            "content": "shape check",
        })
        resp = handle_memory_search({"group_id": self.group_id, "depth": "L2"})
        self.assertTrue(resp.ok)
        mem = resp.result["memories"][0]
        for key in ("id", "content", "kind", "status", "confidence",
                     "source_type", "source_ref", "group_id", "scope_key",
                     "actor_id", "task_id", "milestone_id", "event_ts",
                     "created_at", "updated_at", "last_recalled_at",
                     "content_hash", "hit_count", "tags", "summary", "depth"):
            self.assertIn(key, mem, f"Missing key: {key}")

    def test_search_invalid_enum_returns_validation_error(self):
        resp = handle_memory_search({
            "group_id": self.group_id,
            "status": "INVALID_STATUS",
        })
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "validation_error")

    def test_search_invalid_limit_returns_validation_error(self):
        resp = handle_memory_search({
            "group_id": self.group_id,
            "limit": "abc",
        })
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "validation_error")

    def test_search_track_hit_default_no_side_effect(self):
        create_resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "track hit default",
        })
        mem_id = create_resp.result["id"]
        before = _get_memory_store(self.group_id).get(mem_id)
        resp = handle_memory_search({
            "group_id": self.group_id,
            "query": "track hit default",
        })
        self.assertTrue(resp.ok)
        after = _get_memory_store(self.group_id).get(mem_id)
        self.assertEqual(before["hit_count"], after["hit_count"])
        self.assertEqual(before["last_recalled_at"], after["last_recalled_at"])

    def test_search_track_hit_true_updates_hit_count(self):
        create_resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "track hit true",
        })
        mem_id = create_resp.result["id"]
        resp = handle_memory_search({
            "group_id": self.group_id,
            "query": "track hit true",
            "track_hit": True,
        })
        self.assertTrue(resp.ok)
        mem = _get_memory_store(self.group_id).get(mem_id)
        self.assertEqual(mem["hit_count"], 1)


class TestHandleMemoryStats(MemoryOpsTestBase):
    """Test handle_memory_stats."""

    def test_missing_group_id(self):
        resp = handle_memory_stats({})
        self.assertFalse(resp.ok)

    def test_stats_empty(self):
        resp = handle_memory_stats({"group_id": self.group_id})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["total"], 0)

    def test_stats_with_data(self):
        handle_memory_store({
            "group_id": self.group_id,
            "content": "a",
            "kind": "fact",
            "status": "solid",
        })
        handle_memory_store({
            "group_id": self.group_id,
            "content": "b",
            "kind": "observation",
        })
        resp = handle_memory_stats({"group_id": self.group_id})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["total"], 2)


class TestHandleMemoryIngest(MemoryOpsTestBase):
    """Test handle_memory_ingest basic dispatch."""

    def test_ingest_works(self):
        resp = handle_memory_ingest({"group_id": self.group_id})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["mode"], "signal")


class TestHandleMemoryDelete(MemoryOpsTestBase):
    """Test handle_memory_delete (single + batch)."""

    def test_missing_group_id(self):
        resp = handle_memory_delete({})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "missing_group_id")

    def test_missing_id_or_ids(self):
        resp = handle_memory_delete({"group_id": self.group_id})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "missing_id")

    def test_delete_single(self):
        created = handle_memory_store({"group_id": self.group_id, "content": "to delete"})
        mem_id = created.result["id"]
        resp = handle_memory_delete({"group_id": self.group_id, "id": mem_id})
        self.assertTrue(resp.ok)
        self.assertTrue(resp.result["deleted"])
        self.assertEqual(resp.result["deleted_count"], 1)
        self.assertEqual(resp.result["ids"], [mem_id])

    def test_delete_batch(self):
        m1 = handle_memory_store({"group_id": self.group_id, "content": "d1"}).result["id"]
        m2 = handle_memory_store({"group_id": self.group_id, "content": "d2"}).result["id"]
        resp = handle_memory_delete({"group_id": self.group_id, "ids": [m1, "missing", m2]})
        self.assertTrue(resp.ok)
        self.assertTrue(resp.result["deleted"])
        self.assertEqual(resp.result["deleted_count"], 2)
        self.assertEqual(resp.result["ids"], [m1, m2])

    def test_delete_batch_invalid_ids_type(self):
        resp = handle_memory_delete({"group_id": self.group_id, "ids": "not-a-list"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "validation_error")


class TestHandleMemoryDecay(MemoryOpsTestBase):
    """Test handle_memory_decay."""

    def _age(self, memory_id: str, *, days: int) -> None:
        store = _get_memory_store(self.group_id)
        assert store is not None and store._conn is not None
        ts = (datetime.now(timezone.utc) - timedelta(days=days)).isoformat().replace("+00:00", "Z")
        store._conn.execute(
            "UPDATE memories SET created_at = ?, updated_at = ?, last_recalled_at = '' "
            "WHERE id = ? AND group_id = ?",
            (ts, ts, memory_id, self.group_id),
        )
        store._conn.commit()

    def test_missing_group_id(self):
        resp = handle_memory_decay({})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "missing_group_id")

    def test_decay_returns_candidates(self):
        created = handle_memory_store({"group_id": self.group_id, "content": "stale draft"})
        mem_id = created.result["id"]
        self._age(mem_id, days=60)
        resp = handle_memory_decay({"group_id": self.group_id, "draft_days": 30, "zero_hit_days": 14})
        self.assertTrue(resp.ok)
        ids = [c["id"] for c in resp.result["candidates"]]
        self.assertIn(mem_id, ids)

    def test_decay_invalid_param_returns_validation_error(self):
        resp = handle_memory_decay({"group_id": self.group_id, "limit": "bad"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "validation_error")


class TestTryHandleMemoryOp(MemoryOpsTestBase):
    """Test the dispatcher."""

    def test_dispatches_memory_store(self):
        resp = try_handle_memory_op("memory_store", {
            "group_id": self.group_id,
            "content": "dispatch test",
        })
        self.assertIsNotNone(resp)
        self.assertTrue(resp.ok)

    def test_dispatches_memory_search(self):
        resp = try_handle_memory_op("memory_search", {
            "group_id": self.group_id,
        })
        self.assertIsNotNone(resp)
        self.assertTrue(resp.ok)

    def test_dispatches_memory_stats(self):
        resp = try_handle_memory_op("memory_stats", {
            "group_id": self.group_id,
        })
        self.assertIsNotNone(resp)
        self.assertTrue(resp.ok)

    def test_dispatches_memory_ingest(self):
        resp = try_handle_memory_op("memory_ingest", {
            "group_id": self.group_id,
        })
        self.assertIsNotNone(resp)
        self.assertTrue(resp.ok)

    def test_dispatches_memory_delete(self):
        created = handle_memory_store({"group_id": self.group_id, "content": "dispatch delete"})
        mem_id = created.result["id"]
        resp = try_handle_memory_op("memory_delete", {
            "group_id": self.group_id,
            "id": mem_id,
        })
        self.assertIsNotNone(resp)
        self.assertTrue(resp.ok)

    def test_dispatches_memory_decay(self):
        resp = try_handle_memory_op("memory_decay", {"group_id": self.group_id})
        self.assertIsNotNone(resp)
        self.assertTrue(resp.ok)

    def test_returns_none_for_unknown_op(self):
        resp = try_handle_memory_op("unknown_op", {})
        self.assertIsNone(resp)

    def test_returns_none_for_non_memory_op(self):
        resp = try_handle_memory_op("context_get", {"group_id": "test"})
        self.assertIsNone(resp)


class TestSummaryOps(MemoryOpsTestBase):
    """Summary field via daemon ops (T118 Step 5)."""

    def test_store_with_summary(self):
        resp = handle_memory_store({
            "group_id": self.group_id,
            "content": "ops summary content",
            "summary": "ops short summary",
        })
        self.assertTrue(resp.ok)
        mem = _get_memory_store(self.group_id).get(resp.result["id"])
        self.assertEqual(mem["summary"], "ops short summary")

    def test_update_summary(self):
        create = handle_memory_store({
            "group_id": self.group_id,
            "content": "update summary content",
        })
        resp = handle_memory_store({
            "group_id": self.group_id,
            "id": create.result["id"],
            "summary": "updated summary",
        })
        self.assertTrue(resp.ok)
        mem = _get_memory_store(self.group_id).get(create.result["id"])
        self.assertEqual(mem["summary"], "updated summary")

    def test_search_dict_shape_includes_summary(self):
        handle_memory_store({
            "group_id": self.group_id,
            "content": "shape summary check",
            "summary": "shape sum",
        })
        resp = handle_memory_search({"group_id": self.group_id, "depth": "L2"})
        self.assertTrue(resp.ok)
        mem = resp.result["memories"][0]
        self.assertIn("summary", mem)
        self.assertEqual(mem["summary"], "shape sum")


class TestDepthOps(MemoryOpsTestBase):
    """Depth L0/L2 via daemon ops (T118 Step 5)."""

    def test_search_default_depth_l0(self):
        """Default search returns L0 (summary, no content)."""
        handle_memory_store({
            "group_id": self.group_id,
            "content": "depth default test",
            "summary": "depth default summary",
        })
        resp = handle_memory_search({
            "group_id": self.group_id,
            "query": "depth default test",
        })
        self.assertTrue(resp.ok)
        mem = resp.result["memories"][0]
        self.assertEqual(mem["depth"], "L0")
        self.assertEqual(mem["summary"], "depth default summary")
        self.assertNotIn("content", mem)

    def test_search_depth_l2_returns_content(self):
        """depth=L2 search returns both content and summary."""
        handle_memory_store({
            "group_id": self.group_id,
            "content": "l2 depth content",
            "summary": "l2 depth summary",
        })
        resp = handle_memory_search({
            "group_id": self.group_id,
            "query": "l2 depth content",
            "depth": "L2",
        })
        self.assertTrue(resp.ok)
        mem = resp.result["memories"][0]
        self.assertEqual(mem["depth"], "L2")
        self.assertEqual(mem["content"], "l2 depth content")
        self.assertEqual(mem["summary"], "l2 depth summary")

    def test_search_l0_empty_summary_fallback(self):
        """L0 with empty summary falls back to truncated content."""
        long_content = "B" * 200
        handle_memory_store({
            "group_id": self.group_id,
            "content": long_content,
        })
        resp = handle_memory_search({
            "group_id": self.group_id,
            "depth": "L0",
        })
        self.assertTrue(resp.ok)
        mem = resp.result["memories"][0]
        self.assertEqual(mem["depth"], "L0")
        self.assertTrue(mem["summary"].endswith("\u2026"))
        self.assertEqual(len(mem["summary"]), 151)
        self.assertNotIn("content", mem)

    def test_search_invalid_depth_returns_error(self):
        """Invalid depth returns validation error."""
        resp = handle_memory_search({
            "group_id": self.group_id,
            "depth": "L1",
        })
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "validation_error")


if __name__ == "__main__":
    unittest.main()
