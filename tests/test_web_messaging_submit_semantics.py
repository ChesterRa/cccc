import os
import tempfile
import unittest
from io import BytesIO
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebMessagingSubmitSemantics(unittest.TestCase):
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

    def _with_env(self, key: str, value: str):
        old_value = os.environ.get(key)
        os.environ[key] = value

        def cleanup() -> None:
            if old_value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = old_value

        return cleanup

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def test_reply_surfaces_daemon_error_even_if_async_env_requested(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup_home = self._with_home()
        cleanup_mode = self._with_env("CCCC_WEB_MESSAGE_SUBMIT_MODE", "async")
        try:
            reg = load_registry()
            group = create_group(reg, title="web-message-semantics", topic="")

            def fake_call_daemon(req: dict) -> dict:
                return {
                    "ok": False,
                    "error": {
                        "code": "event_not_found",
                        "message": "event not found: evt_missing",
                        "details": {},
                    },
                }

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                client = self._client()
                resp = client.post(
                    f"/api/v1/groups/{group.group_id}/reply",
                    json={
                        "text": "bad reply",
                        "by": "user",
                        "to": ["user"],
                        "reply_to": "evt_missing",
                        "client_id": "reply-bad-1",
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertFalse(bool(body.get("ok")))
            self.assertEqual(str(((body.get("error") or {}).get("code")) or ""), "event_not_found")
        finally:
            cleanup_mode()
            cleanup_home()

    def test_send_upload_body_mentions_are_not_promoted_to_recipients(self) -> None:
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        _, cleanup_home = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="web-upload-mentions", topic="")
            add_actor(
                group,
                actor_id="peer1",
                title="Peer 1",
                runtime="codex",
                runner="headless",
            )
            captured: dict[str, dict] = {}

            def fake_call_daemon(req: dict) -> dict:
                captured["req"] = req
                return {"ok": True, "result": {"event": {"id": "evt_upload_1", "data": (req.get("args") or {})}}}

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                client = self._client()
                resp = client.post(
                    f"/api/v1/groups/{group.group_id}/send_upload",
                    data={
                        "by": "user",
                        "text": "please ask @peer1 for context",
                        "to_json": "[]",
                        "path": "",
                    },
                    files={"files": ("note.txt", BytesIO(b"hello"), "text/plain")},
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(bool(body.get("ok")))
            args = (captured.get("req") or {}).get("args") or {}
            self.assertEqual(args.get("to"), ["@foreman"])
        finally:
            cleanup_home()


if __name__ == "__main__":
    unittest.main()
