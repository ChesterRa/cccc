import os
import tempfile
import unittest


class TestDiagnosticsOps(unittest.TestCase):
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

    def test_debug_ops_require_developer_mode(self) -> None:
        _, cleanup = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": False}})
            self.assertTrue(update.ok, getattr(update, "error", None))
            resp, _ = self._call("debug_snapshot", {"by": "user"})
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp, "error", None).code), "developer_mode_required")
        finally:
            cleanup()

    def test_debug_tail_logs_invalid_component(self) -> None:
        _, cleanup = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": True}})
            self.assertTrue(update.ok, getattr(update, "error", None))

            tail, _ = self._call("debug_tail_logs", {"by": "user", "component": "unknown"})
            self.assertFalse(tail.ok)
            self.assertEqual(str(getattr(tail, "error", None).code), "invalid_component")
        finally:
            cleanup()

    def test_debug_clear_logs_im_requires_group(self) -> None:
        _, cleanup = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": True}})
            self.assertTrue(update.ok, getattr(update, "error", None))

            clear, _ = self._call("debug_clear_logs", {"by": "user", "component": "im"})
            self.assertFalse(clear.ok)
            self.assertEqual(str(getattr(clear, "error", None).code), "missing_group_id")
        finally:
            cleanup()

    def test_try_handle_unknown_diagnostics_op_returns_none(self) -> None:
        from cccc.daemon.ops.diagnostics_ops import try_handle_diagnostics_op

        resp = try_handle_diagnostics_op(
            "not_diagnostics",
            {},
            developer_mode_enabled=lambda: True,
            get_observability=lambda: {},
            effective_runner_kind=lambda runner: runner,
            throttle_debug_summary=lambda _group_id: {},
            can_read_terminal_transcript=lambda _group, _by, _target: False,
            pty_backlog_bytes=lambda: 1024,
        )
        self.assertIsNone(resp)


if __name__ == "__main__":
    unittest.main()
