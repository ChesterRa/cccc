import json
import os
import tempfile
import unittest
from pathlib import Path


class TestMilestoneCompleteMemoryHook(unittest.TestCase):
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

    def test_milestone_complete_triggers_solidify_and_export(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            create_milestone_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "ops": [{"op": "milestone.create", "name": "M", "description": "desc"}],
                },
            )
            self.assertTrue(create_milestone_resp.ok, getattr(create_milestone_resp, "error", None))

            get_resp, _ = self._call("context_get", {"group_id": group_id})
            self.assertTrue(get_resp.ok, getattr(get_resp, "error", None))
            milestones = (get_resp.result or {}).get("milestones") if isinstance(get_resp.result, dict) else []
            self.assertIsInstance(milestones, list)
            assert isinstance(milestones, list)
            milestone_id = str((milestones[0] or {}).get("id") or "")
            self.assertTrue(milestone_id)

            # 先写入该里程碑的 draft memory，作为 hook 输入。
            store_resp, _ = self._call(
                "memory_store",
                {"group_id": group_id, "content": "hook-target-memory", "milestone_id": milestone_id},
            )
            self.assertTrue(store_resp.ok, getattr(store_resp, "error", None))
            memory_id = str((store_resp.result or {}).get("id") or "")
            self.assertTrue(memory_id)

            complete_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "ops": [{"op": "milestone.complete", "milestone_id": milestone_id, "outcomes": "done"}],
                },
            )
            self.assertTrue(complete_resp.ok, getattr(complete_resp, "error", None))

            search_resp, _ = self._call(
                "memory_search",
                {"group_id": group_id, "milestone_id": milestone_id, "status": "solid"},
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


if __name__ == "__main__":
    unittest.main()
