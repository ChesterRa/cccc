import os
import tempfile
import unittest
from dataclasses import asdict
from types import SimpleNamespace

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

        @app.post("/__test__/write")
        async def write_probe(request: Request) -> dict:
            principal = getattr(request.state, "principal", None)
            return {"user_id": str(getattr(principal, "user_id", "") or "")}

        return TestClient(app)

    def test_websocket_origin_requires_same_origin(self) -> None:
        from cccc.ports.web.middleware import _websocket_origin_allowed

        base = {
            "type": "websocket",
            "headers": [
                (b"host", b"cccc.example"),
                (b"x-forwarded-proto", b"https"),
            ],
        }
        self.assertTrue(
            _websocket_origin_allowed(
                {**base, "headers": [*base["headers"], (b"origin", b"https://cccc.example")]}
            )
        )
        self.assertFalse(
            _websocket_origin_allowed(
                {**base, "headers": [*base["headers"], (b"origin", b"https://evil.example")]}
            )
        )
        proxied = {
            "type": "websocket",
            "headers": [
                (b"host", b"127.0.0.1:8848"),
                (b"x-forwarded-host", b"localhost:5555"),
                (b"x-forwarded-proto", b"http"),
                (b"origin", b"http://localhost:5555"),
            ],
        }
        self.assertTrue(_websocket_origin_allowed(proxied))

        forwarded = {
            "type": "websocket",
            "headers": [
                (b"host", b"127.0.0.1:8848"),
                (b"forwarded", b'for=192.0.2.1;proto=https;host="cccc.example"'),
                (b"origin", b"https://cccc.example"),
            ],
        }
        self.assertTrue(_websocket_origin_allowed(forwarded))

        chained = {
            "type": "websocket",
            "headers": [
                (b"x-forwarded-host", b"cccc.example, 127.0.0.1:8848"),
                (b"x-forwarded-proto", b"https, http"),
                (b"origin", b"https://cccc.example"),
            ],
        }
        self.assertTrue(_websocket_origin_allowed(chained))

        direct_tls = {
            "type": "websocket",
            "scheme": "wss",
            "headers": [
                (b"host", b"cccc.example"),
                (b"origin", b"https://cccc.example"),
            ],
        }
        self.assertTrue(_websocket_origin_allowed(direct_tls))

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

    def test_non_api_probe_keeps_anonymous_principal_for_static_ui_compatibility(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_probe_client()
            resp = client.get("/__test__/principal")
            self.assertEqual(resp.status_code, 200)
            self.assertEqual(resp.json().get("kind"), "anonymous")
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
            resp = client.post(
                "/api/v1/web_access/logout",
                headers={"Origin": "http://testserver"},
            )
            self.assertEqual(resp.status_code, 200)
            set_cookie = str(resp.headers.get("set-cookie") or "")
            self.assertIn("cccc_access_token=""", set_cookie)
            self.assertIn("Max-Age=0", set_cookie)
            follow = client.get("/api/v1/web_access/session")
            self.assertEqual(follow.status_code, 200)
            session = ((follow.json().get("result") or {}).get("web_access_session") or {})
            self.assertFalse(bool(session.get("current_browser_signed_in")))
        finally:
            cleanup()

    def test_cookie_authenticated_writes_require_an_exact_origin(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            token = str(create_access_token("admin-user", is_admin=True).get("token") or "")
            client = self._create_probe_client()
            client.cookies.set("cccc_access_token", token)

            same_origin = client.post(
                "/__test__/write",
                headers={"Origin": "http://testserver"},
            )
            self.assertEqual(same_origin.status_code, 200)

            same_origin_referer = client.post(
                "/__test__/write",
                headers={"Referer": "http://testserver/ui/"},
            )
            self.assertEqual(same_origin_referer.status_code, 200)

            cross_origin = client.post(
                "/__test__/write",
                headers={"Origin": "http://sibling.example"},
            )
            self.assertEqual(cross_origin.status_code, 403)
            self.assertEqual(
                (cross_origin.json().get("error") or {}).get("code"),
                "csrf_origin_invalid",
            )

            missing_origin = client.post("/__test__/write")
            self.assertEqual(missing_origin.status_code, 403)
            self.assertEqual(
                (missing_origin.json().get("error") or {}).get("code"),
                "csrf_origin_invalid",
            )

            bearer = self._create_probe_client().post(
                "/__test__/write",
                headers={"Authorization": f"Bearer {token}"},
            )
            self.assertEqual(bearer.status_code, 200)
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

    def test_query_token_cannot_replace_cookie_or_authenticate_websocket(
        self,
    ) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.ports.web.schemas import resolve_websocket_principal

        _, cleanup = self._with_home()
        try:
            stale = create_access_token("stale-user", is_admin=True)
            current = create_access_token("current-user", is_admin=True)
            stale_token = str(stale.get("token") or "")
            current_token = str(current.get("token") or "")
            client = self._create_probe_client()
            client.cookies.set("cccc_access_token", stale_token)

            response = client.get(f"/api/v1/web_access/session?token={current_token}")
            self.assertEqual(response.status_code, 200)
            session = ((response.json().get("result") or {}).get("web_access_session") or {})
            self.assertEqual(session.get("user_id"), "stale-user")
            self.assertNotIn(current_token, str(response.headers.get("set-cookie") or ""))

            websocket = SimpleNamespace(
                headers={},
                cookies={"cccc_access_token": stale_token},
                query_params={"token": current_token},
            )
            self.assertEqual(
                resolve_websocket_principal(websocket).user_id, "stale-user"
            )
        finally:
            cleanup()

    def test_non_session_query_token_is_ignored_in_favor_of_a_valid_cookie(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            token = str(create_access_token("admin", is_admin=True).get("token") or "")
            client = self._create_probe_client()
            client.cookies.set("cccc_access_token", token)

            response = client.get("/__test__/principal?token=invalid")

            self.assertEqual(response.status_code, 200)
            self.assertEqual(response.json().get("user_id"), "admin")
        finally:
            cleanup()

    def test_stale_cookie_is_rejected_when_no_access_token_is_configured(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_probe_client()
            client.cookies.set("cccc_access_token", "stale-cookie")
            resp = client.get("/__test__/principal")
            self.assertEqual(resp.status_code, 401)
        finally:
            cleanup()
