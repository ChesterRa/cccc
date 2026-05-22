import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestRemoteAccessOps(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        old_web_host = os.environ.get("CCCC_WEB_HOST")
        old_web_port = os.environ.get("CCCC_WEB_PORT")
        old_web_token = os.environ.get("CCCC_WEB_TOKEN")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td
        os.environ.pop("CCCC_WEB_HOST", None)
        os.environ.pop("CCCC_WEB_PORT", None)
        os.environ.pop("CCCC_WEB_TOKEN", None)

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            for key, old_value in (
                ("CCCC_WEB_HOST", old_web_host),
                ("CCCC_WEB_PORT", old_web_port),
                ("CCCC_WEB_TOKEN", old_web_token),
            ):
                if old_value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = old_value

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
            self.assertEqual(str(remote.get("provider") or ""), "off")
            self.assertEqual(bool(remote.get("require_access_token")), True)
            self.assertEqual(bool(remote.get("enabled")), False)
            self.assertEqual(str(remote.get("status") or ""), "stopped")
            cfg = remote.get("config") if isinstance(remote.get("config"), dict) else {}
            self.assertEqual(str(cfg.get("web_host") or ""), "127.0.0.1")
            self.assertEqual(int(cfg.get("web_port") or 0), 8848)
            self.assertEqual(bool(cfg.get("access_token_configured")), False)
            self.assertEqual(int(cfg.get("access_token_count") or 0), 0)
            self.assertEqual(remote.get("next_steps"), [])
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

    def test_remote_access_start_manual_rejects_loopback_binding_before_remote_use(self) -> None:
        _, cleanup = self._with_home()
        try:
            cfg, _ = self._call("remote_access_configure", {"by": "user", "provider": "manual"})
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            start, _ = self._call("remote_access_start", {"by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(str(getattr(start, "error", None).code), "remote_access_unreachable")
        finally:
            cleanup()

    def test_remote_access_manual_start_stop_roundtrip(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            create_access_token("admin-user", is_admin=True)
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "web_host": "192.168.68.52",
                    "web_port": 8848,
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))

            start, _ = self._call("remote_access_start", {"by": "user"})
            self.assertTrue(start.ok, getattr(start, "error", None))
            remote_started = (start.result or {}).get("remote_access") if isinstance(start.result, dict) else {}
            self.assertEqual(str(remote_started.get("status") or ""), "running")
            self.assertEqual(bool(remote_started.get("enabled")), True)
            self.assertIn("192.168.68.52", str(remote_started.get("endpoint") or ""))

            stop, _ = self._call("remote_access_stop", {"by": "user"})
            self.assertTrue(stop.ok, getattr(stop, "error", None))
            remote_stopped = (stop.result or {}).get("remote_access") if isinstance(stop.result, dict) else {}
            self.assertEqual(str(remote_stopped.get("status") or ""), "stopped")
            self.assertEqual(bool(remote_stopped.get("enabled")), False)
        finally:
            cleanup()

    def test_remote_access_configure_allows_insecure_private_binding_by_default(self) -> None:
        _, cleanup = self._with_home()
        try:
            allowed, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "require_access_token": False,
                    "web_host": "192.168.68.52",
                },
            )
            self.assertTrue(allowed.ok, getattr(allowed, "error", None))
            remote = (allowed.result or {}).get("remote_access") if isinstance(allowed.result, dict) else {}
            self.assertEqual(bool(remote.get("require_access_token")), False)
        finally:
            cleanup()

    def test_remote_access_configure_allows_insecure_with_override(self) -> None:
        _, cleanup = self._with_home()
        cleanup_allow = self._with_env("CCCC_REMOTE_ALLOW_INSECURE", "1")
        try:
            allowed, _ = self._call(
                "remote_access_configure",
                {"by": "user", "provider": "manual", "require_access_token": False},
            )
            self.assertTrue(allowed.ok, getattr(allowed, "error", None))
            remote = (allowed.result or {}).get("remote_access") if isinstance(allowed.result, dict) else {}
            self.assertEqual(bool(remote.get("require_access_token")), False)
        finally:
            cleanup_allow()
            cleanup()

    def test_remote_access_configure_allows_local_binding_without_remote_token_gate(self) -> None:
        _, cleanup = self._with_home()
        cleanup_allow = self._with_env("CCCC_REMOTE_ALLOW_INSECURE", None)
        try:
            allowed, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "require_access_token": False,
                    "web_host": "127.0.0.1",
                },
            )
            self.assertTrue(allowed.ok, getattr(allowed, "error", None))
            remote = (allowed.result or {}).get("remote_access") if isinstance(allowed.result, dict) else {}
            self.assertEqual(bool(remote.get("require_access_token")), False)
            diagnostics = remote.get("diagnostics") if isinstance(remote.get("diagnostics"), dict) else {}
            self.assertEqual(str(diagnostics.get("exposure_class") or ""), "local")
            self.assertEqual(bool(diagnostics.get("effective_require_access_token")), False)
            self.assertEqual(bool(diagnostics.get("access_token_requirement_satisfied")), True)
        finally:
            cleanup_allow()
            cleanup()

    def test_remote_access_configure_marks_restart_required_when_binding_changes(self) -> None:
        _, cleanup = self._with_home()
        try:
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "web_host": "0.0.0.0",
                    "web_port": 9001,
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            remote = (cfg.result or {}).get("remote_access") if isinstance(cfg.result, dict) else {}
            self.assertEqual(bool(remote.get("restart_required")), True)
        finally:
            cleanup()

    def test_remote_access_configure_blocks_insecure_public_url_even_with_override(self) -> None:
        _, cleanup = self._with_home()
        cleanup_allow = self._with_env("CCCC_REMOTE_ALLOW_INSECURE", "1")
        try:
            blocked, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "require_access_token": False,
                    "web_public_url": "https://cccc.example.com/ui/",
                },
            )
            self.assertFalse(blocked.ok)
            self.assertEqual(str(getattr(blocked, "error", None).code), "remote_access_invalid_config")
        finally:
            cleanup_allow()
            cleanup()

    def test_remote_access_configure_rejects_public_url_for_tailscale(self) -> None:
        _, cleanup = self._with_home()
        try:
            blocked, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "tailscale",
                    "web_public_url": "https://cccc.example.com/ui/",
                },
            )
            self.assertFalse(blocked.ok)
            self.assertEqual(str(getattr(blocked, "error", None).code), "remote_access_invalid_config")
        finally:
            cleanup()

    def test_remote_access_start_manual_rejects_loopback_binding_without_override(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        cleanup_loopback_override = self._with_env("CCCC_REMOTE_ALLOW_LOOPBACK", None)
        try:
            create_access_token("admin-user", is_admin=True)
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "web_host": "127.0.0.1",
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            start, _ = self._call("remote_access_start", {"by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(str(getattr(start, "error", None).code), "remote_access_unreachable")
        finally:
            cleanup_loopback_override()
            cleanup()

    def test_remote_access_state_surfaces_diagnostics_and_steps(self) -> None:
        _, cleanup = self._with_home()
        try:
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "enabled": True,
                    "web_host": "127.0.0.1",
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            remote = (cfg.result or {}).get("remote_access") if isinstance(cfg.result, dict) else {}
            self.assertEqual(str(remote.get("status") or ""), "misconfigured")
            diagnostics = remote.get("diagnostics") if isinstance(remote.get("diagnostics"), dict) else {}
            self.assertEqual(str(diagnostics.get("exposure_class") or ""), "local")
            self.assertEqual(bool(diagnostics.get("web_bind_loopback")), True)
            self.assertEqual(bool(diagnostics.get("access_token_present")), False)
            self.assertEqual(bool(diagnostics.get("effective_require_access_token")), False)
            self.assertEqual(str(remote.get("status_reason") or ""), "local_only")
            next_steps = remote.get("next_steps") if isinstance(remote.get("next_steps"), list) else []
            self.assertGreaterEqual(len(next_steps), 1)
            self.assertFalse(any("Click Start" in step for step in next_steps))
        finally:
            cleanup()

    def test_remote_access_state_reports_missing_access_token_reason_for_private_binding(self) -> None:
        _, cleanup = self._with_home()
        try:
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "enabled": True,
                    "web_host": "0.0.0.0",
                    "require_access_token": True,
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            remote = (cfg.result or {}).get("remote_access") if isinstance(cfg.result, dict) else {}
            self.assertEqual(str(remote.get("status") or ""), "misconfigured")
            self.assertEqual(str(remote.get("status_reason") or ""), "missing_access_token")
            next_steps = remote.get("next_steps") if isinstance(remote.get("next_steps"), list) else []
            self.assertTrue(any("Create an Admin Access Token" in step for step in next_steps))
        finally:
            cleanup()

    def test_remote_access_state_uses_remote_placeholder_endpoint_for_wildcard_host(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            create_access_token("admin-user", is_admin=True)
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "enabled": True,
                    "web_host": "0.0.0.0",
                    "web_port": 8848,
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            remote = (cfg.result or {}).get("remote_access") if isinstance(cfg.result, dict) else {}
            self.assertEqual(str(remote.get("endpoint") or ""), "http://<your-lan-ip>:8848/ui/")
        finally:
            cleanup()

    def test_remote_access_state_mentions_wsl_private_network_requirement(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch("cccc.daemon.ops.remote_access_ops._running_in_wsl", return_value=True):
                cfg, _ = self._call(
                    "remote_access_configure",
                    {
                        "by": "user",
                        "provider": "manual",
                        "enabled": True,
                        "web_host": "0.0.0.0",
                        "require_access_token": False,
                    },
                )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            remote = (cfg.result or {}).get("remote_access") if isinstance(cfg.result, dict) else {}
            diagnostics = remote.get("diagnostics") if isinstance(remote.get("diagnostics"), dict) else {}
            self.assertEqual(bool(diagnostics.get("running_in_wsl")), True)
            next_steps = remote.get("next_steps") if isinstance(remote.get("next_steps"), list) else []
            self.assertTrue(any("WSL2" in step and "mirrored networking" in step for step in next_steps))
        finally:
            cleanup()

    def test_remote_access_state_surfaces_supervised_live_runtime_mismatch(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.ports.web.runtime_control import write_web_runtime_state

        home, cleanup = self._with_home()
        try:
            create_access_token("admin-user", is_admin=True)
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "enabled": True,
                    "web_host": "0.0.0.0",
                    "web_port": 9001,
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            write_web_runtime_state(
                home=Path(home),
                pid=4321,
                host="127.0.0.1",
                port=8848,
                mode="normal",
                supervisor_managed=True,
                supervisor_pid=1234,
                launch_source="test",
            )
            state_resp, _ = self._call("remote_access_state", {"by": "user"})
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            remote = (state_resp.result or {}).get("remote_access") if isinstance(state_resp.result, dict) else {}
            diagnostics = remote.get("diagnostics") if isinstance(remote.get("diagnostics"), dict) else {}
            self.assertEqual(bool(remote.get("restart_required")), True)
            self.assertEqual(bool(remote.get("apply_supported")), True)
            self.assertEqual(bool(diagnostics.get("live_runtime_present")), True)
            self.assertEqual(str(diagnostics.get("live_runtime_host") or ""), "127.0.0.1")
            self.assertEqual(int(diagnostics.get("live_runtime_port") or 0), 8848)
            self.assertEqual(bool(diagnostics.get("live_runtime_matches_binding")), False)
        finally:
            cleanup()

    def test_remote_access_state_surfaces_unsupervised_live_runtime_mismatch(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.ports.web.runtime_control import write_web_runtime_state

        home, cleanup = self._with_home()
        try:
            create_access_token("admin-user", is_admin=True)
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "enabled": True,
                    "web_host": "0.0.0.0",
                    "web_port": 9001,
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            write_web_runtime_state(
                home=Path(home),
                pid=4321,
                host="127.0.0.1",
                port=8848,
                mode="normal",
                supervisor_managed=False,
                supervisor_pid=None,
                launch_source="test",
            )
            state_resp, _ = self._call("remote_access_state", {"by": "user"})
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            remote = (state_resp.result or {}).get("remote_access") if isinstance(state_resp.result, dict) else {}
            diagnostics = remote.get("diagnostics") if isinstance(remote.get("diagnostics"), dict) else {}
            self.assertEqual(bool(remote.get("restart_required")), True)
            self.assertEqual(bool(remote.get("apply_supported")), False)
            self.assertEqual(bool(diagnostics.get("live_runtime_present")), True)
            self.assertEqual(bool(diagnostics.get("live_runtime_matches_binding")), False)
            next_steps = remote.get("next_steps") if isinstance(remote.get("next_steps"), list) else []
            self.assertTrue(any("Restart the running CCCC Web service" in step for step in next_steps))
        finally:
            cleanup()

    def test_remote_access_start_tailscale_reports_not_installed(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            create_access_token("admin-user", is_admin=True)
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "tailscale",
                    "web_host": "192.168.68.52",
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            with patch("cccc.daemon.ops.remote_access_ops._tailscale_installed", return_value=False):
                start, _ = self._call("remote_access_start", {"by": "user"})
            self.assertFalse(start.ok)
            self.assertEqual(str(getattr(start, "error", None).code), "remote_access_not_installed")
        finally:
            cleanup()

    def test_remote_access_state_tailscale_not_installed_status(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            create_access_token("admin-user", is_admin=True)
            cfg, _ = self._call(
                "remote_access_configure",
                {"by": "user", "provider": "tailscale", "enabled": True, "web_host": "192.168.68.52"},
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            with patch("cccc.daemon.ops.remote_access_ops._tailscale_installed", return_value=False):
                state_resp, _ = self._call("remote_access_state", {"by": "user"})
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            remote = (state_resp.result or {}).get("remote_access") if isinstance(state_resp.result, dict) else {}
            self.assertEqual(str(remote.get("status") or ""), "not_installed")
        finally:
            cleanup()

    def test_remote_access_configure_reports_access_token_count(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            create_access_token("admin-user", is_admin=True)
            create_access_token("ops-user", is_admin=False)
            cfg, _ = self._call(
                "remote_access_configure",
                {
                    "by": "user",
                    "provider": "manual",
                    "web_host": "10.0.0.8",
                    "web_port": 8899,
                    "web_public_url": "https://cccc.example.com/ui/",
                },
            )
            self.assertTrue(cfg.ok, getattr(cfg, "error", None))
            remote = (cfg.result or {}).get("remote_access") if isinstance(cfg.result, dict) else {}
            cfg_doc = remote.get("config") if isinstance(remote.get("config"), dict) else {}
            self.assertEqual(str(cfg_doc.get("web_host") or ""), "10.0.0.8")
            self.assertEqual(int(cfg_doc.get("web_port") or 0), 8899)
            self.assertEqual(str(cfg_doc.get("web_public_url") or ""), "https://cccc.example.com/ui/")
            self.assertEqual(bool(cfg_doc.get("access_token_configured")), True)
            self.assertEqual(int(cfg_doc.get("access_token_count") or 0), 2)
        finally:
            cleanup()

    def test_remote_access_state_uses_env_binding_when_settings_absent(self) -> None:
        _, cleanup = self._with_home()
        cleanup_host = self._with_env("CCCC_WEB_HOST", "10.0.0.8")
        cleanup_port = self._with_env("CCCC_WEB_PORT", "8899")
        try:
            resp, should_stop = self._call("remote_access_state", {"by": "user"})
            self.assertFalse(should_stop)
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            remote = (resp.result or {}).get("remote_access") if isinstance(resp.result, dict) else {}
            cfg = remote.get("config") if isinstance(remote.get("config"), dict) else {}
            diagnostics = remote.get("diagnostics") if isinstance(remote.get("diagnostics"), dict) else {}
            self.assertEqual(str(cfg.get("web_host") or ""), "10.0.0.8")
            self.assertEqual(int(cfg.get("web_port") or 0), 8899)
            self.assertEqual(str(diagnostics.get("web_host_source") or ""), "env")
            self.assertEqual(str(diagnostics.get("web_port_source") or ""), "env")
        finally:
            cleanup_port()
            cleanup_host()
            cleanup()
