import base64
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from fastapi.testclient import TestClient


class TestNomcpSessions(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        old_public_url = os.environ.get("CCCC_WEB_PUBLIC_URL")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td
        os.environ["CCCC_WEB_PUBLIC_URL"] = "https://cccc.example.test/ui/"

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            if old_public_url is None:
                os.environ.pop("CCCC_WEB_PUBLIC_URL", None)
            else:
                os.environ["CCCC_WEB_PUBLIC_URL"] = old_public_url

        return Path(td), cleanup

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def _admin_token(self) -> str:
        from cccc.kernel.access_tokens import create_access_token

        return str(create_access_token("admin", is_admin=True).get("token") or "")

    def _repo(self, base: Path) -> Path:
        root = base / "repo"
        (root / "docs").mkdir(parents=True)
        (root / "src" / "cccc").mkdir(parents=True)
        (root / "tests").mkdir(parents=True)
        (root / "web" / "src").mkdir(parents=True)
        (root / "README.md").write_text("hello README\nliteral a.b\n", encoding="utf-8")
        (root / "PROJECT.md").write_text("project brief\n", encoding="utf-8")
        (root / "docs" / "note.md").write_text("doc note\nneedle here\n", encoding="utf-8")
        (root / "docs" / "long.md").write_text("\n".join(f"line {idx}" for idx in range(1, 701)) + "\n", encoding="utf-8")
        (root / "src" / "cccc" / "demo.py").write_text("print('demo')\n", encoding="utf-8")
        (root / "tests" / "test_demo.py").write_text("def test_demo():\n    assert True\n", encoding="utf-8")
        (root / "web" / "src" / "demo.ts").write_text("export const demo = 1;\n", encoding="utf-8")
        (root / ".env").write_text("SECRET=1\n", encoding="utf-8")
        (root / "docs" / "binary.md").write_bytes(b"abc\x00def")
        subprocess.run(["git", "init"], cwd=root, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        subprocess.run(["git", "config", "user.email", "test@example.com"], cwd=root, check=True)
        subprocess.run(["git", "config", "user.name", "Test"], cwd=root, check=True)
        subprocess.run(["git", "add", "."], cwd=root, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        subprocess.run(["git", "commit", "-m", "init"], cwd=root, check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        (root / "docs" / "note.md").write_text("doc note changed\nneedle here\n", encoding="utf-8")
        return root

    def _group(self, root: Path):
        from cccc.kernel.group import attach_scope_to_group, create_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.scope import detect_scope

        reg = load_registry()
        group = create_group(reg, title="nomcp-test", topic="")
        return attach_scope_to_group(reg, group, detect_scope(root), set_active=True)

    def _create_session(self, client: TestClient, admin: str, group_id: str, **overrides):
        payload = {
            "group_id": group_id,
            "title": "No-MCP test",
            "brief": "Read-only advisory test.",
            "recipient": "user",
        }
        payload.update(overrides)
        resp = client.post(
            "/api/v1/nomcp/sessions",
            headers={"Authorization": f"Bearer {admin}"},
            json=payload,
        )
        self.assertEqual(resp.status_code, 200, resp.text)
        result = resp.json().get("result") or {}
        session = result.get("session") or {}
        secret = str(result.get("secret") or "")
        self.assertTrue(str(session.get("sid") or "").startswith("nomcp_"))
        self.assertTrue(secret.startswith("nomcps_"))
        self.assertIn("https://cccc.example.test/nomcp/s/", str(session.get("session_url_with_token") or ""))
        self.assertIn("?token=", str(session.get("session_url_with_token") or ""))
        return session, secret

    def _ledger_events(self, group) -> list[dict]:
        events: list[dict] = []
        for line in Path(group.ledger_path).read_text(encoding="utf-8").splitlines():
            if line.strip():
                events.append(json.loads(line))
        return events

    def test_nomcp_session_serves_read_only_resources_with_token(self) -> None:
        home, cleanup = self._with_home()
        try:
            root = self._repo(home)
            group = self._group(root)
            admin = self._admin_token()
            client = self._client()
            session, secret = self._create_session(client, admin, group.group_id)
            sid = session["sid"]

            list_resp = client.get("/api/v1/nomcp/sessions", headers={"Authorization": f"Bearer {admin}"})
            self.assertEqual(list_resp.status_code, 200)
            listed = ((list_resp.json().get("result") or {}).get("sessions") or [])[0]
            self.assertEqual(listed.get("sid"), sid)
            self.assertFalse(listed.get("secret_available"))
            self.assertNotIn("session_url_with_token", listed)
            self.assertGreaterEqual(int(listed.get("resource_count") or 0), 1)
            self.assertGreaterEqual(int(listed.get("changed_file_count") or 0), 1)

            home_resp = client.get(f"/nomcp/s/{sid}", params={"token": secret})
            self.assertEqual(home_resp.status_code, 200)
            self.assertIn("Recommended Reading Order", home_resp.text)
            self.assertIn("Status Summary", home_resp.text)
            self.assertIn("Diff Summary", home_resp.text)

            resources = client.get(f"/nomcp/s/{sid}/resources", params={"token": secret, "format": "md"})
            self.assertEqual(resources.status_code, 200)
            self.assertIn("docs/note.md", resources.text)
            self.assertNotIn(".env", resources.text)

            read_resp = client.get(
                f"/nomcp/s/{sid}/read",
                params={"token": secret, "path": "docs/note.md", "format": "json"},
            )
            self.assertEqual(read_resp.status_code, 200)
            self.assertIn("needle here", str((read_resp.json().get("result") or {}).get("content") or ""))

            status_resp = client.get(f"/nomcp/s/{sid}/status", params={"token": secret, "format": "json"})
            self.assertEqual(status_resp.status_code, 200)
            self.assertIn("docs/note.md", (status_resp.json().get("result") or {}).get("status", {}).get("changed_files") or [])

            diff_resp = client.get(f"/nomcp/s/{sid}/diff", params={"token": secret, "path": "docs/note.md", "format": "json"})
            self.assertEqual(diff_resp.status_code, 200)
            self.assertIn("doc note changed", str((diff_resp.json().get("result") or {}).get("diff") or ""))
        finally:
            cleanup()

    def test_nomcp_session_rejects_unsafe_paths_and_respects_session_narrowing(self) -> None:
        home, cleanup = self._with_home()
        try:
            root = self._repo(home)
            outside = home / "outside.md"
            outside.write_text("outside\n", encoding="utf-8")
            if hasattr(os, "symlink"):
                try:
                    (root / "docs" / "escape.md").symlink_to(outside)
                except OSError:
                    pass
            group = self._group(root)
            admin = self._admin_token()
            client = self._client()
            session, secret = self._create_session(client, admin, group.group_id, allowed_paths=["docs"])
            sid = session["sid"]

            src_resp = client.get(f"/nomcp/s/{sid}/read", params={"token": secret, "path": "src/cccc/demo.py", "format": "json"})
            self.assertEqual(src_resp.status_code, 403)
            env_resp = client.get(f"/nomcp/s/{sid}/read", params={"token": secret, "path": ".env", "format": "json"})
            self.assertEqual(env_resp.status_code, 403)
            traversal_resp = client.get(f"/nomcp/s/{sid}/read", params={"token": secret, "path": "../README.md", "format": "json"})
            self.assertEqual(traversal_resp.status_code, 400)
            binary_resp = client.get(f"/nomcp/s/{sid}/read", params={"token": secret, "path": "docs/binary.md", "format": "json"})
            self.assertEqual(binary_resp.status_code, 415)
            if (root / "docs" / "escape.md").exists():
                escape_resp = client.get(f"/nomcp/s/{sid}/read", params={"token": secret, "path": "docs/escape.md", "format": "json"})
                self.assertEqual(escape_resp.status_code, 403)

            long_resp = client.get(
                f"/nomcp/s/{sid}/read",
                params={"token": secret, "path": "docs/long.md", "start": 1, "end": 700, "format": "json"},
            )
            self.assertEqual(long_resp.status_code, 200)
            result = long_resp.json().get("result") or {}
            self.assertEqual(result.get("end"), 500)
            self.assertTrue(result.get("truncated"))
        finally:
            cleanup()

    def test_nomcp_search_is_literal_and_excludes_denied_files(self) -> None:
        home, cleanup = self._with_home()
        try:
            root = self._repo(home)
            (root / "docs" / "regex.md").write_text("axb should not match literal\nliteral a.b should match\n", encoding="utf-8")
            (root / ".env.extra").write_text("literal a.b secret\n", encoding="utf-8")
            group = self._group(root)
            admin = self._admin_token()
            client = self._client()
            session, secret = self._create_session(client, admin, group.group_id)
            resp = client.get(f"/nomcp/s/{session['sid']}/search", params={"token": secret, "q": "a.b", "format": "json"})
            self.assertEqual(resp.status_code, 200)
            matches = (resp.json().get("result") or {}).get("matches") or []
            joined = "\n".join(str(item) for item in matches)
            self.assertIn("literal a.b", joined)
            self.assertNotIn("axb should not match", joined)
            self.assertNotIn(".env", joined)
        finally:
            cleanup()

    def test_nomcp_advisory_send_is_idempotent_and_scoped(self) -> None:
        home, cleanup = self._with_home()
        try:
            root = self._repo(home)
            group = self._group(root)
            admin = self._admin_token()
            client = self._client()
            session, secret = self._create_session(client, admin, group.group_id, reply_to_event_id="evt_parent")
            sid = session["sid"]

            resp = client.post(f"/nomcp/s/{sid}/send?token={secret}", data={"msg_id": "m1", "text": "advisory body"})
            self.assertEqual(resp.status_code, 200)
            self.assertIn("accepted", resp.text)
            repeat = client.post(f"/nomcp/s/{sid}/send?token={secret}", data={"msg_id": "m1", "text": "advisory body"})
            self.assertEqual(repeat.status_code, 200)
            self.assertIn("duplicate_ignored", repeat.text)

            text = "GET advisory"
            encoded = base64.urlsafe_b64encode(text.encode("utf-8")).decode("ascii").rstrip("=")
            get_resp = client.get(f"/nomcp/s/{sid}/send", params={"token": secret, "msg_id": "m2", "text_b64url": encoded})
            self.assertEqual(get_resp.status_code, 200)
            too_large = base64.urlsafe_b64encode(("x" * (13 * 1024)).encode("utf-8")).decode("ascii").rstrip("=")
            large_resp = client.get(f"/nomcp/s/{sid}/send", params={"token": secret, "msg_id": "m3", "text_b64url": too_large})
            self.assertEqual(large_resp.status_code, 413)

            messages = [event for event in self._ledger_events(group) if event.get("kind") == "chat.message"]
            self.assertEqual(len(messages), 2)
            first = messages[0]
            self.assertEqual(first.get("by"), "nomcp-advisory")
            data = first.get("data") or {}
            self.assertEqual(data.get("reply_to"), "evt_parent")
            self.assertEqual(data.get("source_platform"), "nomcp")
            self.assertEqual(data.get("source_user_id"), sid)
            self.assertEqual(data.get("client_id"), f"nomcp:{sid}:m1")
            refs = data.get("refs") or []
            self.assertEqual(refs[0].get("source"), "nomcp")
            self.assertTrue(refs[0].get("cannot_execute_local_tools"))
        finally:
            cleanup()

    def test_nomcp_revoked_expired_and_invalid_tokens_are_rejected(self) -> None:
        home, cleanup = self._with_home()
        try:
            root = self._repo(home)
            group = self._group(root)
            admin = self._admin_token()
            client = self._client()
            session, secret = self._create_session(client, admin, group.group_id)
            sid = session["sid"]

            bad = client.get(f"/nomcp/s/{sid}/read", params={"token": "wrong", "path": "README.md", "format": "json"})
            self.assertEqual(bad.status_code, 403)
            revoke = client.delete(f"/api/v1/nomcp/sessions/{sid}", headers={"Authorization": f"Bearer {admin}"})
            self.assertEqual(revoke.status_code, 200)
            revoked = client.get(f"/nomcp/s/{sid}/read", params={"token": secret, "path": "README.md", "format": "json"})
            self.assertEqual(revoked.status_code, 410)
            revoked_send = client.post(f"/nomcp/s/{sid}/send?token={secret}", data={"msg_id": "revoked", "text": "body"})
            self.assertEqual(revoked_send.status_code, 410)

            session2, secret2 = self._create_session(client, admin, group.group_id)
            path = home / "state" / "nomcp_sessions" / f"{session2['sid']}.json"
            raw = json.loads(path.read_text(encoding="utf-8"))
            raw["expires_at"] = "2000-01-01T00:00:00Z"
            path.write_text(json.dumps(raw, ensure_ascii=False), encoding="utf-8")
            expired = client.get(f"/nomcp/s/{session2['sid']}/read", params={"token": secret2, "path": "README.md", "format": "json"})
            self.assertEqual(expired.status_code, 410)
            expired_send = client.post(f"/nomcp/s/{session2['sid']}/send?token={secret2}", data={"msg_id": "expired", "text": "body"})
            self.assertEqual(expired_send.status_code, 410)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
