import os
import tempfile
import unittest


class TestContextSyncAtomicity(unittest.TestCase):
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
        resp, _ = self._call("group_create", {"title": "context-atomic", "topic": "", "by": "user"})
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        group_id = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)
        return group_id

    def test_context_sync_string_false_dry_run_executes(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "dry_run": "false",
                    "ops": [{"op": "task.create", "title": "applied", "outcome": "test"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))
            result = sync_resp.result if isinstance(sync_resp.result, dict) else {}
            self.assertFalse(bool(result.get("dry_run")))

            tasks_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(tasks_resp.ok, getattr(tasks_resp, "error", None))
            tasks = (tasks_resp.result or {}).get("tasks") if isinstance(tasks_resp.result, dict) else []
            self.assertIsInstance(tasks, list)
            assert isinstance(tasks, list)
            self.assertTrue(any(isinstance(t, dict) and t.get("title") == "applied" for t in tasks))
        finally:
            cleanup()

    def test_agent_change_rolls_back_on_batch_error(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "ops": [
                        {"op": "agent_state.update", "actor_id": "peer1", "focus": "working"},
                        {"op": "unknown.op"},
                    ],
                },
            )
            self.assertFalse(sync_resp.ok)

            ctx_resp, _ = self._call("context_get", {"group_id": group_id})
            self.assertTrue(ctx_resp.ok, getattr(ctx_resp, "error", None))
            agents = (
                (ctx_resp.result or {}).get("agent_states")
                if isinstance(ctx_resp.result, dict)
                else []
            )
            self.assertIsInstance(agents, list)
            assert isinstance(agents, list)
            self.assertEqual(agents, [])
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
