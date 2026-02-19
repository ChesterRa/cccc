import os
import tempfile
import unittest


class TestGroupLifecycleOps(unittest.TestCase):
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

    def test_group_start_requires_active_scope(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(getattr(start.error, "code", ""), "missing_project_root")
        finally:
            cleanup()

    def test_group_start_does_not_resume_paused_group(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "group-start-paused", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            set_state, _ = self._call("group_set_state", {"group_id": group_id, "state": "paused", "by": "user"})
            self.assertTrue(set_state.ok, getattr(set_state, "error", None))

            start, _ = self._call("group_start", {"group_id": group_id, "by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))

            show, _ = self._call("group_show", {"group_id": group_id})
            self.assertTrue(show.ok, getattr(show, "error", None))
            group_doc = (show.result or {}).get("group") if isinstance(show.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertEqual(str(group_doc.get("state") or ""), "paused")
            self.assertTrue(bool(group_doc.get("running")))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
