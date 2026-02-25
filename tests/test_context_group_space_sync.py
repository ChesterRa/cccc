import os
import tempfile
import unittest


class TestContextGroupSpaceSync(unittest.TestCase):
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

    def _create_group(self, title: str = "ctx-space-sync") -> str:
        resp, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        gid = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _bind_space(self, group_id: str, remote_space_id: str = "nb_ctx_sync") -> None:
        resp, _ = self._call(
            "group_space_bind",
            {
                "group_id": group_id,
                "provider": "notebooklm",
                "action": "bind",
                "remote_space_id": remote_space_id,
                "by": "user",
            },
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))

    def test_curated_context_change_enqueues_group_space_sync_job(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._bind_space(gid, "nb_ctx_1")

            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "note.add", "content": "ship this"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))
            result = sync_resp.result if isinstance(sync_resp.result, dict) else {}
            space_sync = result.get("space_sync") if isinstance(result.get("space_sync"), dict) else {}
            self.assertEqual(bool(space_sync.get("queued")), True)
            self.assertEqual(bool(space_sync.get("deduped")), False)
            self.assertTrue(str(space_sync.get("job_id") or "").strip())

            jobs_resp, _ = self._call(
                "group_space_jobs",
                {"group_id": gid, "provider": "notebooklm", "action": "list"},
            )
            self.assertTrue(jobs_resp.ok, getattr(jobs_resp, "error", None))
            jobs = (jobs_resp.result or {}).get("jobs") if isinstance(jobs_resp.result, dict) else []
            self.assertIsInstance(jobs, list)
            assert isinstance(jobs, list)
            self.assertGreaterEqual(len(jobs), 1)
            job = jobs[0] if isinstance(jobs[0], dict) else {}
            self.assertEqual(str(job.get("kind") or ""), "context_sync")
            payload = job.get("payload") if isinstance(job.get("payload"), dict) else {}
            summary = payload.get("summary") if isinstance(payload.get("summary"), dict) else {}
            notes = summary.get("notes") if isinstance(summary.get("notes"), list) else []
            self.assertTrue(any(isinstance(n, dict) and n.get("content") == "ship this" for n in notes))
        finally:
            cleanup()

    def test_presence_only_change_does_not_enqueue_group_space_sync_job(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._bind_space(gid, "nb_ctx_2")

            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "presence.update", "agent_id": "peer1", "status": "working"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))
            result = sync_resp.result if isinstance(sync_resp.result, dict) else {}
            self.assertFalse(isinstance(result.get("space_sync"), dict))

            jobs_resp, _ = self._call(
                "group_space_jobs",
                {"group_id": gid, "provider": "notebooklm", "action": "list"},
            )
            self.assertTrue(jobs_resp.ok, getattr(jobs_resp, "error", None))
            jobs = (jobs_resp.result or {}).get("jobs") if isinstance(jobs_resp.result, dict) else []
            self.assertEqual(len(jobs), 0)
        finally:
            cleanup()

    def test_context_sync_reports_not_bound_without_failing(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "note.add", "content": "not bound yet"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))
            result = sync_resp.result if isinstance(sync_resp.result, dict) else {}
            space_sync = result.get("space_sync") if isinstance(result.get("space_sync"), dict) else {}
            self.assertEqual(bool(space_sync.get("queued")), False)
            self.assertEqual(str(space_sync.get("reason") or ""), "not_bound")
        finally:
            cleanup()

    def test_same_context_version_dedupes_context_sync_job(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._bind_space(gid, "nb_ctx_3")

            first, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "vision.update", "vision": "north star"}],
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))
            first_result = first.result if isinstance(first.result, dict) else {}
            first_space = first_result.get("space_sync") if isinstance(first_result.get("space_sync"), dict) else {}
            self.assertEqual(bool(first_space.get("queued")), True)
            self.assertEqual(bool(first_space.get("deduped")), False)

            second, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "vision.update", "vision": "north star"}],
                },
            )
            self.assertTrue(second.ok, getattr(second, "error", None))
            second_result = second.result if isinstance(second.result, dict) else {}
            second_space = second_result.get("space_sync") if isinstance(second_result.get("space_sync"), dict) else {}
            self.assertEqual(bool(second_space.get("queued")), True)
            self.assertEqual(bool(second_space.get("deduped")), True)
            self.assertEqual(
                str(first_space.get("idempotency_key") or ""),
                str(second_space.get("idempotency_key") or ""),
            )
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
