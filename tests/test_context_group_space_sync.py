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
                "lane": "work",
                "action": "bind",
                "remote_space_id": remote_space_id,
                "by": "user",
            },
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))

    def test_curated_context_change_does_not_enqueue_group_space_sync_job_by_default(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._bind_space(gid, "nb_ctx_1")

            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "task.create", "title": "ship this", "outcome": "deliver"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))
            result = sync_resp.result if isinstance(sync_resp.result, dict) else {}
            self.assertNotIn("space_sync", result)

            jobs_resp, _ = self._call(
                "group_space_jobs",
                {"group_id": gid, "provider": "notebooklm", "lane": "work", "action": "list"},
            )
            self.assertTrue(jobs_resp.ok, getattr(jobs_resp, "error", None))
            jobs = (jobs_resp.result or {}).get("jobs") if isinstance(jobs_resp.result, dict) else []
            self.assertIsInstance(jobs, list)
            assert isinstance(jobs, list)
            self.assertEqual(len(jobs), 0)
        finally:
            cleanup()

    def test_agent_only_change_does_not_enqueue_group_space_sync_job(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._bind_space(gid, "nb_ctx_2")

            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "peer1",
                    "ops": [{"op": "agent_state.update", "actor_id": "peer1", "focus": "working"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))
            result = sync_resp.result if isinstance(sync_resp.result, dict) else {}
            self.assertFalse(isinstance(result.get("space_sync"), dict))

            jobs_resp, _ = self._call(
                "group_space_jobs",
                {"group_id": gid, "provider": "notebooklm", "lane": "work", "action": "list"},
            )
            self.assertTrue(jobs_resp.ok, getattr(jobs_resp, "error", None))
            jobs = (jobs_resp.result or {}).get("jobs") if isinstance(jobs_resp.result, dict) else []
            self.assertEqual(len(jobs), 0)
        finally:
            cleanup()

    def test_context_sync_does_not_report_space_sync_when_not_bound(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "task.create", "title": "not bound yet", "outcome": "test"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))
            result = sync_resp.result if isinstance(sync_resp.result, dict) else {}
            self.assertNotIn("space_sync", result)
        finally:
            cleanup()

    def test_repeated_context_changes_do_not_create_context_sync_jobs(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            self._bind_space(gid, "nb_ctx_3")

            first, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "coordination.brief.update", "objective": "north star"}],
                },
            )
            self.assertTrue(first.ok, getattr(first, "error", None))
            first_result = first.result if isinstance(first.result, dict) else {}
            self.assertNotIn("space_sync", first_result)

            second, _ = self._call(
                "context_sync",
                {
                    "group_id": gid,
                    "by": "user",
                    "ops": [{"op": "coordination.brief.update", "objective": "north star"}],
                },
            )
            self.assertTrue(second.ok, getattr(second, "error", None))
            second_result = second.result if isinstance(second.result, dict) else {}
            self.assertNotIn("space_sync", second_result)

            jobs_resp, _ = self._call(
                "group_space_jobs",
                {"group_id": gid, "provider": "notebooklm", "lane": "work", "action": "list"},
            )
            self.assertTrue(jobs_resp.ok, getattr(jobs_resp, "error", None))
            jobs = (jobs_resp.result or {}).get("jobs") if isinstance(jobs_resp.result, dict) else []
            self.assertEqual(len(jobs), 0)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
