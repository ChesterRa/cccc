import os
import tempfile
import unittest
from unittest.mock import patch


class TestGroupStateOps(unittest.TestCase):
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

    def test_group_set_state_roundtrip(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-state", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            idle, _ = self._call("group_set_state", {"group_id": group_id, "state": "idle", "by": "user"})
            self.assertTrue(idle.ok, getattr(idle, "error", None))
            self.assertEqual(str((idle.result or {}).get("state") or ""), "idle")

            active, _ = self._call("group_set_state", {"group_id": group_id, "state": "active", "by": "user"})
            self.assertTrue(active.ok, getattr(active, "error", None))
            self.assertEqual(str((active.result or {}).get("state") or ""), "active")
        finally:
            cleanup()

    def test_resume_from_idle_clears_pending_auto_idle_notifications(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-state-resume", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            idle, _ = self._call("group_set_state", {"group_id": group_id, "state": "idle", "by": "user"})
            self.assertTrue(idle.ok, getattr(idle, "error", None))

            with patch("cccc.daemon.server.THROTTLE.clear_pending_system_notifies", return_value=1) as clear_mock:
                active, _ = self._call("group_set_state", {"group_id": group_id, "state": "active", "by": "user"})

            self.assertTrue(active.ok, getattr(active, "error", None))
            clear_mock.assert_called_once()
            notify_kinds = clear_mock.call_args.kwargs.get("notify_kinds")
            self.assertIsInstance(notify_kinds, set)
            self.assertIn("auto_idle", notify_kinds)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
