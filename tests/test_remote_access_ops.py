import os
import tempfile
import unittest
from unittest.mock import patch


class TestRemoteAccessOps(unittest.TestCase):
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

    def _with_env(self, key: str, value: str | None):
        old = os.environ.get(key)
        if value is None:
            os.environ.pop(key, None)
        else:
            os.environ[key] = value

        def cleanup() -> None:
            if old is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = old

        return cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_remote_access_state_defaults(self) -> None:
        _, cleanup = self._with_home()
        try:
            resp, should_stop = self._call("remote_access_state", {"by": "user"})
            self.assertFalse(should_stop)
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            remote = (resp.result or {}).get("remote_access") if isinstance(resp.result, dict) else {}
            self.assertIsInstance(remote, dict)
            assert isinstance(remote, dict)
            self.assertEqual(str(remote.get("provider") or ""), "off")
            self.assertEqual(bool(remote.get("enforce_web_token")), True)
            self.assertEqual(bool(remote.get("enabled")), False)
            self.assertEqual(str(remote.get("status") or ""), "stopped")
        finally:
            cleanup()

    def test_remote_access_configure_requires_user(self) -> None:
        _, cleanup = self._with_home()
        try:
            resp, _ = self._call("remote_access_configure", {"by": "peer1", "provider": "manual"})
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp, "error", None).code), "permission_denied")
        finally:
            cleanup()

    def test_remote_access_start_manual_requires_web_token(self) -> None:
        _, cleanup = self._with_home()
        cleanup_token = self._with_env("CCCC_WEB_TOKEN", None)
        try:
            cfg, _ = self._call("remote_access_configure", {"by": "user", "provider": "manual"})
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))

            start, _ = self._call("remote_access_start", {"by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(str(getattr(start, "error", None).code), "remote_access_invalid_config")
        finally:
            cleanup_token()
            cleanup()

    def test_remote_access_manual_start_stop_roundtrip(self) -> None:
        _, cleanup = self._with_home()
        cleanup_token = self._with_env("CCCC_WEB_TOKEN", "test-token")
        cleanup_host = self._with_env("CCCC_WEB_HOST", "192.168.68.52")
        cleanup_port = self._with_env("CCCC_WEB_PORT", "8848")
        try:
            cfg, _ = self._call("remote_access_configure", {"by": "user", "provider": "manual"})
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))

            start, _ = self._call("remote_access_start", {"by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))
            remote_started = (start.result or {}).get("remote_access") if isinstance(start.result, dict) else {}
            self.assertIsInstance(remote_started, dict)
            assert isinstance(remote_started, dict)
            self.assertEqual(str(remote_started.get("status") or ""), "running")
            self.assertEqual(bool(remote_started.get("enabled")), True)
            self.assertIn("192.168.68.52", str(remote_started.get("endpoint") or ""))

            stop, _ = self._call("remote_access_stop", {"by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))
            remote_stopped = (stop.result or {}).get("remote_access") if isinstance(stop.result, dict) else {}
            self.assertIsInstance(remote_stopped, dict)
            assert isinstance(remote_stopped, dict)
            self.assertEqual(str(remote_stopped.get("status") or ""), "stopped")
            self.assertEqual(bool(remote_stopped.get("enabled")), False)
        finally:
            cleanup_port()
            cleanup_host()
            cleanup_token()
            cleanup()

    def test_remote_access_configure_blocks_insecure_by_default(self) -> None:
        _, cleanup = self._with_home()
        cleanup_allow = self._with_env("CCCC_REMOTE_ALLOW_INSECURE", None)
        try:
            blocked, _ = self._call(
                "remote_access_configure",
                {"by": "user", "provider": "manual", "enforce_web_token": False},
            )
            self.assertFalse(blocked.ok)
            self.assertEqual(str(getattr(blocked, "error", None).code), "remote_access_invalid_config")
        finally:
            cleanup_allow()
            cleanup()

    def test_remote_access_configure_allows_insecure_with_explicit_override(self) -> None:
        _, cleanup = self._with_home()
        cleanup_allow = self._with_env("CCCC_REMOTE_ALLOW_INSECURE", "1")
        try:
            allowed, _ = self._call(
                "remote_access_configure",
                {"by": "user", "provider": "manual", "enforce_web_token": False},
            )
            self.assertTrue(allowed.ok, getattr(allowed, "error", None))
            remote = (allowed.result or {}).get("remote_access") if isinstance(allowed.result, dict) else {}
            self.assertIsInstance(remote, dict)
            assert isinstance(remote, dict)
            self.assertEqual(bool(remote.get("enforce_web_token")), False)
        finally:
            cleanup_allow()
            cleanup()

    def test_remote_access_configure_rejects_unsupported_mode(self) -> None:
        _, cleanup = self._with_home()
        try:
            resp, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "mode": "public_internet",
                },
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp, "error", None).code), "remote_access_invalid_config")
        finally:
            cleanup()

    def test_remote_access_start_manual_rejects_loopback_binding_without_override(self) -> None:
        _, cleanup = self._with_home()
        cleanup_token = self._with_env("CCCC_WEB_TOKEN", "token")
        cleanup_host = self._with_env("CCCC_WEB_HOST", "127.0.0.1")
        cleanup_public_url = self._with_env("CCCC_WEB_PUBLIC_URL", None)
        cleanup_loopback_override = self._with_env("CCCC_REMOTE_ALLOW_LOOPBACK", None)
        try:
            cfg, _ = self._call("remote_access_configure", {"by": "user", "provider": "manual"})
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))

            start, _ = self._call("remote_access_start", {"by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(str(getattr(start, "error", None).code), "remote_access_unreachable")
        finally:
            cleanup_loopback_override()
            cleanup_public_url()
            cleanup_host()
            cleanup_token()
            cleanup()

    def test_remote_access_state_surfaces_diagnostics_and_steps(self) -> None:
        _, cleanup = self._with_home()
        cleanup_host = self._with_env("CCCC_WEB_HOST", "127.0.0.1")
        cleanup_public_url = self._with_env("CCCC_WEB_PUBLIC_URL", None)
        cleanup_token = self._with_env("CCCC_WEB_TOKEN", None)
        try:
            cfg, _ = self._call(
                "remote_access_configure",
                {"by": "user", "provider": "manual", "enabled": True},
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            remote = (cfg.result or {}).get("remote_access") if isinstance(cfg.result, dict) else {}
            self.assertIsInstance(remote, dict)
            assert isinstance(remote, dict)
            self.assertEqual(str(remote.get("status") or ""), "misconfigured")
            diagnostics = remote.get("diagnostics") if isinstance(remote.get("diagnostics"), dict) else {}
            self.assertIsInstance(diagnostics, dict)
            assert isinstance(diagnostics, dict)
            self.assertEqual(bool(diagnostics.get("web_bind_loopback")), True)
            self.assertEqual(bool(diagnostics.get("web_token_present")), False)
            next_steps = remote.get("next_steps") if isinstance(remote.get("next_steps"), list) else []
            self.assertIsInstance(next_steps, list)
            self.assertGreaterEqual(len(next_steps), 1)
        finally:
            cleanup_token()
            cleanup_public_url()
            cleanup_host()
            cleanup()

    def test_remote_access_start_tailscale_reports_not_installed(self) -> None:
        _, cleanup = self._with_home()
        cleanup_token = self._with_env("CCCC_WEB_TOKEN", "token")
        cleanup_host = self._with_env("CCCC_WEB_HOST", "192.168.68.52")
        cleanup_public_url = self._with_env("CCCC_WEB_PUBLIC_URL", None)
        try:
            cfg, _ = self._call("remote_access_configure", {"by": "user", "provider": "tailscale"})
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            with patch("cccc.daemon.ops.remote_access_ops._tailscale_installed", return_value=False):
                start, _ = self._call("remote_access_start", {"by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(str(getattr(start, "error", None).code), "remote_access_not_installed")
        finally:
            cleanup_public_url()
            cleanup_host()
            cleanup_token()
            cleanup()

    def test_remote_access_state_tailscale_not_installed_status(self) -> None:
        _, cleanup = self._with_home()
        cleanup_host = self._with_env("CCCC_WEB_HOST", "192.168.68.52")
        try:
            cfg, _ = self._call(
                "remote_access_configure",
                {"by": "user", "provider": "tailscale", "enabled": True},
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            with patch("cccc.daemon.ops.remote_access_ops._tailscale_installed", return_value=False):
                state_resp, _ = self._call("remote_access_state", {"by": "user"})
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            remote = (state_resp.result or {}).get("remote_access") if isinstance(state_resp.result, dict) else {}
            self.assertIsInstance(remote, dict)
            assert isinstance(remote, dict)
            self.assertEqual(str(remote.get("status") or ""), "not_installed")
        finally:
            cleanup_host()
            cleanup()


if __name__ == "__main__":
    unittest.main()
