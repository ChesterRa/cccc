import json
import os
import tempfile
import unittest
from pathlib import Path


class TestRootTaskCompleteMemoryHook(unittest.TestCase):
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
        resp, _ = self._call("group_create", {"title": "memory-hook", "topic": "", "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        group_id = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)
        return group_id

    def test_root_task_complete_triggers_solidify_and_export(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            # Create a root task (parent_id=null => phase/root task semantics)
            create_task_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.create", "name": "Phase 1", "goal": "deliver phase 1"}],
                },
            )
            self.assertTrue(create_task_resp.ok, getattr(create_task_resp, "error", None))

            # Find the created task ID
            task_list_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(task_list_resp.ok, getattr(task_list_resp, "error", None))
            tasks = (task_list_resp.result or {}).get("tasks") if isinstance(task_list_resp.result, dict) else []
            self.assertIsInstance(tasks, list)
            assert isinstance(tasks, list)
            self.assertGreaterEqual(len(tasks), 1)
            task_id = str((tasks[0] or {}).get("id") or "")
            self.assertTrue(task_id)

            # Store a draft memory associated with this task
            store_resp, _ = self._call(
                "memory_store",
                {"group_id": group_id, "content": "hook-target-memory", "task_id": task_id},
            )
            self.assertTrue(store_resp.ok, getattr(store_resp, "error", None))
            memory_id = str((store_resp.result or {}).get("id") or "")
            self.assertTrue(memory_id)

            # Complete the root task (triggers memory solidify+export hook)
            complete_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.status", "task_id": task_id, "status": "done"}],
                },
            )
            self.assertTrue(complete_resp.ok, getattr(complete_resp, "error", None))

            # Verify memory was solidified
            search_resp, _ = self._call(
                "memory_search",
                {"group_id": group_id, "task_id": task_id, "status": "solid"},
            )
            self.assertTrue(search_resp.ok, getattr(search_resp, "error", None))
            result = search_resp.result if isinstance(search_resp.result, dict) else {}
            memories = result.get("memories") if isinstance(result.get("memories"), list) else []
            self.assertTrue(any(str(m.get("id") or "") == memory_id for m in memories))

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            md_path = Path(group.path) / "state" / "memory.md"
            manifest_path = Path(group.path) / "state" / "manifest.json"
            self.assertTrue(md_path.exists(), f"missing export file: {md_path}")
            self.assertTrue(manifest_path.exists(), f"missing manifest file: {manifest_path}")

            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            self.assertEqual(str(manifest.get("group_id") or ""), group_id)
            self.assertTrue(str(manifest.get("sha256") or ""))
            self.assertGreaterEqual(int(manifest.get("memory_count") or 0), 1)

            exported_md = md_path.read_text(encoding="utf-8")
            self.assertIn("hook-target-memory", exported_md)
        finally:
            cleanup()

    def test_root_task_complete_dry_run_has_no_memory_side_effect(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            create_task_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.create", "name": "Phase DryRun", "goal": "verify dry run"}],
                },
            )
            self.assertTrue(create_task_resp.ok, getattr(create_task_resp, "error", None))

            task_list_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(task_list_resp.ok, getattr(task_list_resp, "error", None))
            tasks = (task_list_resp.result or {}).get("tasks") if isinstance(task_list_resp.result, dict) else []
            self.assertIsInstance(tasks, list)
            assert isinstance(tasks, list)
            self.assertGreaterEqual(len(tasks), 1)
            task_id = str((tasks[0] or {}).get("id") or "")
            self.assertTrue(task_id)

            store_resp, _ = self._call(
                "memory_store",
                {"group_id": group_id, "content": "dry-run-memory", "task_id": task_id},
            )
            self.assertTrue(store_resp.ok, getattr(store_resp, "error", None))
            memory_id = str((store_resp.result or {}).get("id") or "")
            self.assertTrue(memory_id)

            dry_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "dry_run": True,
                    "ops": [{"op": "task.status", "task_id": task_id, "status": "done"}],
                },
            )
            self.assertTrue(dry_resp.ok, getattr(dry_resp, "error", None))
            dry_result = dry_resp.result if isinstance(dry_resp.result, dict) else {}
            self.assertTrue(bool(dry_result.get("dry_run")))

            draft_resp, _ = self._call(
                "memory_search",
                {"group_id": group_id, "task_id": task_id, "status": "draft"},
            )
            self.assertTrue(draft_resp.ok, getattr(draft_resp, "error", None))
            draft_result = draft_resp.result if isinstance(draft_resp.result, dict) else {}
            draft_memories = draft_result.get("memories") if isinstance(draft_result.get("memories"), list) else []
            self.assertTrue(any(str(m.get("id") or "") == memory_id for m in draft_memories))

            solid_resp, _ = self._call(
                "memory_search",
                {"group_id": group_id, "task_id": task_id, "status": "solid"},
            )
            self.assertTrue(solid_resp.ok, getattr(solid_resp, "error", None))
            solid_result = solid_resp.result if isinstance(solid_resp.result, dict) else {}
            solid_memories = solid_result.get("memories") if isinstance(solid_result.get("memories"), list) else []
            self.assertFalse(any(str(m.get("id") or "") == memory_id for m in solid_memories))

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            md_path = Path(group.path) / "state" / "memory.md"
            manifest_path = Path(group.path) / "state" / "manifest.json"
            self.assertFalse(md_path.exists(), f"unexpected export file in dry-run: {md_path}")
            self.assertFalse(manifest_path.exists(), f"unexpected manifest in dry-run: {manifest_path}")
        finally:
            cleanup()

if __name__ == "__main__":
    unittest.main()
