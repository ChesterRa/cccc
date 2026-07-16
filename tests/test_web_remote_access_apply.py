import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebRemoteAccessApply(unittest.TestCase):
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

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

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

    def test_remote_access_apply_accepts_supervised_restart(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.settings import update_remote_access_settings
        from cccc.ports.web.app import create_app
        _, cleanup = self._with_home()
        cleanup_supervised = self._with_env("CCCC_WEB_SUPERVISED", "1")
        cleanup_host = self._with_env("CCCC_WEB_EFFECTIVE_HOST", "127.0.0.1")
        cleanup_port = self._with_env("CCCC_WEB_EFFECTIVE_PORT", "8848")
        cleanup_mode = self._with_env("CCCC_WEB_EFFECTIVE_MODE", "normal")
        try:
            created = create_access_token("admin-user", is_admin=True)
            token = str(created.get("token") or "")
            update_remote_access_settings({"provider": "manual", "enabled": True, "web_host": "0.0.0.0", "web_port": 9001})
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                app = create_app()
                called: list[str] = []
                with TestClient(app) as client:
                    client.app.state.request_web_restart = lambda: called.append("restart")
                    resp = client.post("/api/v1/remote_access/apply?by=user", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            result = body.get("result") or {}
            remote = result.get("remote_access") or {}
            self.assertEqual(bool(result.get("accepted")), True)
            self.assertEqual(bool(remote.get("restart_required")), True)
            self.assertEqual(bool(remote.get("apply_supported")), True)
            self.assertEqual(called, ["restart"])
        finally:
            cleanup_mode()
            cleanup_port()
            cleanup_host()
            cleanup_supervised()
            cleanup()

    def test_remote_access_apply_rejects_unsupervised_runtime(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.settings import update_remote_access_settings
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        cleanup_supervised = self._with_env("CCCC_WEB_SUPERVISED", None)
        cleanup_host = self._with_env("CCCC_WEB_EFFECTIVE_HOST", "127.0.0.1")
        cleanup_port = self._with_env("CCCC_WEB_EFFECTIVE_PORT", "8848")
        cleanup_mode = self._with_env("CCCC_WEB_EFFECTIVE_MODE", "normal")
        try:
            created = create_access_token("admin-user", is_admin=True)
            token = str(created.get("token") or "")
            update_remote_access_settings({"provider": "manual", "enabled": True, "web_host": "0.0.0.0", "web_port": 9001})
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                with TestClient(create_app()) as client:
                    resp = client.post("/api/v1/remote_access/apply?by=user", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 409)
            body = resp.json()
            error = body.get("error") or {}
            self.assertEqual(str((error.get("code") if isinstance(error, dict) else "") or ""), "web_apply_unsupported")
        finally:
            cleanup_mode()
            cleanup_port()
            cleanup_host()
            cleanup_supervised()
            cleanup()
