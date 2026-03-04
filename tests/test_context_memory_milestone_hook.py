from __future__ import annotations

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

    def test_root_task_complete_triggers_file_memory_writes(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            create_task_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.create", "name": "Phase 1", "goal": "deliver phase 1"}],
                },
            )
            self.assertTrue(create_task_resp.ok, getattr(create_task_resp, "error", None))

            task_list_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(task_list_resp.ok, getattr(task_list_resp, "error", None))
            tasks = (task_list_resp.result or {}).get("tasks") if isinstance(task_list_resp.result, dict) else []
            self.assertIsInstance(tasks, list)
            assert isinstance(tasks, list)
            self.assertGreaterEqual(len(tasks), 1)
            task = tasks[0] if isinstance(tasks[0], dict) else {}
            task_id = str(task.get("id") or "")
            self.assertTrue(task_id)

            complete_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "by": "user",
                    "ops": [{"op": "task.status", "task_id": task_id, "status": "done"}],
                },
            )
            self.assertTrue(complete_resp.ok, getattr(complete_resp, "error", None))

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_root = Path(group.path) / "state" / "memory"
            self.assertTrue(memory_root.exists())

            memory_file = memory_root / "MEMORY.md"
            self.assertTrue(memory_file.exists())
            memory_text = memory_file.read_text(encoding="utf-8")
            self.assertIn("Root task completed", memory_text)
            self.assertIn(task_id, memory_text)

            daily_files = sorted((memory_root / "daily").glob("*.md"))
            self.assertGreaterEqual(len(daily_files), 1)
            daily_text = "\n".join(p.read_text(encoding="utf-8") for p in daily_files)
            self.assertIn("Task status update", daily_text)
            self.assertIn(task_id, daily_text)
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

            from cccc.kernel.group import load_group

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            memory_root = Path(group.path) / "state" / "memory"
            self.assertFalse(memory_root.exists(), f"unexpected memory side effect in dry-run: {memory_root}")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
