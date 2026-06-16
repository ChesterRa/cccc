from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestMemoryRemeOps(unittest.TestCase):
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

    def _create_group(self, title: str = "reme-test") -> str:
        resp, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        gid = str((resp.result or {}).get("group_id") or "")
        self.assertTrue(gid)
        return gid

    def test_layout_write_search_get_flow(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()

            layout_resp, _ = self._call("memory_reme_layout_get", {"group_id": gid})
            self.assertTrue(layout_resp.ok, getattr(layout_resp, "error", None))
            layout = layout_resp.result if isinstance(layout_resp.result, dict) else {}
            daily_file = Path(str(layout.get("today_daily_file") or ""))
            self.assertTrue(daily_file.exists())

            write_resp, _ = self._call(
                "memory_reme_write",
                {
                    "group_id": gid,
                    "target": "daily",
                    "date": daily_file.name.split("__")[0],
                    "content": "Completed migration checklist and validated search path.",
                    "idempotency_key": "t_memory_reme_ops_daily_write",
                },
            )
            self.assertTrue(write_resp.ok, getattr(write_resp, "error", None))

            search_resp, _ = self._call(
                "memory_reme_search",
                {"group_id": gid, "query": "migration checklist", "max_results": 5, "min_score": 0.01},
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            result = search_resp.result if isinstance(search_resp.result, dict) else {}
            hits = result.get("hits") if isinstance(result.get("hits"), list) else []
            self.assertGreaterEqual(len(hits), 1)
            first = hits[0] if isinstance(hits[0], dict) else {}
            path = str(first.get("path") or "")
            self.assertTrue(path.endswith(".md"))

            get_resp, _ = self._call("memory_reme_get", {"group_id": gid, "path": path, "offset": 1, "limit": 40})
            self.assertTrue(get_resp.ok, getattr(get_resp, "error", None))
            payload = get_resp.result if isinstance(get_resp.result, dict) else {}
            content = str(payload.get("content") or "")
            self.assertIn("migration checklist", content)
        finally:
            cleanup()

    def test_first_class_memory_ops_adapt_reme_for_sdk_clients(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("sdk-memory")

            health_resp, _ = self._call("memory_health", {"group_id": gid})
            self.assertTrue(health_resp.ok, getattr(health_resp, "error", None))
            health = health_resp.result if isinstance(health_resp.result, dict) else {}
            self.assertEqual(health.get("provider"), "cccc-memory")
            self.assertEqual(health.get("source"), "local-index")
            self.assertEqual(health.get("status"), "ok")
            self.assertTrue(health.get("indexReady"))
            self.assertTrue(health.get("writable"))
            self.assertTrue(str(health.get("memoryRoot") or "").endswith("state/memory"))
            self.assertIsInstance(health.get("latencyMs"), int)

            write_resp, _ = self._call(
                "memory_write",
                {
                    "group_id": gid,
                    "actor_id": "dingtalk-worker",
                    "target": "daily",
                    "content": "DingTalk reply profile prefers concise Chinese answers.",
                    "tags": ["dingtalk-profile", "reply-style"],
                    "source_refs": ["message:m1"],
                    "idempotency_key": "sdk-memory-write-m1",
                },
            )
            self.assertTrue(write_resp.ok, getattr(write_resp, "error", None))
            write_payload = write_resp.result if isinstance(write_resp.result, dict) else {}
            self.assertEqual(write_payload.get("provider"), "cccc-memory")
            self.assertEqual(write_payload.get("source"), "local-file")
            self.assertEqual(write_payload.get("status"), "written")
            path = str(write_payload.get("path") or "")
            self.assertTrue(path.endswith(".md"))
            self.assertTrue(write_payload.get("contentHash"))

            search_resp, _ = self._call(
                "memory_search",
                {
                    "group_id": gid,
                    "actor_id": "dingtalk-worker",
                    "query": "concise Chinese reply profile",
                    "limit": 5,
                    "target": "daily",
                    "tags": ["reply-style"],
                },
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            search_payload = search_resp.result if isinstance(search_resp.result, dict) else {}
            self.assertEqual(search_payload.get("provider"), "cccc-memory")
            self.assertEqual(search_payload.get("source"), "local-index")
            hits = search_payload.get("hits") if isinstance(search_payload.get("hits"), list) else []
            self.assertGreaterEqual(len(hits), 1)
            first = hits[0] if isinstance(hits[0], dict) else {}
            self.assertIn("startLine", first)
            self.assertIn("sourceRefs", first)

            get_resp, _ = self._call("memory_get", {"group_id": gid, "path": path, "offset": 1, "limit": 60})
            self.assertTrue(get_resp.ok, getattr(get_resp, "error", None))
            get_payload = get_resp.result if isinstance(get_resp.result, dict) else {}
            self.assertEqual(get_payload.get("provider"), "cccc-memory")
            self.assertEqual(get_payload.get("source"), "local-file")
            self.assertEqual(get_payload.get("path"), path)
            self.assertIn("DingTalk reply profile", str(get_payload.get("content") or ""))

            profile_resp, _ = self._call(
                "memory_profile_get",
                {
                    "group_id": gid,
                    "actor_id": "dingtalk-worker",
                    "user_id": "waterbang",
                    "tags": ["dingtalk-profile", "reply-style"],
                },
            )
            self.assertTrue(profile_resp.ok, getattr(profile_resp, "error", None))
            profile_payload = profile_resp.result if isinstance(profile_resp.result, dict) else {}
            self.assertEqual(profile_payload.get("provider"), "cccc-memory")
            self.assertEqual(profile_payload.get("source"), "local-index")
            self.assertIn("DingTalk reply profile", str(profile_payload.get("profile") or ""))
            profile_hits = profile_payload.get("hits") if isinstance(profile_payload.get("hits"), list) else []
            self.assertGreaterEqual(len(profile_hits), 1)
        finally:
            cleanup()

    def test_memory_search_tag_filter_matches_long_entry_tail_chunks(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("sdk-memory-long-entry")
            filler = "\n".join(f"Filler line {i}: ordinary project context without the search token." for i in range(90))
            tail_marker = "tail-only-calibration-marker"

            write_resp, _ = self._call(
                "memory_write",
                {
                    "group_id": gid,
                    "actor_id": "memory-worker",
                    "target": "daily",
                    "content": f"{filler}\nFinal detail: {tail_marker} belongs to the tagged long entry.",
                    "tags": ["long-entry-tag"],
                    "source_refs": ["message:tail"],
                    "idempotency_key": "sdk-memory-long-entry-tail",
                },
            )
            self.assertTrue(write_resp.ok, getattr(write_resp, "error", None))

            search_resp, _ = self._call(
                "memory_search",
                {
                    "group_id": gid,
                    "actor_id": "memory-worker",
                    "query": tail_marker,
                    "limit": 5,
                    "target": "daily",
                    "tags": ["long-entry-tag"],
                },
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            payload = search_resp.result if isinstance(search_resp.result, dict) else {}
            hits = payload.get("hits") if isinstance(payload.get("hits"), list) else []
            self.assertGreaterEqual(len(hits), 1)
            first = hits[0] if isinstance(hits[0], dict) else {}
            self.assertIn("long-entry-tag", first.get("tags") or [])
            self.assertIn("message:tail", first.get("sourceRefs") or [])
        finally:
            cleanup()

    def test_memory_search_cjk_query_without_spaces_matches_memory(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("sdk-memory-cjk")

            write_resp, _ = self._call(
                "memory_write",
                {
                    "group_id": gid,
                    "actor_id": "memory-worker",
                    "target": "daily",
                    "content": "偏好：回答应保持简洁，并使用中文。",
                    "tags": ["reply-style"],
                    "source_refs": ["message:cjk"],
                    "idempotency_key": "sdk-memory-cjk-reply-style",
                },
            )
            self.assertTrue(write_resp.ok, getattr(write_resp, "error", None))

            search_resp, _ = self._call(
                "memory_search",
                {
                    "group_id": gid,
                    "actor_id": "memory-worker",
                    "query": "中文简洁回复",
                    "limit": 5,
                    "target": "daily",
                    "tags": ["reply-style"],
                },
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            payload = search_resp.result if isinstance(search_resp.result, dict) else {}
            hits = payload.get("hits") if isinstance(payload.get("hits"), list) else []
            self.assertGreaterEqual(len(hits), 1)
            first = hits[0] if isinstance(hits[0], dict) else {}
            self.assertIn("reply-style", first.get("tags") or [])
            self.assertIn("message:cjk", first.get("sourceRefs") or [])
        finally:
            cleanup()

    def test_local_keyword_search_respects_latin_token_boundaries(self) -> None:
        import asyncio

        from cccc.vendor.reme.core.enumeration import MemorySource
        from cccc.vendor.reme.core.file_store import LocalFileStore
        from cccc.vendor.reme.core.schema import FileMetadata, MemoryChunk
        from cccc.vendor.reme.core.utils.common_utils import hash_text

        async def _scenario(db_path: Path):
            store = LocalFileStore(
                store_name="token_boundary",
                db_path=db_path,
                vector_enabled=False,
                fts_enabled=True,
            )
            await store.start()
            chunks = [
                MemoryChunk(
                    id="capillary",
                    path="/tmp/memory.md",
                    source=MemorySource.MEMORY,
                    start_line=1,
                    end_line=1,
                    text="Capillary calibration notes only.",
                    hash=hash_text("capillary"),
                ),
                MemoryChunk(
                    id="api",
                    path="/tmp/memory.md",
                    source=MemorySource.MEMORY,
                    start_line=2,
                    end_line=2,
                    text="Public API contract remains stable.",
                    hash=hash_text("api"),
                ),
            ]
            await store.upsert_file(
                FileMetadata(
                    hash=hash_text("token-boundary"),
                    mtime_ms=1,
                    size=1,
                    path="/tmp/memory.md",
                    chunk_count=len(chunks),
                ),
                MemorySource.MEMORY,
                chunks,
            )
            hits = await store.keyword_search("api", 5, [MemorySource.MEMORY])
            await store.close()
            return hits

        with tempfile.TemporaryDirectory() as td:
            hits = asyncio.run(_scenario(Path(td)))

        self.assertEqual(len(hits), 1)
        self.assertIn("Public API contract", hits[0].snippet)

    def test_local_keyword_search_keeps_low_coverage_match_below_tight_threshold(self) -> None:
        import asyncio

        from cccc.vendor.reme.core.enumeration import MemorySource
        from cccc.vendor.reme.core.file_store import LocalFileStore
        from cccc.vendor.reme.core.schema import FileMetadata, MemoryChunk
        from cccc.vendor.reme.core.utils.common_utils import hash_text

        async def _scenario(db_path: Path):
            store = LocalFileStore(
                store_name="keyword_low_coverage",
                db_path=db_path,
                vector_enabled=False,
                fts_enabled=True,
            )
            await store.start()
            await store.upsert_file(
                FileMetadata(
                    hash=hash_text("keyword-low-coverage"),
                    mtime_ms=1,
                    size=1,
                    path="/tmp/memory.md",
                    chunk_count=1,
                ),
                MemorySource.MEMORY,
                [
                    MemoryChunk(
                        id="project-only",
                        path="/tmp/memory.md",
                        source=MemorySource.MEMORY,
                        start_line=1,
                        end_line=1,
                        text="Project status note without the requested detail.",
                        hash=hash_text("project-only"),
                    )
                ],
            )
            hits = await store.keyword_search(
                "project alpha beta gamma delta epsilon",
                5,
                [MemorySource.MEMORY],
            )
            await store.close()
            return hits

        with tempfile.TemporaryDirectory() as td:
            hits = asyncio.run(_scenario(Path(td)))

        self.assertEqual(len(hits), 1)
        self.assertLess(hits[0].score, 0.6)

    def test_memory_search_daily_filter_accepts_windows_paths(self) -> None:
        from cccc.contracts.v1 import DaemonResponse
        from cccc.daemon.memory.memory_sdk_ops import handle_memory_search

        with patch(
            "cccc.daemon.memory.memory_ops.handle_memory_reme_search",
            return_value=DaemonResponse(
                ok=True,
                result={
                    "hits": [
                        {
                            "path": r"C:\state\memory\daily\2026-06-07__group.md",
                            "start_line": 1,
                            "score": 1.0,
                            "snippet": "Windows daily memory hit",
                        }
                    ]
                },
            ),
        ):
            resp = handle_memory_search({"group_id": "g1", "query": "daily", "target": "daily"})

        self.assertTrue(resp.ok, getattr(resp, "error", None))
        payload = resp.result if isinstance(resp.result, dict) else {}
        hits = payload.get("hits") if isinstance(payload.get("hits"), list) else []
        self.assertEqual(len(hits), 1)

    def test_memory_search_forwards_caller_recall_controls(self) -> None:
        from cccc.contracts.v1 import DaemonResponse
        from cccc.daemon.memory.memory_sdk_ops import handle_memory_search

        captured: dict = {}

        def _capture(reme_args):
            captured.update(reme_args)
            return DaemonResponse(ok=True, result={"hits": []})

        with patch("cccc.daemon.memory.memory_ops.handle_memory_reme_search", side_effect=_capture):
            handle_memory_search(
                {
                    "group_id": "g1",
                    "query": "q",
                    "min_score": 0.6,
                    "max_results": 12,
                    "vector_weight": 0.7,
                    "candidate_multiplier": 3,
                }
            )

        self.assertEqual(captured.get("min_score"), 0.6)
        self.assertEqual(captured.get("max_results"), 12)
        self.assertEqual(captured.get("vector_weight"), 0.7)
        self.assertEqual(captured.get("candidate_multiplier"), 3)

    def test_memory_search_defaults_low_min_score_and_limit_alias(self) -> None:
        from cccc.contracts.v1 import DaemonResponse
        from cccc.daemon.memory.memory_sdk_ops import handle_memory_search

        captured: dict = {}

        def _capture(reme_args):
            captured.update(reme_args)
            return DaemonResponse(ok=True, result={"hits": []})

        with patch("cccc.daemon.memory.memory_ops.handle_memory_reme_search", side_effect=_capture):
            handle_memory_search({"group_id": "g1", "query": "q", "limit": 5})

        # No caller min_score: keep the low default so tag/target post-filtering has candidates.
        self.assertEqual(captured.get("min_score"), 0.01)
        # `limit` is the SDK alias for max_results when max_results is absent.
        self.assertEqual(captured.get("max_results"), 5)
        self.assertNotIn("vector_weight", captured)
        self.assertNotIn("candidate_multiplier", captured)

    def test_context_check_compact_daily_flush(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("reme-compact")
            messages = []
            for i in range(80):
                role = "user" if i % 2 == 0 else "assistant"
                messages.append({"role": role, "content": f"turn {i} " + ("x" * 200)})

            check_resp, _ = self._call(
                "memory_reme_context_check",
                {
                    "group_id": gid,
                    "messages": messages,
                    "context_window_tokens": 3000,
                    "reserve_tokens": 200,
                    "keep_recent_tokens": 500,
                },
            )
            self.assertTrue(check_resp.ok, getattr(check_resp, "error", None))
            check = check_resp.result if isinstance(check_resp.result, dict) else {}
            self.assertTrue(bool(check.get("needs_compaction")))

            compact_resp, _ = self._call(
                "memory_reme_compact",
                {
                    "group_id": gid,
                    "messages_to_summarize": check.get("messages_to_summarize") or [],
                    "turn_prefix_messages": check.get("turn_prefix_messages") or [],
                    "previous_summary": "",
                },
            )
            self.assertTrue(compact_resp.ok, getattr(compact_resp, "error", None))
            compact_payload = compact_resp.result if isinstance(compact_resp.result, dict) else {}
            self.assertTrue(str(compact_payload.get("summary") or "").strip())

            flush_resp, _ = self._call(
                "memory_reme_daily_flush",
                {"group_id": gid, "messages": messages[:10], "language": "en"},
            )
            self.assertTrue(flush_resp.ok, getattr(flush_resp, "error", None))
            flush = flush_resp.result if isinstance(flush_resp.result, dict) else {}
            self.assertEqual(str(flush.get("status") or ""), "written")
        finally:
            cleanup()

    def test_auto_conversation_cycle_writes_then_silences_duplicate(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("reme-auto-cycle")
            from cccc.kernel.group import load_group
            from cccc.kernel.ledger import append_event
            from cccc.daemon.memory.memory_ops import run_auto_conversation_memory_cycle

            # Seed context signals for signal_pack (coordination brief + task + agent state).
            seed_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [
                        {"op": "coordination.brief.update", "objective": "Ship reliable memory lifecycle.", "current_focus": "Memory lane"},
                        {"op": "task.create", "title": "Memory Lane", "outcome": "Auto flush with low noise"},
                    ],
                },
            )
            self.assertTrue(seed_resp.ok, getattr(seed_resp, "error", None))
            agent_seed_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "peer1",
                    "ops": [
                        {
                            "op": "agent_state.update",
                            "actor_id": "peer1",
                            "focus": "memory lane",
                            "next_action": "validate auto flush",
                            "what_changed": "seeded",
                        }
                    ],
                },
            )
            self.assertTrue(agent_seed_resp.ok, getattr(agent_seed_resp, "error", None))

            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            for i in range(140):
                by = "user" if i % 2 == 0 else "peer1"
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=gid,
                    scope_key="",
                    by=by,
                    data={"text": f"turn {i} " + ("x" * 180), "to": []},
                )

            first = run_auto_conversation_memory_cycle(
                group_id=gid,
                actor_id="peer1",
                max_messages=240,
                context_window_tokens=3000,
                reserve_tokens=200,
                keep_recent_tokens=500,
                signal_pack_token_budget=120,
            )
            self.assertEqual(str(first.get("status") or ""), "written")
            target = Path(str(first.get("target_file") or ""))
            self.assertTrue(target.exists(), f"missing daily target file: {target}")
            text = target.read_text(encoding="utf-8", errors="replace")
            self.assertIn("## Conversation Summary", text)
            self.assertIn("## Coordination Snapshot", text)
            self.assertIn("## Task Snapshot", text)
            self.assertIn("## Agent Resume Cues", text)
            self.assertNotIn("[signal_pack]", text)

            second = run_auto_conversation_memory_cycle(
                group_id=gid,
                actor_id="peer1",
                max_messages=240,
                context_window_tokens=3000,
                reserve_tokens=200,
                keep_recent_tokens=500,
                signal_pack_token_budget=120,
            )
            self.assertEqual(str(second.get("status") or ""), "silent")
        finally:
            cleanup()

    def test_daily_flush_signal_pack_budget_enforced(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("reme-signal-pack")
            messages = [{"role": "user", "content": "Need memory compaction summary."}]
            large_signal_pack = {
                "coordination_brief": {
                    "objective": "A" * 2000,
                    "current_focus": "B" * 1200,
                    "constraints": ["research", "implementation", "review", "ops", "qa", "pm", "design"],
                    "project_brief": "C" * 1200,
                },
                "tasks": {
                    "active": [f"active-{i} " + ("D" * 120) for i in range(20)],
                    "planned": [f"planned-{i} " + ("E" * 120) for i in range(20)],
                    "done_recent": [f"done-{i} " + ("F" * 120) for i in range(20)],
                    "blocked": [f"blocked-{i} " + ("J" * 120) for i in range(20)],
                    "waiting_user": [f"waiting-{i} " + ("K" * 120) for i in range(20)],
                },
                "agent_states": [
                    {
                        "id": f"a{i}",
                        "hot": {"focus": "G" * 400, "next_action": "H" * 400, "blockers": ["I" * 300]},
                        "warm": {"what_changed": "seeded", "resume_hint": "re-open memory lane"},
                    }
                    for i in range(20)
                ],
            }
            flush_resp, _ = self._call(
                "memory_reme_daily_flush",
                {
                    "group_id": gid,
                    "messages": messages,
                    "signal_pack": large_signal_pack,
                    "signal_pack_token_budget": 64,
                },
            )
            self.assertTrue(flush_resp.ok, getattr(flush_resp, "error", None))
            result = flush_resp.result if isinstance(flush_resp.result, dict) else {}
            meta = result.get("signal_pack") if isinstance(result.get("signal_pack"), dict) else {}
            self.assertEqual(str(meta.get("schema") or ""), "v1")
            self.assertLessEqual(int(meta.get("token_estimate") or 0), int(meta.get("token_budget") or 0))
            self.assertLessEqual(int(meta.get("token_budget") or 0), 64)
        finally:
            cleanup()

    def test_dedup_precheck_silent_semantics(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("reme-dedup-precheck")
            seed_resp, _ = self._call(
                "memory_reme_write",
                {
                    "group_id": gid,
                    "target": "memory",
                    "content": "Keep changelog entries concise and factual.",
                    "dedup_intent": "new",
                },
            )
            self.assertTrue(seed_resp.ok, getattr(seed_resp, "error", None))

            flush_resp, _ = self._call(
                "memory_reme_daily_flush",
                {
                    "group_id": gid,
                    "messages": [{"role": "user", "content": "Keep changelog entries concise and factual."}],
                    "dedup_intent": "silent",
                    "dedup_query": "Keep changelog entries concise and factual.",
                },
            )
            self.assertTrue(flush_resp.ok, getattr(flush_resp, "error", None))
            result = flush_resp.result if isinstance(flush_resp.result, dict) else {}
            dedup = result.get("dedup") if isinstance(result.get("dedup"), dict) else {}
            self.assertEqual(str(result.get("status") or ""), "silent")
            self.assertEqual(str(result.get("reason") or ""), "precheck_silent")
            self.assertEqual(str(dedup.get("precheck_decision") or ""), "silent")
            self.assertEqual(str(dedup.get("final_decision") or ""), "silent")
            self.assertEqual(str(dedup.get("final_reason") or ""), "precheck_silent")
            self.assertEqual(str(dedup.get("decision") or ""), "silent")
        finally:
            cleanup()

    def test_dedup_persistence_idempotency_semantics(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("reme-dedup-idempotency")
            args = {
                "group_id": gid,
                "target": "memory",
                "content": "Idempotency marker test payload.",
                "idempotency_key": "dedup_idempotency_case",
                "dedup_intent": "new",
            }
            first_resp, _ = self._call("memory_reme_write", args)
            self.assertTrue(first_resp.ok, getattr(first_resp, "error", None))
            self.assertEqual(str((first_resp.result or {}).get("status") or ""), "written")

            second_resp, _ = self._call("memory_reme_write", args)
            self.assertTrue(second_resp.ok, getattr(second_resp, "error", None))
            result = second_resp.result if isinstance(second_resp.result, dict) else {}
            dedup = result.get("dedup") if isinstance(result.get("dedup"), dict) else {}
            self.assertEqual(str(result.get("status") or ""), "silent")
            self.assertEqual(str(result.get("reason") or ""), "persistence_idempotency_key")
            self.assertEqual(str(dedup.get("precheck_decision") or ""), "new")
            self.assertEqual(str(dedup.get("final_decision") or ""), "silent")
            self.assertEqual(str(dedup.get("final_reason") or ""), "persistence_idempotency_key")
            self.assertEqual(str(dedup.get("decision") or ""), "silent")
        finally:
            cleanup()

    def test_memory_write_to_memory_shadows_same_content_into_daily(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("reme-memory-shadow")
            from cccc.kernel.group import load_group

            payload = {
                "group_id": gid,
                "target": "memory",
                "content": "Keep the memory-lane coverage bridge deterministic.",
                "idempotency_key": "memory_shadow_case",
                "dedup_intent": "new",
            }
            first_resp, _ = self._call("memory_reme_write", payload)
            self.assertTrue(first_resp.ok, getattr(first_resp, "error", None))
            first = first_resp.result if isinstance(first_resp.result, dict) else {}
            self.assertEqual(str(first.get("status") or ""), "written")
            shadow = first.get("shadow_daily") if isinstance(first.get("shadow_daily"), dict) else {}
            self.assertIn(str(shadow.get("status") or ""), {"written", "silent"})

            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            memory_root = Path(group.path) / "state" / "memory"
            memory_text = (memory_root / "MEMORY.md").read_text(encoding="utf-8")
            daily_text = "\n".join(p.read_text(encoding="utf-8") for p in sorted((memory_root / "daily").glob("*.md")))
            needle = "Keep the memory-lane coverage bridge deterministic."
            self.assertEqual(memory_text.count(needle), 1)
            self.assertEqual(daily_text.count(needle), 1)

            second_resp, _ = self._call("memory_reme_write", payload)
            self.assertTrue(second_resp.ok, getattr(second_resp, "error", None))
            second = second_resp.result if isinstance(second_resp.result, dict) else {}
            self.assertEqual(str(second.get("status") or ""), "silent")
            daily_text_2 = "\n".join(p.read_text(encoding="utf-8") for p in sorted((memory_root / "daily").glob("*.md")))
            self.assertEqual(daily_text_2.count(needle), 1)
        finally:
            cleanup()

    def test_dedup_persistence_content_hash_semantics(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("reme-dedup-content-hash")
            args = {
                "group_id": gid,
                "target": "memory",
                "content": "Content hash dedup payload.",
                "dedup_intent": "new",
            }
            first_resp, _ = self._call("memory_reme_write", args)
            self.assertTrue(first_resp.ok, getattr(first_resp, "error", None))
            self.assertEqual(str((first_resp.result or {}).get("status") or ""), "written")

            second_resp, _ = self._call("memory_reme_write", args)
            self.assertTrue(second_resp.ok, getattr(second_resp, "error", None))
            result = second_resp.result if isinstance(second_resp.result, dict) else {}
            dedup = result.get("dedup") if isinstance(result.get("dedup"), dict) else {}
            self.assertEqual(str(result.get("status") or ""), "silent")
            self.assertEqual(str(result.get("reason") or ""), "persistence_content_hash")
            self.assertEqual(str(dedup.get("precheck_decision") or ""), "new")
            self.assertEqual(str(dedup.get("final_decision") or ""), "silent")
            self.assertEqual(str(dedup.get("final_reason") or ""), "persistence_content_hash")
            self.assertEqual(str(dedup.get("decision") or ""), "silent")
        finally:
            cleanup()

    def test_group_signal_pack_prioritizes_active_actor_and_keeps_rich_warm_fields(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("reme-signal-pack-rich")
            from cccc.daemon.memory.memory_ops import _build_group_signal_pack

            create_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [
                        {
                            "op": "task.create",
                            "title": "Primary Work",
                            "outcome": "ship reliable recovery",
                            "status": "active",
                            "assignee": "peer1",
                        }
                    ],
                },
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))

            peer1_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "peer1",
                    "ops": [
                        {
                            "op": "agent_state.update",
                            "actor_id": "peer1",
                            "focus": "primary work",
                            "next_action": "verify bootstrap recovery",
                            "what_changed": "picked up the active task",
                            "resume_hint": "re-open the bootstrap tests",
                            "environment_summary": "workspace has a small dirty tree",
                            "user_model": "prefers concise evidence",
                            "persona_notes": "do not overbuild the fix",
                        }
                    ],
                },
            )
            self.assertTrue(peer1_resp.ok, getattr(peer1_resp, "error", None))

            peer2_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "peer2",
                    "ops": [
                        {
                            "op": "agent_state.update",
                            "actor_id": "peer2",
                            "focus": "secondary",
                            "next_action": "wait",
                            "what_changed": "idle",
                            "resume_hint": "none",
                            "environment_summary": "cold",
                            "user_model": "secondary",
                            "persona_notes": "secondary",
                        }
                    ],
                },
            )
            self.assertTrue(peer2_resp.ok, getattr(peer2_resp, "error", None))

            pack, meta = _build_group_signal_pack(gid, token_budget=4096)
            self.assertIsInstance(pack, dict)
            assert isinstance(pack, dict)
            self.assertEqual(str(meta.get("schema") or ""), "v1")
            agent_states = pack.get("agent_states") if isinstance(pack.get("agent_states"), list) else []
            self.assertGreaterEqual(len(agent_states), 1)
            first = agent_states[0] if isinstance(agent_states[0], dict) else {}
            self.assertEqual(str(first.get("id") or ""), "peer1")
            self.assertEqual(str(first.get("environment_summary") or ""), "workspace has a small dirty tree")
            self.assertEqual(str(first.get("user_model") or ""), "prefers concise evidence")
            self.assertEqual(str(first.get("persona_notes") or ""), "do not overbuild the fix")
        finally:
            cleanup()


    def test_signal_pack_budget_drops_optional_rich_fields_before_core_hot_fields(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.daemon.memory.memory_ops import _normalize_signal_pack

            payload = {
                "coordination_brief": {
                    "objective": "ship recovery",
                    "current_focus": "bootstrap",
                    "constraints": ["keep it lean"],
                    "project_brief": "x" * 400,
                },
                "tasks": {
                    "active": ["T001: Primary Work"],
                    "planned": [],
                    "done_recent": [],
                    "blocked": [],
                    "waiting_user": [],
                },
                "agent_states": [
                    {
                        "id": "peer1",
                        "hot": {
                            "active_task_id": "T001",
                            "focus": "primary work",
                            "next_action": "verify bootstrap",
                            "blockers": ["none"],
                        },
                        "warm": {
                            "what_changed": "picked up the task",
                            "resume_hint": "re-open tests",
                            "environment_summary": "workspace has a very long environment summary " * 8,
                            "user_model": "user likes concise evidence " * 8,
                            "persona_notes": "avoid overbuilding and keep low noise " * 8,
                        },
                    }
                ],
            }
            pack, meta = _normalize_signal_pack(payload, token_budget=190)
            self.assertIsInstance(pack, dict)
            assert isinstance(pack, dict)
            self.assertLessEqual(int(meta.get("token_estimate") or 0), int(meta.get("token_budget") or 0))
            agent_states = pack.get("agent_states") if isinstance(pack.get("agent_states"), list) else []
            self.assertGreaterEqual(len(agent_states), 1)
            first = agent_states[0] if isinstance(agent_states[0], dict) else {}
            self.assertEqual(str(first.get("id") or ""), "peer1")
            self.assertEqual(str(first.get("active_task_id") or ""), "T001")
            self.assertEqual(str(first.get("focus") or ""), "primary work")
            self.assertEqual(str(first.get("next_action") or ""), "verify bootstrap")
            self.assertNotIn("persona_notes", first)
        finally:
            cleanup()

if __name__ == "__main__":
    unittest.main()
