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

            update, _ = self._call(
                "observability_update",
                {
                    "by": "user",
                    "patch": {
                        "developer_mode": True,
                        "logger_levels": {
                            "asyncio": "warning",
                            "cccc.daemon.group_space_ops": "debug",
                            "": "info",
                            "httpx": "bogus",
                        },
                        "runtime_visibility": {
                            "peer_runtime": "hidden",
                            "assistant_runtime": "hidden",
                        },
                    },
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))

            get, _ = self._call("observability_get", {})
            self.assertTrue(get.ok, getattr(get, "error", None))
            obs = (get.result or {}).get("observability") if isinstance(get.result, dict) else {}
            self.assertIsInstance(obs, dict)
            assert isinstance(obs, dict)
            self.assertEqual(bool(obs.get("developer_mode")), True)
            self.assertEqual(
                obs.get("logger_levels"),
                {
                    "asyncio": "WARNING",
                    "cccc.daemon.group_space_ops": "DEBUG",
                },
            )
            runtime_visibility = obs.get("runtime_visibility") if isinstance(obs.get("runtime_visibility"), dict) else {}
            self.assertEqual(str(runtime_visibility.get("peer_runtime") or ""), "hidden")
            self.assertEqual(str(runtime_visibility.get("assistant_runtime") or ""), "hidden")
        finally:
            cleanup()

    def test_observability_runtime_visibility_migrates_legacy_assistant_key(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.settings import save_settings

            save_settings(
                {
                    "observability": {
                        "runtime_visibility": {
                            "pet_runtime": "hidden",
                        }
                    }
                }
            )

            get, _ = self._call("observability_get", {})
            self.assertTrue(get.ok, getattr(get, "error", None))
            obs = (get.result or {}).get("observability") if isinstance(get.result, dict) else {}
            self.assertIsInstance(obs, dict)
            assert isinstance(obs, dict)
            runtime_visibility = obs.get("runtime_visibility") if isinstance(obs.get("runtime_visibility"), dict) else {}
            self.assertEqual(str(runtime_visibility.get("assistant_runtime") or ""), "hidden")
            self.assertNotIn("pet_runtime", runtime_visibility)
        finally:
            cleanup()

    def test_observability_cache_is_scoped_to_current_home(self) -> None:
        _, cleanup_first = self._with_home()
        try:
            update, _ = self._call("observability_update", {"by": "user", "patch": {"developer_mode": True}})
            self.assertTrue(update.ok, getattr(update, "error", None))
        finally:
            cleanup_first()

        _, cleanup_second = self._with_home()
        try:
            get, _ = self._call("observability_get", {})
            self.assertTrue(get.ok, getattr(get, "error", None))
            obs = (get.result or {}).get("observability") if isinstance(get.result, dict) else {}
            self.assertIsInstance(obs, dict)
            assert isinstance(obs, dict)
            self.assertEqual(bool(obs.get("developer_mode")), False)
        finally:
            cleanup_second()

    def test_branding_update_permissions_and_roundtrip(self) -> None:
        _, cleanup = self._with_home()
        try:
            denied, _ = self._call("branding_update", {"by": "peer1", "patch": {"product_name": "Acme"}})
            self.assertFalse(denied.ok)
            self.assertEqual(str(getattr(denied, "error", None).code), "permission_denied")

            update, _ = self._call("branding_update", {"by": "user", "patch": {"product_name": "Acme Console"}})
            self.assertTrue(update.ok, getattr(update, "error", None))

            get, _ = self._call("branding_get", {})
            self.assertTrue(get.ok, getattr(get, "error", None))
            branding = (get.result or {}).get("branding") if isinstance(get.result, dict) else {}
            self.assertIsInstance(branding, dict)
            assert isinstance(branding, dict)
            self.assertEqual(str(branding.get("product_name") or ""), "Acme Console")
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
                get_web_branding=lambda: {},
                update_web_branding_settings=lambda patch: patch,
            )
        )


if __name__ == "__main__":
    unittest.main()
