import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebDaemonTransportDetails(unittest.TestCase):
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

        return cleanup

    def test_ping_503_preserves_daemon_transport_details(self) -> None:
        cleanup = self._with_home()
        try:
            from cccc.ports.web.app import create_app

            fake_resp = {
                "ok": False,
                "error": {
                    "code": "daemon_unavailable",
                    "message": "daemon unavailable",
                    "details": {
                        "phase": "read",
                        "reason": "timeout",
                        "transport": "unix",
                        "op": "ping",
                    },
                },
            }
            with patch("cccc.ports.web.app.call_daemon", return_value=fake_resp):
                with TestClient(create_app()) as client:
                    resp = client.get("/api/v1/ping")

            self.assertEqual(resp.status_code, 503)
            payload = resp.json()
            self.assertFalse(bool(payload.get("ok")))
            self.assertEqual(payload["error"]["code"], "daemon_unavailable")
            self.assertEqual(payload["error"]["message"], "ccccd unavailable")
            self.assertEqual(payload["error"]["details"]["phase"], "read")
            self.assertEqual(payload["error"]["details"]["reason"], "timeout")
            self.assertEqual(payload["error"]["details"]["transport"], "unix")
            self.assertEqual(payload["error"]["details"]["op"], "ping")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
