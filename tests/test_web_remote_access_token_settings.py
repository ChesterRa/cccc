import os
import tempfile
import unittest
from dataclasses import asdict

from fastapi.testclient import TestClient


class TestWebPrincipalResolution(unittest.TestCase):
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
        old_value = os.environ.get(key)
        if value is None:
            os.environ.pop(key, None)
        else:
            os.environ[key] = value

        def cleanup() -> None:
            if old_value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = old_value

        return cleanup

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

    def test_request_without_token_gets_anonymous_principal(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_probe_client()
            resp = client.get("/__test__/principal")
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("present"))
            self.assertEqual(str(body.get("kind") or ""), "anonymous")
            self.assertEqual(str(body.get("user_id") or ""), "")
        finally:
            cleanup()

    def test_valid_user_token_resolves_principal_and_sets_cookie(self) -> None:
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            created = create_token("user-a", allowed_groups=["g-1"], is_admin=False)
            token = str(created.get("token") or "")
            client = self._create_probe_client()
            resp = client.get("/__test__/principal", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertEqual(str(body.get("kind") or ""), "user")
            self.assertEqual(str(body.get("user_id") or ""), "user-a")
            self.assertEqual(body.get("allowed_groups"), ["g-1"])
            self.assertEqual(bool(body.get("is_admin")), False)
            self.assertIn("cccc_web_token=", str(resp.headers.get("set-cookie") or ""))
        finally:
            cleanup()

    def test_legacy_web_token_resolves_admin_principal(self) -> None:
        from cccc.kernel.settings import update_remote_access_settings

        _, cleanup = self._with_home()
        try:
            update_remote_access_settings({"web_token": "legacy-token"})
            client = self._create_probe_client()
            resp = client.get("/__test__/principal", headers={"Authorization": "Bearer legacy-token"})
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("present"))
            self.assertEqual(str(body.get("kind") or ""), "user")
            self.assertEqual(str(body.get("user_id") or ""), "admin")
            self.assertEqual(body.get("allowed_groups"), [])
            self.assertEqual(bool(body.get("is_admin")), True)
            self.assertIn("cccc_web_token=legacy-token", str(resp.headers.get("set-cookie") or ""))
        finally:
            cleanup()

    def test_legacy_web_token_coexists_with_user_tokens(self) -> None:
        from cccc.kernel.settings import update_remote_access_settings
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            update_remote_access_settings({"web_token": "legacy-token"})
            created = create_token("user-a", allowed_groups=["g-1"], is_admin=False)
            user_token = str(created.get("token") or "")
            client = self._create_probe_client()

            legacy_resp = client.get("/__test__/principal", headers={"Authorization": "Bearer legacy-token"})
            self.assertEqual(legacy_resp.status_code, 200)
            self.assertEqual(str(legacy_resp.json().get("user_id") or ""), "admin")
            self.assertEqual(bool(legacy_resp.json().get("is_admin")), True)

            user_resp = client.get("/__test__/principal", headers={"Authorization": f"Bearer {user_token}"})
            self.assertEqual(user_resp.status_code, 200)
            body = user_resp.json()
            self.assertEqual(str(body.get("user_id") or ""), "user-a")
            self.assertEqual(body.get("allowed_groups"), ["g-1"])
            self.assertEqual(bool(body.get("is_admin")), False)
        finally:
            cleanup()

    def test_stale_cookie_is_ignored_when_no_token_is_configured(self) -> None:
        _, cleanup = self._with_home()
        cleanup_env = self._with_env("CCCC_WEB_TOKEN", None)
        try:
            client = self._create_probe_client()
            client.cookies.set("cccc_web_token", "stale-cookie")
            resp = client.get("/__test__/principal")
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("present"))
            self.assertEqual(str(body.get("kind") or ""), "anonymous")
            self.assertIn("cccc_web_token=""", str(resp.headers.get("set-cookie") or ""))
        finally:
            cleanup_env()
            cleanup()


if __name__ == "__main__":
    unittest.main()
