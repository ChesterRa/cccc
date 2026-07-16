import os
import tempfile
import unittest
from dataclasses import asdict

from fastapi.testclient import TestClient


class TestWebAccessAuth(unittest.TestCase):
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

    def _create_probe_client(self) -> TestClient:
        from fastapi import Request
        from cccc.ports.web.app import create_app

        app = create_app()

        @app.get("/__test__/principal")
        async def principal_probe(request: Request) -> dict:
            principal = getattr(request.state, "principal", None)
            if principal is None:
                return {"present": False}
            payload = asdict(principal)
            payload["allowed_groups"] = list(principal.allowed_groups)
            payload["present"] = True
            return payload

        return TestClient(app)

    def test_web_access_session_reports_open_access_before_tokens_exist(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_probe_client()
            resp = client.get("/api/v1/web_access/session")
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            session = ((body.get("result") or {}).get("web_access_session") or {})
            self.assertEqual(bool(session.get("login_active")), False)
            self.assertEqual(bool(session.get("current_browser_signed_in")), False)
            self.assertEqual(int(session.get("access_token_count") or 0), 0)
            self.assertTrue(bool(session.get("can_access_global_settings")))
            runtime_visibility = session.get("runtime_visibility") if isinstance(session.get("runtime_visibility"), dict) else {}
            self.assertEqual(str(runtime_visibility.get("peer_runtime") or ""), "visible")
            self.assertEqual(str(runtime_visibility.get("assistant_runtime") or ""), "hidden")
        finally:
            cleanup()

    def test_web_access_session_reports_signed_in_browser(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            created = create_access_token("admin-user", allowed_groups=["g-1"], is_admin=True)
            token = str(created.get("token") or "")
            client = self._create_probe_client()
            resp = client.get("/api/v1/web_access/session", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            session = ((body.get("result") or {}).get("web_access_session") or {})
            self.assertEqual(bool(session.get("login_active")), True)
            self.assertEqual(bool(session.get("current_browser_signed_in")), True)
            self.assertEqual(str(session.get("user_id") or ""), "admin-user")
            self.assertEqual(bool(session.get("is_admin")), True)
            self.assertEqual(session.get("allowed_groups"), [])
            self.assertEqual(int(session.get("access_token_count") or 0), 1)
            self.assertTrue(bool(session.get("can_access_global_settings")))
        finally:
            cleanup()

    def test_web_access_session_reports_locked_management_for_non_admin_browser_when_tokens_exist(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.settings import update_observability_settings

        _, cleanup = self._with_home()
        try:
            update_observability_settings(
                {
                    "runtime_visibility": {
                        "peer_runtime": "hidden",
                        "assistant_runtime": "hidden",
                    }
                }
            )
            create_access_token("admin-user", is_admin=True)
            member = create_access_token("member-user", is_admin=False)
            member_token = str(member.get("token") or "")
            client = self._create_probe_client()
            resp = client.get("/api/v1/web_access/session", headers={"Authorization": f"Bearer {member_token}"})
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            session = ((body.get("result") or {}).get("web_access_session") or {})
            self.assertEqual(bool(session.get("login_active")), True)
            self.assertEqual(bool(session.get("current_browser_signed_in")), True)
            self.assertEqual(bool(session.get("is_admin")), False)
            self.assertEqual(int(session.get("access_token_count") or 0), 2)
            self.assertFalse(bool(session.get("can_access_global_settings")))
            runtime_visibility = session.get("runtime_visibility") if isinstance(session.get("runtime_visibility"), dict) else {}
            self.assertEqual(str(runtime_visibility.get("peer_runtime") or ""), "hidden")
            self.assertEqual(str(runtime_visibility.get("assistant_runtime") or ""), "hidden")
        finally:
            cleanup()

    def test_request_without_access_token_gets_anonymous_principal(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_probe_client()
            resp = client.get("/__test__/principal")
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("present"))
            self.assertEqual(str(body.get("kind") or ""), "anonymous")
        finally:
            cleanup()

    def test_web_access_logout_with_cookie_only_clears_session(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            created = create_access_token("member-user", is_admin=False)
            token = str(created.get("token") or "")
            client = self._create_probe_client()
            client.cookies.set("cccc_access_token", token)
            resp = client.post("/api/v1/web_access/logout")
            self.assertEqual(resp.status_code, 200)
            set_cookie = str(resp.headers.get("set-cookie") or "")
            self.assertIn("cccc_access_token=""", set_cookie)
            self.assertIn("Max-Age=0", set_cookie)
            follow = client.get("/api/v1/web_access/session")
            self.assertEqual(follow.status_code, 401)
        finally:
            cleanup()

    def test_web_access_logout_clears_cookie_without_rebinding_token(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            created = create_access_token("admin-user", is_admin=True)
            token = str(created.get("token") or "")
            client = self._create_probe_client()
            resp = client.post("/api/v1/web_access/logout", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(bool((body.get("result") or {}).get("signed_out")))
            set_cookie = str(resp.headers.get("set-cookie") or "")
            self.assertIn("cccc_access_token=""", set_cookie)
            self.assertIn("Max-Age=0", set_cookie)
            self.assertNotIn(token, set_cookie)
        finally:
            cleanup()

    def test_valid_access_token_resolves_principal_and_sets_cookie(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            created = create_access_token("user-a", allowed_groups=["g-1"], is_admin=False)
            token = str(created.get("token") or "")
            client = self._create_probe_client()
            resp = client.get("/__test__/principal", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertEqual(str(body.get("kind") or ""), "user")
            self.assertEqual(str(body.get("user_id") or ""), "user-a")
            self.assertEqual(body.get("allowed_groups"), ["g-1"])
            self.assertFalse(bool(body.get("is_admin")))
            self.assertIn("cccc_access_token=", str(resp.headers.get("set-cookie") or ""))
        finally:
            cleanup()

    def test_stale_cookie_is_ignored_when_no_access_token_is_configured(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_probe_client()
            client.cookies.set("cccc_access_token", "stale-cookie")
            resp = client.get("/__test__/principal")
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertEqual(str(body.get("kind") or ""), "anonymous")
            self.assertIn("cccc_access_token=\"\"", str(resp.headers.get("set-cookie") or ""))
        finally:
            cleanup()
