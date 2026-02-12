import os
import tempfile
import unittest


class TestGroupBootstrapOps(unittest.TestCase):
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

    def test_group_create_and_attach_roundtrip(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "bootstrap", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call("attach", {"group_id": group_id, "path": ".", "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))
            scope_key = str((attach.result or {}).get("scope_key") or "").strip()
            self.assertTrue(scope_key)
        finally:
            cleanup()

    def test_missing_template_callbacks_returns_internal_error(self) -> None:
        from cccc.daemon.ops.group_bootstrap_ops import try_handle_group_bootstrap_op

        resp = try_handle_group_bootstrap_op("group_template_export", {})
        self.assertIsNotNone(resp)
        assert resp is not None
        self.assertFalse(resp.ok)
        self.assertEqual(str(getattr(resp, "error", None).code), "internal_error")

    def test_unknown_op_returns_none(self) -> None:
        from cccc.daemon.ops.group_bootstrap_ops import try_handle_group_bootstrap_op

        self.assertIsNone(try_handle_group_bootstrap_op("not_bootstrap", {}))


if __name__ == "__main__":
    unittest.main()
