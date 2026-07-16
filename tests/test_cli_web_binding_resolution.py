import os
import tempfile
import unittest


class TestCliWebBindingResolution(unittest.TestCase):
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

    def test_resolve_web_server_binding_uses_settings_before_env(self) -> None:
        from cccc.cli.common import _resolve_web_server_binding
        from cccc.kernel.settings import update_remote_access_settings

        _, cleanup = self._with_home()
        cleanup_host = self._with_env("CCCC_WEB_HOST", "10.0.0.8")
        cleanup_port = self._with_env("CCCC_WEB_PORT", "9999")
        try:
            update_remote_access_settings({"web_host": "0.0.0.0", "web_port": 9001})
            self.assertEqual(_resolve_web_server_binding(), ("0.0.0.0", 9001))
        finally:
            cleanup_port()
            cleanup_host()
            cleanup()

    def test_resolve_web_server_binding_falls_back_to_env(self) -> None:
        from cccc.cli.common import _resolve_web_server_binding

        _, cleanup = self._with_home()
        cleanup_host = self._with_env("CCCC_WEB_HOST", "10.0.0.8")
        cleanup_port = self._with_env("CCCC_WEB_PORT", "9999")
        try:
            self.assertEqual(_resolve_web_server_binding(), ("10.0.0.8", 9999))
        finally:
            cleanup_port()
            cleanup_host()
            cleanup()

    def test_resolve_web_server_binding_uses_defaults_without_settings_or_env(self) -> None:
        from cccc.cli.common import _resolve_web_server_binding

        _, cleanup = self._with_home()
        cleanup_host = self._with_env("CCCC_WEB_HOST", None)
        cleanup_port = self._with_env("CCCC_WEB_PORT", None)
        try:
            self.assertEqual(_resolve_web_server_binding(), ("127.0.0.1", 8848))
        finally:
            cleanup_port()
            cleanup_host()
            cleanup()


    def test_update_settings_does_not_shadow_env_port(self) -> None:
        """Regression: toggling 'enabled' must not persist default web_port=8848,
        which would shadow CCCC_WEB_PORT env fallback."""
        from cccc.cli.common import _resolve_web_server_binding
        from cccc.kernel.settings import update_remote_access_settings

        _, cleanup = self._with_home()
        cleanup_host = self._with_env("CCCC_WEB_HOST", None)
        cleanup_port = self._with_env("CCCC_WEB_PORT", "7777")
        try:
            # Simulate UI toggling enabled without touching port
            update_remote_access_settings({"provider": "manual", "enabled": True, "web_host": "0.0.0.0"})
            # Port should still fall back to env, not the default 8848
            self.assertEqual(_resolve_web_server_binding(), ("0.0.0.0", 7777))
        finally:
            cleanup_port()
            cleanup_host()
            cleanup()


if __name__ == "__main__":
    unittest.main()
