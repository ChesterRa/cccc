import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebRemoteAccessTokenFromSettings(unittest.TestCase):
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

    def test_api_requires_token_from_settings(self) -> None:
        from cccc.kernel.settings import update_remote_access_settings
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            update_remote_access_settings({"web_token": "tok_settings"})
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = TestClient(create_app())

                denied = client.get("/api/v1/remote_access")
                self.assertEqual(denied.status_code, 401)
                body = denied.json()
                self.assertFalse(body.get("ok"))
                self.assertEqual(str((body.get("error") or {}).get("code") or ""), "unauthorized")

                allowed = client.get("/api/v1/remote_access?token=tok_settings")
                self.assertEqual(allowed.status_code, 200)
                self.assertTrue(allowed.json().get("ok"))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()

