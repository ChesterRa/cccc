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

    def test_membership_status_returns_global_account_fields(self) -> None:
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
            self.assertNotIn("connector_url", membership)
            self.assertIn("account_origin", membership)
            self.assertFalse(bool(membership.get("logged_in")))
        finally:
            cleanup()

    def test_account_connection_routes_stay_thin_and_user_scoped(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            created = create_access_token("admin-user", is_admin=True)
            token = str(created.get("token") or "")
            seen: list[dict] = []

            def call(req: dict):
                seen.append(req)
                return {
                    "ok": True,
                    "result": {"membership": {"logged_in": False}},
                }

            with patch("cccc.ports.web.app.call_daemon", side_effect=call):
                app = create_app()
                with TestClient(app) as client:
                    for path in (
                        "/api/v1/membership/login",
                        "/api/v1/membership/login/poll",
                        "/api/v1/membership/logout",
                    ):
                        response = client.post(
                            path,
                            headers={"Authorization": f"Bearer {token}"},
                        )
                        self.assertEqual(response.status_code, 200)

            membership_ops = [
                request for request in seen if request["op"].startswith("membership_")
            ]
            self.assertEqual(
                [request["op"] for request in membership_ops],
                ["membership_login", "membership_login_poll", "membership_logout"],
            )
            self.assertTrue(
                all(request["args"] == {"by": "user"} for request in membership_ops)
            )
        finally:
            cleanup()
