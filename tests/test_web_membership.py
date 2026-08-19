import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebMembershipRoutes(unittest.TestCase):
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

    def test_membership_status_returns_three_url_slots(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            created = create_access_token("admin-user", is_admin=True)
            token = str(created.get("token") or "")
            with patch(
                "cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon
            ):
                app = create_app()
                with TestClient(app) as client:
                    resp = client.get(
                        "/api/v1/membership",
                        headers={"Authorization": f"Bearer {token}"},
                    )
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("ok"))
            membership = (body.get("result") or {}).get("membership") or {}
            self.assertIn("logged_in", membership)
            self.assertIn("hostname", membership)
            self.assertIn("web_url", membership)
            self.assertIn("connector_url", membership)
            self.assertIn("account_origin", membership)
            self.assertFalse(bool(membership.get("logged_in")))
        finally:
            cleanup()
