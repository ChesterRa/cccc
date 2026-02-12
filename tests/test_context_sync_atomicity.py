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
                    "ops": [{"op": "note.add", "content": "applied"}],
                },
            )
            self.assertTrue(sync_resp.ok, getattr(sync_resp, "error", None))
            result = sync_resp.result if isinstance(sync_resp.result, dict) else {}
            self.assertFalse(bool(result.get("dry_run")))

            get_resp, _ = self._call("context_get", {"group_id": group_id})
            self.assertTrue(get_resp.ok, getattr(get_resp, "error", None))
            notes = (get_resp.result or {}).get("notes") if isinstance(get_resp.result, dict) else []
            self.assertIsInstance(notes, list)
            assert isinstance(notes, list)
            self.assertTrue(any(isinstance(n, dict) and n.get("content") == "applied" for n in notes))
        finally:
            cleanup()

    def test_presence_change_rolls_back_on_batch_error(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            sync_resp, _ = self._call(
                "context_sync",
                {
                    "group_id": group_id,
                    "ops": [
                        {"op": "presence.update", "agent_id": "peer1", "status": "working"},
                        {"op": "unknown.op"},
                    ],
                },
            )
            self.assertFalse(sync_resp.ok)

            presence_resp, _ = self._call("presence_get", {"group_id": group_id})
            self.assertTrue(presence_resp.ok, getattr(presence_resp, "error", None))
            agents = (presence_resp.result or {}).get("agents") if isinstance(presence_resp.result, dict) else []
            self.assertIsInstance(agents, list)
            assert isinstance(agents, list)
            self.assertEqual(agents, [])
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
