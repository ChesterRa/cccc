import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebPrincipalGuards(unittest.TestCase):
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

    def _create_group(self, title: str) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title=title, topic="")
        return group.group_id

    def _create_client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def test_invalid_access_token_gets_401_on_group_route(self) -> None:
        _, cleanup = self._with_home()
        try:
            gid = self._create_group("g1")
            client = self._create_client()
            resp = client.get(f"/api/v1/groups/{gid}", headers={"Authorization": "Bearer invalid-token"})
            self.assertEqual(resp.status_code, 401)
            self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "unauthorized")
        finally:
            cleanup()

    def test_anonymous_forbidden_when_access_tokens_exist(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("g1")
            create_access_token("user-a", allowed_groups=[gid], is_admin=False)
            client = self._create_client()
            resp = client.get(f"/api/v1/groups/{gid}")
            self.assertEqual(resp.status_code, 401)
            self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "unauthorized")
        finally:
            cleanup()

    def test_groups_list_filters_by_allowed_groups(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            gid1 = self._create_group("g1")
            self._create_group("g2")
            token = str(create_access_token("user-a", allowed_groups=[gid1], is_admin=False).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._create_client()
                resp = client.get("/api/v1/groups", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            groups = ((resp.json().get("result") or {}).get("groups") or [])
            ids = sorted(str(item.get("group_id") or item.get("id") or "") for item in groups if isinstance(item, dict))
            self.assertEqual(ids, [gid1])
        finally:
            cleanup()

    def test_non_admin_observability_and_fs_recent_are_denied(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            token = str(create_access_token("user-a", allowed_groups=[], is_admin=False).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._create_client()
                headers = {"Authorization": f"Bearer {token}"}

                resp = client.get("/api/v1/observability", headers=headers)
                self.assertEqual(resp.status_code, 403)
                self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "permission_denied")

                resp = client.get("/api/v1/fs/recent", headers=headers)
                self.assertEqual(resp.status_code, 403)
                self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "permission_denied")
        finally:
            cleanup()

    def test_admin_can_access_fs_recent(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            token = str(create_access_token("admin-a", allowed_groups=[], is_admin=True).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._create_client()
                resp = client.get("/api/v1/fs/recent", headers={"Authorization": f"Bearer {token}"})
                self.assertEqual(resp.status_code, 200)
                self.assertTrue(bool(resp.json().get("ok")))
                self.assertIsInstance((resp.json().get("result") or {}).get("suggestions"), list)
        finally:
            cleanup()

    def test_ping_omits_home_by_default_and_includes_it_on_demand(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._create_client()

                default_resp = client.get("/api/v1/ping")
                self.assertEqual(default_resp.status_code, 200)
                default_result = default_resp.json().get("result") or {}
                self.assertNotIn("home", default_result)

                detailed_resp = client.get("/api/v1/ping?include_home=1")
                self.assertEqual(detailed_resp.status_code, 200)
                detailed_result = detailed_resp.json().get("result") or {}
                self.assertTrue(bool(str(detailed_result.get("home") or "").strip()))
        finally:
            cleanup()

    def test_runtimes_response_is_trimmed_to_ui_fields(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.runtime import RuntimeInfo

        _, cleanup = self._with_home()
        try:
            token = str(create_access_token("user-a", allowed_groups=[], is_admin=False).get("token") or "")
            runtime = RuntimeInfo(
                name="codex",
                display_name="Codex CLI",
                command="codex",
                available=True,
                path="/usr/bin/codex",
                capabilities="MCP; MCP setup: auto",
                mcp_add_command=["codex", "mcp", "add"],
            )
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon), patch(
                "cccc.kernel.runtime.detect_all_runtimes",
                return_value=[runtime],
            ), patch(
                "cccc.kernel.runtime.get_runtime_command_with_flags",
                return_value=["codex", "--mcp-config", "cccc"],
            ):
                client = self._create_client()
                resp = client.get("/api/v1/runtimes", headers={"Authorization": f"Bearer {token}"})
                self.assertEqual(resp.status_code, 200)
                runtimes = ((resp.json().get("result") or {}).get("runtimes") or [])
                self.assertEqual(len(runtimes), 1)
                row = runtimes[0]
                self.assertEqual(row.get("name"), "codex")
                self.assertEqual(row.get("display_name"), "Codex CLI")
                self.assertEqual(row.get("recommended_command"), "codex --mcp-config cccc")
                self.assertEqual(row.get("available"), True)
                self.assertNotIn("command", row)
                self.assertNotIn("path", row)
                self.assertNotIn("capabilities", row)
        finally:
            cleanup()

    def test_empty_scoped_token_sees_no_groups(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            self._create_group("g1")
            self._create_group("g2")
            token = str(create_access_token("user-a", allowed_groups=[], is_admin=False).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._create_client()
                resp = client.get("/api/v1/groups", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            groups = ((resp.json().get("result") or {}).get("groups") or [])
            self.assertEqual(groups, [])
        finally:
            cleanup()

    def test_empty_scoped_token_cannot_access_group_route(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("g1")
            token = str(create_access_token("user-a", allowed_groups=[], is_admin=False).get("token") or "")
            client = self._create_client()
            resp = client.get(f"/api/v1/groups/{gid}", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 403)
            self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "permission_denied")
        finally:
            cleanup()

    def test_group_delete_requires_admin_even_with_group_scope(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("g1")
            token = str(create_access_token("user-a", allowed_groups=[gid], is_admin=False).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._create_client()
                resp = client.delete(f"/api/v1/groups/{gid}?confirm={gid}", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 403)
            self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "permission_denied")
        finally:
            cleanup()

    def test_group_reset_requires_admin_even_with_group_scope(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("g1")
            token = str(create_access_token("user-a", allowed_groups=[gid], is_admin=False).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._create_client()
                resp = client.post(f"/api/v1/groups/{gid}/reset?confirm={gid}", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 403)
            self.assertEqual(str((resp.json().get("error") or {}).get("code") or ""), "permission_denied")
        finally:
            cleanup()

    def test_admin_can_delete_group(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("g1")
            token = str(create_access_token("admin-user", is_admin=True).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._create_client()
                resp = client.delete(f"/api/v1/groups/{gid}?confirm={gid}", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            self.assertTrue(bool(resp.json().get("ok")))
        finally:
            cleanup()

    def test_admin_can_reset_group(self) -> None:
        from cccc.kernel.access_tokens import create_access_token
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("g1")
            token = str(create_access_token("admin-user", is_admin=True).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._create_client()
                resp = client.post(f"/api/v1/groups/{gid}/reset?confirm={gid}", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            payload = resp.json()
            self.assertTrue(bool(payload.get("ok")))
            new_gid = str(((payload.get("result") or {}) if isinstance(payload.get("result"), dict) else {}).get("new_group_id") or "")
            self.assertTrue(new_gid)
            self.assertNotEqual(new_gid, gid)
            self.assertIsNone(load_group(gid))
            self.assertIsNotNone(load_group(new_gid))
        finally:
            cleanup()

    def test_websocket_terminal_checks_group_access(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            gid1 = self._create_group("g1")
            gid2 = self._create_group("g2")
            token = str(create_access_token("user-a", allowed_groups=[gid1], is_admin=False).get("token") or "")
            client = self._create_client()
            with client.websocket_connect(f"/api/v1/groups/{gid2}/actors/demo/term?token={token}") as ws:
                payload = ws.receive_json()
                self.assertFalse(payload.get("ok"))
                self.assertEqual(str((payload.get("error") or {}).get("code") or ""), "permission_denied")
        finally:
            cleanup()

    def test_websocket_auth_accepts_valid_access_token(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("g1")
            token = str(create_access_token("user-a", allowed_groups=[gid], is_admin=True).get("token") or "")
            client = self._create_client()
            with client.websocket_connect(f"/api/v1/groups/{gid}/actors/demo/term?token={token}") as ws:
                payload = ws.receive_json()
                self.assertFalse(payload.get("ok"))
                self.assertNotEqual(str((payload.get("error") or {}).get("code") or ""), "auth_required")
        finally:
            cleanup()


    # -- /api/v1/health public path + token resolution regression (T256) --

    def test_health_anonymous_returns_200_without_extended_fields(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch("cccc.ports.web.app.call_daemon", return_value={"ok": True}):
                client = self._create_client()
                resp = client.get("/api/v1/health")
            self.assertEqual(resp.status_code, 200)
            result = (resp.json().get("result") or {})
            self.assertNotIn("version", result)
            self.assertNotIn("home", result)
        finally:
            cleanup()

    def test_health_valid_token_returns_200_with_extended_fields(self) -> None:
        from cccc.kernel.access_tokens import create_access_token

        _, cleanup = self._with_home()
        try:
            token = str(create_access_token("user-a", allowed_groups=[], is_admin=False).get("token") or "")
            with patch("cccc.ports.web.app.call_daemon", return_value={"ok": True}):
                client = self._create_client()
                resp = client.get("/api/v1/health", headers={"Authorization": f"Bearer {token}"})
            self.assertEqual(resp.status_code, 200)
            result = (resp.json().get("result") or {})
            self.assertIn("version", result)
            self.assertIn("home", result)
        finally:
            cleanup()

    def test_health_invalid_token_still_returns_200(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch("cccc.ports.web.app.call_daemon", return_value={"ok": True}):
                client = self._create_client()
                resp = client.get("/api/v1/health", headers={"Authorization": "Bearer bad-token"})
            self.assertEqual(resp.status_code, 200)
            result = (resp.json().get("result") or {})
            self.assertNotIn("version", result)
            self.assertNotIn("home", result)
        finally:
            cleanup()

    def test_ready_anonymous_returns_200_without_daemon_ping(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch("cccc.ports.web.app.call_daemon", side_effect=AssertionError("ready must not ping daemon")):
                client = self._create_client()
                resp = client.get("/api/v1/ready")
            self.assertEqual(resp.status_code, 200)
            self.assertEqual(resp.json(), {"ok": True, "result": {"web": "ready"}})
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
