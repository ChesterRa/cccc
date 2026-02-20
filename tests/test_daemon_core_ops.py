import os
import tempfile
import unittest


class TestDaemonCoreOps(unittest.TestCase):
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

    def test_ping_and_shutdown(self) -> None:
        _, cleanup = self._with_home()
        try:
            ping, should_stop = self._call("ping", {})
            self.assertTrue(ping.ok, getattr(ping, "error", None))
            self.assertFalse(should_stop)
            result = ping.result if isinstance(ping.result, dict) else {}
            self.assertIsInstance(result, dict)
            assert isinstance(result, dict)
            capabilities = result.get("capabilities") if isinstance(result.get("capabilities"), dict) else {}
            self.assertEqual(bool(capabilities.get("events_stream")), True)
            self.assertEqual(bool(capabilities.get("remote_access")), True)

            shutdown, should_stop = self._call("shutdown", {})
            self.assertTrue(shutdown.ok, getattr(shutdown, "error", None))
            self.assertTrue(should_stop)
        finally:
            cleanup()

    def test_observability_update_permissions_and_roundtrip(self) -> None:
        _, cleanup = self._with_home()
        try:
            denied, _ = self._call("observability_update", {"by": "peer1", "patch": {"developer_mode": True}})
            self.assertFalse(denied.ok)
            self.assertEqual(str(getattr(denied, "error", None).code), "permission_denied")

            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": True}})
            self.assertTrue(update.ok, getattr(update, "error", None))

            get, _ = self._call("observability_get", {})
            self.assertTrue(get.ok, getattr(get, "error", None))
            obs = (get.result or {}).get("observability") if isinstance(get.result, dict) else {}
            self.assertIsInstance(obs, dict)
            assert isinstance(obs, dict)
            self.assertEqual(bool(obs.get("developer_mode")), True)
        finally:
            cleanup()

    def test_try_handle_unknown_daemon_core_op_returns_none(self) -> None:
        from cccc.daemon.ops.daemon_core_ops import try_handle_daemon_core_op

        self.assertIsNone(
            try_handle_daemon_core_op(
                "not_core",
                {},
                version="x",
                pid_provider=lambda: 1,
                now_iso=lambda: "now",
                get_observability=lambda: {},
                update_observability_settings=lambda patch: patch,
                apply_observability_settings=lambda _obs: None,
            )
        )


if __name__ == "__main__":
    unittest.main()
