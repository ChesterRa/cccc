import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestGroupSpaceProjection(unittest.TestCase):
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

    def _create_group(self, title: str = "space-projection") -> str:
        resp, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        gid = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _projection_path(self, repo_root: Path, group_id: str) -> Path:
        _ = group_id
        return repo_root / "space" / ".space-status.json"

    def test_bind_writes_projection_manifest_in_repo(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group()
            attach_resp, _ = self._call(
                "attach",
                {"path": str(project_dir), "group_id": gid, "by": "user"},
            )
            self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))

            bind_resp, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "action": "bind",
                    "remote_space_id": "nb_projection_1",
                    "by": "user",
                },
            )
            self.assertTrue(bind_resp.ok, getattr(bind_resp, "error", None))

            from cccc.kernel.group import load_group

            group = load_group(gid)
            self.assertIsNotNone(group)
            scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
            scope_root = Path(str((scopes[0] if scopes else {}).get("url") or "")).resolve()
            manifest = self._projection_path(scope_root, gid)
            self.assertTrue(manifest.exists())
            doc = json.loads(manifest.read_text(encoding="utf-8"))
            self.assertEqual(str(doc.get("group_id") or ""), gid)
            binding = doc.get("binding") if isinstance(doc.get("binding"), dict) else {}
            self.assertEqual(str(binding.get("status") or ""), "bound")
            self.assertEqual(str(binding.get("remote_space_id") or ""), "nb_projection_1")
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_context_sync_updates_projection_latest_context_sync(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-projection-sync")
            attach_resp, _ = self._call(
                "attach",
                {"path": str(project_dir), "group_id": gid, "by": "user"},
            )
            self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))

            bind_resp, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "action": "bind",
                    "remote_space_id": "nb_projection_2",
                    "by": "user",
                },
            )
            self.assertTrue(bind_resp.ok, getattr(bind_resp, "error", None))

            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "task.create", "name": "projection note", "goal": "test"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))

            from cccc.kernel.group import load_group

            group = load_group(gid)
            self.assertIsNotNone(group)
            scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
            scope_root = Path(str((scopes[0] if scopes else {}).get("url") or "")).resolve()
            manifest = self._projection_path(scope_root, gid)
            self.assertTrue(manifest.exists())
            doc = json.loads(manifest.read_text(encoding="utf-8"))
            latest = doc.get("latest_context_sync") if isinstance(doc.get("latest_context_sync"), dict) else {}
            self.assertTrue(str(latest.get("job_id") or "").strip())
            self.assertEqual(str(latest.get("state") or ""), "pending")
            queue = doc.get("queue_summary") if isinstance(doc.get("queue_summary"), dict) else {}
            self.assertGreaterEqual(int(queue.get("pending") or 0), 1)
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_worker_execution_refreshes_projection_state(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            gid = self._create_group("space-projection-worker")
            attach_resp, _ = self._call(
                "attach",
                {"path": str(project_dir), "group_id": gid, "by": "user"},
            )
            self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))

            bind_resp, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "action": "bind",
                    "remote_space_id": "nb_projection_3",
                    "by": "user",
                },
            )
            self.assertTrue(bind_resp.ok, getattr(bind_resp, "error", None))

            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "task.create", "name": "worker refresh", "goal": "test"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))

            from cccc.daemon.space.group_space_runtime import process_due_space_jobs
            from cccc.kernel.group import load_group

            with patch("cccc.daemon.space.group_space_runtime.provider_ingest", return_value={"ok": True}):
                tick = process_due_space_jobs(limit=20)
            self.assertGreaterEqual(int(tick.get("processed") or 0), 1)

            group = load_group(gid)
            self.assertIsNotNone(group)
            scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
            scope_root = Path(str((scopes[0] if scopes else {}).get("url") or "")).resolve()
            manifest = self._projection_path(scope_root, gid)
            self.assertTrue(manifest.exists())
            doc = json.loads(manifest.read_text(encoding="utf-8"))
            latest = doc.get("latest_context_sync") if isinstance(doc.get("latest_context_sync"), dict) else {}
            self.assertEqual(str(latest.get("state") or ""), "succeeded")
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()


if __name__ == "__main__":
    unittest.main()
