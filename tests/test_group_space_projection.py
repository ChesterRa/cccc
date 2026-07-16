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
                    "lane": "work",
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
            binding = (((doc.get("bindings") or {}).get("work")) if isinstance(doc.get("bindings"), dict) else {})
            self.assertEqual(str(binding.get("status") or ""), "bound")
            self.assertEqual(str(binding.get("remote_space_id") or ""), "nb_projection_1")
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_context_sync_does_not_enqueue_projection_context_sync_by_default(self) -> None:
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
                    "lane": "work",
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
                    "ops": [{"op": "task.create", "title": "projection note", "goal": "test"}],
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
            self.assertEqual(latest, {})
            queue = (((doc.get("queue_summary") or {}).get("work")) if isinstance(doc.get("queue_summary"), dict) else {})
            self.assertEqual(int(queue.get("pending") or 0), 0)
            self.assertEqual(int(queue.get("running") or 0), 0)
            self.assertEqual(int(queue.get("failed") or 0), 0)
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_worker_execution_refreshes_projection_state_for_explicit_job(self) -> None:
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
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_projection_3",
                    "by": "user",
                },
            )
            self.assertTrue(bind_resp.ok, getattr(bind_resp, "error", None))

            from cccc.daemon.space.group_space_store import enqueue_space_job
            from cccc.daemon.space.group_space_runtime import process_due_space_jobs
            from cccc.kernel.group import load_group

            job, deduped = enqueue_space_job(
                group_id=gid,
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_projection_3",
                kind="context_sync",
                payload={"summary": {"tasks": [{"title": "worker refresh"}]}},
                idempotency_key="projection-worker-explicit",
            )
            self.assertFalse(deduped)
            self.assertTrue(str(job.get("job_id") or "").strip())

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

    def test_unbind_neutralizes_projection_sync_state(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            from cccc.daemon.space.group_space_paths import resolve_space_root, space_state_path
            from cccc.kernel.group import load_group

            gid = self._create_group("space-projection-unbind-sync")
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
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_projection_old",
                    "by": "user",
                },
            )
            self.assertTrue(bind_resp.ok, getattr(bind_resp, "error", None))

            space_root = resolve_space_root(gid, create=False)
            self.assertIsNotNone(space_root)
            state_path = space_state_path(space_root or project_dir)
            state_path.write_text(
                json.dumps(
                    {
                        "v": 1,
                        "group_id": gid,
                        "provider": "notebooklm",
                        "remote_space_id": "nb_projection_old",
                        "last_run_at": "2026-03-13T00:00:00Z",
                        "converged": True,
                        "unsynced_count": 0,
                        "failed_count": 0,
                        "remote_sources": 4,
                        "materialized_sources": 4,
                        "last_error": "",
                    },
                    ensure_ascii=False,
                ),
                encoding="utf-8",
            )

            unbind_resp, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "unbind",
                    "by": "user",
                },
            )
            self.assertTrue(unbind_resp.ok, getattr(unbind_resp, "error", None))

            group = load_group(gid)
            self.assertIsNotNone(group)
            scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
            scope_root = Path(str((scopes[0] if scopes else {}).get("url") or "")).resolve()
            manifest = self._projection_path(scope_root, gid)
            doc = json.loads(manifest.read_text(encoding="utf-8"))

            sync_state = doc.get("sync") if isinstance(doc.get("sync"), dict) else {}
            self.assertEqual(str(sync_state.get("reason") or ""), "work_lane_unbound")
            self.assertEqual(str(sync_state.get("last_run_at") or ""), "")
            self.assertEqual(bool(sync_state.get("converged")), False)

            latest = doc.get("latest_context_sync") if isinstance(doc.get("latest_context_sync"), dict) else {}
            self.assertEqual(latest, {})
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()

    def test_unbind_hides_stale_work_queue_and_latest_context_sync(self) -> None:
        _, cleanup = self._with_home()
        project_ctx = tempfile.TemporaryDirectory()
        project_dir = Path(project_ctx.__enter__()).resolve()
        try:
            from cccc.kernel.group import load_group

            gid = self._create_group("space-projection-unbind-queue")
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
                    "lane": "work",
                    "action": "bind",
                    "remote_space_id": "nb_projection_queue",
                    "by": "user",
                },
            )
            self.assertTrue(bind_resp.ok, getattr(bind_resp, "error", None))

            from cccc.daemon.space.group_space_store import enqueue_space_job

            job, deduped = enqueue_space_job(
                group_id=gid,
                provider="notebooklm",
                lane="work",
                remote_space_id="nb_projection_queue",
                kind="context_sync",
                payload={"summary": {"tasks": [{"title": "projection stale"}]}},
                idempotency_key="projection-unbind-stale",
            )
            self.assertFalse(deduped)
            self.assertTrue(str(job.get("job_id") or "").strip())

            unbind_resp, _ = self._call(
                "group_space_bind",
                {
                    "group_id": gid,
                    "provider": "notebooklm",
                    "lane": "work",
                    "action": "unbind",
                    "by": "user",
                },
            )
            self.assertTrue(unbind_resp.ok, getattr(unbind_resp, "error", None))

            group = load_group(gid)
            self.assertIsNotNone(group)
            scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
            scope_root = Path(str((scopes[0] if scopes else {}).get("url") or "")).resolve()
            manifest = self._projection_path(scope_root, gid)
            doc = json.loads(manifest.read_text(encoding="utf-8"))

            latest = doc.get("latest_context_sync") if isinstance(doc.get("latest_context_sync"), dict) else {}
            self.assertEqual(latest, {})

            queue = (((doc.get("queue_summary") or {}).get("work")) if isinstance(doc.get("queue_summary"), dict) else {})
            self.assertEqual(int(queue.get("pending") or 0), 0)
            self.assertEqual(int(queue.get("running") or 0), 0)
            self.assertEqual(int(queue.get("failed") or 0), 0)
        finally:
            project_ctx.__exit__(None, None, None)
            cleanup()


if __name__ == "__main__":
    unittest.main()
