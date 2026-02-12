import os
import tempfile
import unittest
from pathlib import Path


class TestGroupCoreOps(unittest.TestCase):
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

    def test_group_update_and_detach_scope_behaviors(self) -> None:
        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "g1", "topic": "old", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            update_resp, _ = self._call(
                "group_update",
                {"group_id": group_id, "by": "user", "patch": {"title": "new-title", "topic": "new-topic"}},
            )
            self.assertTrue(update_resp.ok, getattr(update_resp, "error", None))
            group_doc = (update_resp.result or {}).get("group") if isinstance(update_resp.result, dict) else {}
            self.assertIsInstance(group_doc, dict)
            assert isinstance(group_doc, dict)
            self.assertEqual(str(group_doc.get("title") or ""), "new-title")
            self.assertEqual(str(group_doc.get("topic") or ""), "new-topic")

            bad_update_resp, _ = self._call(
                "group_update",
                {"group_id": group_id, "by": "user", "patch": {"unknown_key": 1}},
            )
            self.assertFalse(bad_update_resp.ok)
            self.assertEqual((bad_update_resp.error.code if bad_update_resp.error else ""), "invalid_patch")

            with tempfile.TemporaryDirectory(prefix="cccc_scope_") as scope_dir_raw:
                scope_dir = Path(scope_dir_raw)
                attach_resp, _ = self._call(
                    "attach",
                    {"group_id": group_id, "path": str(scope_dir), "by": "user"},
                )
                self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))
                scope_key = str((attach_resp.result or {}).get("scope_key") or "").strip()
                self.assertTrue(scope_key)

                use_resp, _ = self._call(
                    "group_use",
                    {"group_id": group_id, "path": str(scope_dir), "by": "user"},
                )
                self.assertTrue(use_resp.ok, getattr(use_resp, "error", None))
                self.assertEqual(str((use_resp.result or {}).get("active_scope_key") or ""), scope_key)

                detach_resp, _ = self._call(
                    "group_detach_scope",
                    {"group_id": group_id, "scope_key": scope_key, "by": "user"},
                )
                self.assertTrue(detach_resp.ok, getattr(detach_resp, "error", None))
                self.assertEqual(str((detach_resp.result or {}).get("group_id") or ""), group_id)

                show_resp, _ = self._call("group_show", {"group_id": group_id})
                self.assertTrue(show_resp.ok, getattr(show_resp, "error", None))
                show_group = (show_resp.result or {}).get("group") if isinstance(show_resp.result, dict) else {}
                self.assertIsInstance(show_group, dict)
                assert isinstance(show_group, dict)
                scopes = show_group.get("scopes") if isinstance(show_group.get("scopes"), list) else []
                self.assertEqual(len(scopes), 0)
        finally:
            cleanup()

    def test_group_delete_clears_active_and_removes_group(self) -> None:
        from cccc.kernel.active import load_active, set_active_group_id
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "delete-me", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            set_active_group_id(group_id)
            self.assertEqual(str(load_active().get("active_group_id") or ""), group_id)

            delete_resp, _ = self._call("group_delete", {"group_id": group_id, "by": "user"})
            self.assertTrue(delete_resp.ok, getattr(delete_resp, "error", None))
            self.assertEqual(str((delete_resp.result or {}).get("group_id") or ""), group_id)

            self.assertIsNone(load_group(group_id))
            self.assertEqual(str(load_active().get("active_group_id") or ""), "")

            show_resp, _ = self._call("group_show", {"group_id": group_id})
            self.assertFalse(show_resp.ok)
            self.assertEqual((show_resp.error.code if show_resp.error else ""), "group_not_found")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
