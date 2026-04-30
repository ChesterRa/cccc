import hashlib
import os
import tempfile
import unittest

from fastapi.testclient import TestClient


class TestAccessTokenRoutes(unittest.TestCase):
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

    def _create_client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def test_list_access_tokens_no_auth_when_no_tokens(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_client()
            resp = client.get("/api/v1/access-tokens")
            self.assertEqual(resp.status_code, 200)
            data = resp.json()
            self.assertTrue(data.get("ok"))
            self.assertEqual(data["result"]["access_tokens"], [])
        finally:
            cleanup()

    def test_create_and_list_access_tokens(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")

            client = self._create_client()
            resp = client.post(
                "/api/v1/access-tokens",
                json={"user_id": "test-user", "allowed_groups": ["g1"], "is_admin": False},
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 200)
            data = resp.json()
            created = data["result"]["access_token"]
            self.assertEqual(str(created.get("user_id") or ""), "test-user")
            self.assertEqual(created.get("allowed_groups"), ["g1"])
            self.assertTrue(str(created.get("token") or "").startswith("acc_"))

            resp = client.get("/api/v1/access-tokens", headers={"Authorization": f"Bearer {admin_token}"})
            self.assertEqual(resp.status_code, 200)
            items = resp.json()["result"]["access_tokens"]
            self.assertEqual(len(items), 2)
            for item in items:
                self.assertIn("token_id", item)
                self.assertIn("token_preview", item)
                self.assertNotIn("token", item)
        finally:
            cleanup()

    def test_first_admin_access_token_sets_login_cookie(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_client()
            resp = client.post(
                "/api/v1/access-tokens",
                json={"user_id": "first-admin", "is_admin": True},
            )
            self.assertEqual(resp.status_code, 200)
            created = resp.json()["result"]["access_token"]
            token = str(created.get("token") or "")
            self.assertTrue(token.startswith("acc_"))
            set_cookie = str(resp.headers.get("set-cookie") or "")
            self.assertIn("cccc_access_token=", set_cookie)
            self.assertIn(token, set_cookie)

            session = client.get("/api/v1/web_access/session")
            self.assertEqual(session.status_code, 200)
            data = ((session.json().get("result") or {}).get("web_access_session") or {})
            self.assertTrue(bool(data.get("current_browser_signed_in")))
            self.assertTrue(bool(data.get("can_access_global_settings")))
            self.assertEqual(str(data.get("user_id") or ""), "first-admin")
        finally:
            cleanup()

    def test_first_admin_access_token_cookie_honors_forwarded_https(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_client()
            resp = client.post(
                "/api/v1/access-tokens",
                json={"user_id": "first-admin", "is_admin": True},
                headers={"x-forwarded-proto": "https"},
            )
            self.assertEqual(resp.status_code, 200)
            set_cookie = str(resp.headers.get("set-cookie") or "").lower()
            self.assertIn("cccc_access_token=", set_cookie)
            self.assertIn("secure", set_cookie)
            self.assertIn("samesite=none", set_cookie)
        finally:
            cleanup()

    def test_admin_access_token_ignores_allowed_groups_on_create(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            client = self._create_client()
            resp = client.post(
                "/api/v1/access-tokens",
                json={"user_id": "scoped-admin", "allowed_groups": ["g1", "g2"], "is_admin": True},
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 200)
            created = resp.json()["result"]["access_token"]
            self.assertEqual(created.get("allowed_groups"), [])
            self.assertTrue(bool(created.get("is_admin")))
        finally:
            cleanup()

    def test_admin_access_token_ignores_allowed_groups_on_update(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            target = create_access_token("other-admin", is_admin=True)
            target_token = str(target.get("token") or "")
            target_token_id = hashlib.sha256(target_token.encode("utf-8")).hexdigest()[:16]

            client = self._create_client()
            resp = client.patch(
                f"/api/v1/access-tokens/{target_token_id}",
                json={"allowed_groups": ["g1", "g2"]},
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 200)
            updated = resp.json()["result"]["access_token"]
            self.assertEqual(updated.get("allowed_groups"), [])
            self.assertTrue(bool(updated.get("is_admin")))
        finally:
            cleanup()

    def test_delete_access_token_by_token_id(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            target = create_access_token("to-delete", is_admin=False)
            target_token = str(target.get("token") or "")
            target_token_id = hashlib.sha256(target_token.encode("utf-8")).hexdigest()[:16]

            client = self._create_client()
            resp = client.delete(
                f"/api/v1/access-tokens/{target_token_id}",
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 200)
            self.assertTrue(resp.json().get("ok"))

            resp = client.get("/api/v1/access-tokens", headers={"Authorization": f"Bearer {admin_token}"})
            items = resp.json()["result"]["access_tokens"]
            users = [str(item.get("user_id") or "") for item in items]
            self.assertNotIn("to-delete", users)
        finally:
            cleanup()

    def test_delete_current_session_access_token_marks_session_deleted(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            create_access_token("other-admin", is_admin=True)

            client = self._create_client()
            resp = client.delete(
                "/api/v1/access-tokens/" + hashlib.sha256(admin_token.encode("utf-8")).hexdigest()[:16],
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 200)
            data = resp.json()
            self.assertTrue(data.get("ok"))
            self.assertTrue(bool((data.get("result") or {}).get("deleted_current_session")))
            self.assertTrue(bool((data.get("result") or {}).get("access_tokens_remain")))
        finally:
            cleanup()

    def test_delete_nonexistent_access_token_returns_404(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            client = self._create_client()
            resp = client.delete(
                "/api/v1/access-tokens/0123456789abcdef",
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 404)
        finally:
            cleanup()

    def test_full_raw_token_is_not_a_valid_route_identifier(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            target = create_access_token("to-delete-compat", is_admin=False)
            raw_token = str(target.get("token") or "")
            client = self._create_client()
            resp = client.delete(
                f"/api/v1/access-tokens/{raw_token}",
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 400)
        finally:
            cleanup()


    def test_scoped_access_token_requires_allowed_groups_on_create(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            client = self._create_client()
            resp = client.post(
                "/api/v1/access-tokens",
                json={"user_id": "scoped-user", "is_admin": False, "allowed_groups": []},
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 400)
            self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "invalid_request")
        finally:
            cleanup()

    def test_scoped_access_token_requires_allowed_groups_on_update(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            target = create_access_token("member-user", is_admin=False, allowed_groups=["g1"])
            target_token = str(target.get("token") or "")
            target_token_id = hashlib.sha256(target_token.encode("utf-8")).hexdigest()[:16]
            client = self._create_client()
            resp = client.patch(
                f"/api/v1/access-tokens/{target_token_id}",
                json={"allowed_groups": []},
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 400)
            self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "invalid_request")
        finally:
            cleanup()

    def test_admin_token_cannot_be_downgraded_without_allowed_groups(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            admin = create_access_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            target = create_access_token("other-admin", is_admin=True)
            target_token = str(target.get("token") or "")
            target_token_id = hashlib.sha256(target_token.encode("utf-8")).hexdigest()[:16]
            client = self._create_client()
            resp = client.patch(
                f"/api/v1/access-tokens/{target_token_id}",
                json={"is_admin": False},
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 400)
            self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "invalid_request")
        finally:
            cleanup()

    def test_non_admin_cannot_create_or_list_access_tokens(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            user = create_access_token("regular-user", is_admin=False)
            token = str(user.get("token") or "")
            client = self._create_client()
            resp_create = client.post(
                "/api/v1/access-tokens",
                json={"user_id": "new-user"},
                headers={"Authorization": f"Bearer {token}"},
            )
            self.assertEqual(resp_create.status_code, 403)
            resp_list = client.get("/api/v1/access-tokens", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp_list.status_code, 403)
        finally:
            cleanup()

    def test_first_access_token_must_be_admin(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._create_client()
            resp = client.post("/api/v1/access-tokens", json={"user_id": "first-user", "is_admin": False})
            self.assertEqual(resp.status_code, 400)
            body = resp.json()
            self.assertEqual(str((body.get("error") or {}).get("code") or ""), "admin_required_first")
        finally:
            cleanup()
