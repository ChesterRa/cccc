import os
import tempfile
import unittest
import hashlib
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebTokenRoutes(unittest.TestCase):
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

    def _create_client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def test_list_tokens_no_auth_when_no_tokens(self) -> None:
        """When no tokens exist, anonymous access is allowed (tokens_enabled=False)."""
        _, cleanup = self._with_home()
        cleanup_env = self._with_env("CCCC_WEB_TOKEN", None)
        try:
            client = self._create_client()
            resp = client.get("/api/v1/tokens")
            self.assertEqual(resp.status_code, 200)
            data = resp.json()
            self.assertTrue(data.get("ok"))
            self.assertEqual(data["result"]["tokens"], [])
        finally:
            cleanup_env()
            cleanup()

    def test_create_and_list_tokens(self) -> None:
        """Admin can create and list tokens."""
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            # Create an admin token first
            admin = create_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")

            client = self._create_client()
            # Create a new token via API
            resp = client.post(
                "/api/v1/tokens",
                json={"user_id": "test-user", "allowed_groups": ["g1"], "is_admin": False},
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 200)
            data = resp.json()
            self.assertTrue(data.get("ok"))
            created = data["result"]["token"]
            self.assertEqual(str(created.get("user_id") or ""), "test-user")
            self.assertEqual(created.get("allowed_groups"), ["g1"])
            new_token = str(created.get("token") or "")
            self.assertTrue(new_token.startswith("usr_"))

            # List tokens
            resp = client.get(
                "/api/v1/tokens",
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 200)
            tokens = resp.json()["result"]["tokens"]
            # Should have 2 tokens: admin + test-user
            self.assertEqual(len(tokens), 2)
            # token_preview should be present and masked
            for t in tokens:
                self.assertIn("token_preview", t)
                self.assertIn("token_id", t)
                self.assertIn("...", t["token_preview"])
                self.assertNotIn("token", t)
        finally:
            cleanup()

    def test_delete_token_by_token_id(self) -> None:
        """Admin can delete a token via token_id."""
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            admin = create_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            target = create_token("to-delete", is_admin=False)
            target_token = str(target.get("token") or "")
            target_token_id = hashlib.sha256(target_token.encode("utf-8")).hexdigest()[:16]

            client = self._create_client()
            resp = client.delete(
                f"/api/v1/tokens/{target_token_id}",
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 200)
            self.assertTrue(resp.json().get("ok"))

            # Verify deleted
            resp = client.get(
                "/api/v1/tokens",
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            tokens = resp.json()["result"]["tokens"]
            token_ids = [str(t.get("user_id") or "") for t in tokens]
            self.assertNotIn("to-delete", token_ids)
        finally:
            cleanup()

    def test_delete_token_by_full_token_is_backward_compatible(self) -> None:
        """Admin can still delete using full token for backward compatibility."""
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            admin = create_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")
            target = create_token("to-delete-compat", is_admin=False)
            target_token = str(target.get("token") or "")

            client = self._create_client()
            resp = client.delete(
                f"/api/v1/tokens/{target_token}",
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 200)
            self.assertTrue(resp.json().get("ok"))
        finally:
            cleanup()

    def test_delete_nonexistent_token_404(self) -> None:
        """Deleting a nonexistent token returns 404."""
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            admin = create_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")

            client = self._create_client()
            resp = client.delete(
                "/api/v1/tokens/usr_nonexistent",
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 404)
        finally:
            cleanup()

    def test_non_admin_cannot_create_tokens(self) -> None:
        """Non-admin user gets 403 when trying to create tokens."""
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            user = create_token("regular-user", is_admin=False)
            user_token = str(user.get("token") or "")

            client = self._create_client()
            resp = client.post(
                "/api/v1/tokens",
                json={"user_id": "new-user"},
                headers={"Authorization": f"Bearer {user_token}"},
            )
            self.assertEqual(resp.status_code, 403)
        finally:
            cleanup()

    def test_non_admin_cannot_list_tokens(self) -> None:
        """Non-admin user gets 403 when trying to list tokens."""
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            user = create_token("regular-user", is_admin=False)
            user_token = str(user.get("token") or "")

            client = self._create_client()
            resp = client.get(
                "/api/v1/tokens",
                headers={"Authorization": f"Bearer {user_token}"},
            )
            self.assertEqual(resp.status_code, 403)
        finally:
            cleanup()

    def test_create_token_empty_user_id_returns_400(self) -> None:
        """Creating a token with empty user_id returns 400."""
        from cccc.kernel.web_tokens import create_token

        _, cleanup = self._with_home()
        try:
            admin = create_token("admin-user", is_admin=True)
            admin_token = str(admin.get("token") or "")

            client = self._create_client()
            resp = client.post(
                "/api/v1/tokens",
                json={"user_id": ""},
                headers={"Authorization": f"Bearer {admin_token}"},
            )
            self.assertEqual(resp.status_code, 400)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
