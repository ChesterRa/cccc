from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path


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
            self.assertIn("signal_pack", text)

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
