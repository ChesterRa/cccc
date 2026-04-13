from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path


class TestContextMemoryExperienceHook(unittest.TestCase):
    _REQUIRED_FIELDS = (
        "id",
        "source_kind",
        "source_refs",
        "title",
        "summary",
        "applicability",
        "recommended_action",
        "failure_signals",
        "status",
        "proposed_by",
        "created_at",
        "updated_at",
    )

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

    def _create_group(self) -> str:
        resp, _ = self._call("group_create", {"title": "experience-hook", "topic": "", "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        group_id = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)
        return group_id

    def _load_candidates(self, group_id: str) -> tuple[Path, list[dict]]:
        from cccc.kernel.group import load_group
        from cccc.util.fs import read_json

        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        path = Path(group.path) / "state" / "experience_candidates.json"
        raw = read_json(path)
        items = raw.get("candidates") if isinstance(raw.get("candidates"), list) else []
        normalized = [item for item in items if isinstance(item, dict)]
        return path, normalized

    def test_root_task_move_done_creates_proposed_candidate(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            create_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.create", "title": "Phase 1", "outcome": "deliver phase 1"}],
                },
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            task_list_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(task_list_resp.ok, getattr(task_list_resp, "error", None))
            tasks = (task_list_resp.result or {}).get("tasks") if isinstance(task_list_resp.result, dict) else []
            self.assertIsInstance(tasks, list)
            assert isinstance(tasks, list)
            task_id = str((tasks[0] if tasks else {}).get("id") or "")
            self.assertTrue(task_id)

            done_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.move", "task_id": task_id, "status": "done"}],
                },
            )
            self.assertTrue(done_resp.ok, getattr(done_resp, "error", None))

            path, candidates = self._load_candidates(group_id)
            self.assertTrue(path.exists())
            self.assertGreaterEqual(len(candidates), 1)
            candidate = candidates[0]
            self.assertEqual(str(candidate.get("source_kind") or ""), "task.root_done")
            self.assertEqual(str(candidate.get("status") or ""), "proposed")
            self.assertEqual(str(candidate.get("task_id") or ""), task_id)
            self.assertEqual(str(candidate.get("proposed_by") or ""), "user")
            self.assertEqual(str(candidate.get("title") or ""), "Phase 1")
            self.assertEqual(str(candidate.get("summary") or ""), "deliver phase 1")
            self.assertEqual(str(candidate.get("applicability") or ""), "")
            self.assertEqual(str(candidate.get("recommended_action") or ""), "")
            self.assertEqual(candidate.get("failure_signals"), [])
            for field in self._REQUIRED_FIELDS:
                self.assertIn(field, candidate, f"missing required candidate field: {field}")
            source_refs = candidate.get("source_refs")
            self.assertIsInstance(source_refs, list)
            assert isinstance(source_refs, list)
            self.assertIn(f"task:{task_id}", source_refs)
            self.assertIsInstance(candidate.get("failure_signals"), list)

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_file = Path(group.path) / "state" / "memory" / "MEMORY.md"
            if memory_file.exists():
                memory_text = memory_file.read_text(encoding="utf-8")
                self.assertNotIn("Root task completed:", memory_text)
        finally:
            cleanup()

    def test_decision_candidates_are_permissive_and_leave_governance_later(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [
                        {
                            "op": "coordination.note.add",
                            "kind": "decision",
                            "summary": "Decide to adopt strict state version checks for context sync.",
                        },
                        {
                            "op": "coordination.note.add",
                            "kind": "decision",
                            "summary": "Maybe switch this later?",
                        },
                    ],
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))

            _, candidates = self._load_candidates(group_id)
            decision_candidates = [item for item in candidates if str(item.get("source_kind") or "") == "coordination.decision"]
            self.assertEqual(len(decision_candidates), 2)
            summaries = {str(item.get("summary") or "") for item in decision_candidates}
            self.assertIn("Decide to adopt strict state version checks for context sync.", summaries)
            self.assertIn("Maybe switch this later?", summaries)
            for candidate in decision_candidates:
                for field in self._REQUIRED_FIELDS:
                    self.assertIn(field, candidate, f"missing required candidate field: {field}")
                self.assertEqual(str(candidate.get("status") or ""), "proposed")
                self.assertEqual(str(candidate.get("proposed_by") or ""), "user")
                self.assertEqual(str(candidate.get("applicability") or ""), "")
                self.assertEqual(str(candidate.get("recommended_action") or ""), "")
                self.assertEqual(candidate.get("failure_signals"), [])
        finally:
            cleanup()

    def test_dry_run_does_not_create_experience_candidate(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            create_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.create", "title": "Dry Run Task", "outcome": "verify dry run"}],
                },
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            task_list_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(task_list_resp.ok, getattr(task_list_resp, "error", None))
            tasks = (task_list_resp.result or {}).get("tasks") if isinstance(task_list_resp.result, dict) else []
            self.assertIsInstance(tasks, list)
            assert isinstance(tasks, list)
            task_id = str((tasks[0] if tasks else {}).get("id") or "")
            self.assertTrue(task_id)

            dry_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "dry_run": True,
                    "ops": [
                        {"op": "coordination.note.add", "kind": "decision", "summary": "Decide to use merged logs."},
                        {"op": "task.move", "task_id": task_id, "status": "done"},
                    ],
                },
            )
            self.assertTrue(dry_resp.ok, getattr(dry_resp, "error", None))
            path, candidates = self._load_candidates(group_id)
            self.assertFalse(path.exists(), f"unexpected experience candidate side effect in dry-run: {path}")
            self.assertEqual(candidates, [])
        finally:
            cleanup()

    def test_failed_batch_does_not_persist_decision_candidate(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [
                        {
                            "op": "coordination.note.add",
                            "kind": "decision",
                            "summary": "Decide to persist candidate only after context commit succeeds.",
                        },
                        {
                            "op": "task.move",
                            "task_id": "T404",
                            "status": "done",
                        },
                    ],
                },
            )
            self.assertFalse(resp.ok)
            path, candidates = self._load_candidates(group_id)
            self.assertFalse(path.exists(), f"unexpected orphan experience candidate after failed batch: {path}")
            self.assertEqual(candidates, [])
        finally:
            cleanup()

    def test_unified_extraction_entry_supports_decision_dry_run_without_persist(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            from cccc.kernel.experience import SOURCE_KIND_COORDINATION_DECISION, extract_experience_candidate
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            candidate = extract_experience_candidate(
                group=group,
                source_kind=SOURCE_KIND_COORDINATION_DECISION,
                payload={
                    "summary": "Decide to keep extraction permissive and defer governance.",
                    "by": "user",
                    "task_id": "T100",
                    "source_refs": ["coordination:decision"],
                },
                dry_run=True,
            )
            self.assertIsInstance(candidate, dict)
            assert isinstance(candidate, dict)
            self.assertEqual(str(candidate.get("source_kind") or ""), SOURCE_KIND_COORDINATION_DECISION)

            path, candidates = self._load_candidates(group_id)
            self.assertFalse(path.exists())
            self.assertEqual(candidates, [])
        finally:
            cleanup()

    def test_headless_turn_success_candidate_is_extracted_from_prompt(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            from cccc.kernel.experience import SOURCE_KIND_HEADLESS_TURN_SUCCESS, extract_experience_candidate
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            candidate = extract_experience_candidate(
                group=group,
                source_kind=SOURCE_KIND_HEADLESS_TURN_SUCCESS,
                payload={
                    "prompt_text": "修复学习看板的治理语义，并把 stability 和 patch_review_mode 直接上屏。",
                    "by": "agent1",
                    "actor_id": "agent1",
                    "runtime": "codex",
                    "turn_id": "turn-001",
                    "event_id": "evt-001",
                    "source_refs": ["headless:turn.success"],
                },
                dry_run=True,
            )
            self.assertIsInstance(candidate, dict)
            assert isinstance(candidate, dict)
            self.assertEqual(str(candidate.get("source_kind") or ""), SOURCE_KIND_HEADLESS_TURN_SUCCESS)
            self.assertEqual(str(candidate.get("status") or ""), "proposed")
            self.assertEqual(str(candidate.get("proposed_by") or ""), "agent1")
            self.assertEqual(str(candidate.get("task_id") or ""), "turn-001")
            self.assertIn("Successful codex turn pattern", str(candidate.get("applicability") or ""))
            self.assertIn("修复学习看板的治理语义", str(candidate.get("summary") or ""))
            source_refs = candidate.get("source_refs")
            self.assertIsInstance(source_refs, list)
            assert isinstance(source_refs, list)
            self.assertIn("headless:turn.success", source_refs)
            self.assertIn("turn:turn-001", source_refs)
            self.assertIn("event:evt-001", source_refs)
            self.assertIn("actor:agent1", source_refs)
        finally:
            cleanup()

    def test_headless_turn_success_candidate_dedupes_same_prompt_pattern(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            from cccc.kernel.experience import SOURCE_KIND_HEADLESS_TURN_SUCCESS, extract_experience_candidate
            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            first = extract_experience_candidate(
                group=group,
                source_kind=SOURCE_KIND_HEADLESS_TURN_SUCCESS,
                payload={
                    "prompt_text": "修复学习看板的治理语义，并把 stability 和 patch_review_mode 直接上屏。",
                    "by": "agent1",
                    "actor_id": "agent1",
                    "runtime": "codex",
                    "turn_id": "turn-001",
                    "event_id": "evt-001",
                    "source_refs": ["headless:turn.success"],
                },
                dry_run=True,
            )
            second = extract_experience_candidate(
                group=group,
                source_kind=SOURCE_KIND_HEADLESS_TURN_SUCCESS,
                payload={
                    "prompt_text": "修复学习看板的治理语义，并把 stability 和 patch_review_mode 直接上屏。",
                    "by": "agent1",
                    "actor_id": "agent1",
                    "runtime": "codex",
                    "turn_id": "turn-002",
                    "event_id": "evt-002",
                    "source_refs": ["headless:turn.success"],
                },
                dry_run=True,
            )
            self.assertIsInstance(first, dict)
            self.assertIsInstance(second, dict)
            assert isinstance(first, dict) and isinstance(second, dict)
            self.assertEqual(str(first.get("id") or ""), str(second.get("id") or ""))
            self.assertNotEqual(str(first.get("task_id") or ""), str(second.get("task_id") or ""))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
