import os
import tempfile
import unittest
from io import BytesIO
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebActorAvatarApi(unittest.TestCase):
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

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        return create_group(reg, title="actor-avatar", topic="").group_id

    def _add_actor(self, group_id: str, actor_id: str = "peer-1") -> None:
        created = self._local_call_daemon(
            {
                "op": "actor_add",
                "args": {
                    "group_id": group_id,
                    "actor_id": actor_id,
                    "runtime": "codex",
                    "runner": "pty",
                    "command": [],
                    "env": {},
                    "by": "user",
                },
            }
        )
        self.assertTrue(bool(created.get("ok")), created)

    def test_upload_and_clear_actor_avatar(self) -> None:
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group

        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._add_actor(group_id)

            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()
                upload_resp = client.post(
                    f"/api/v1/groups/{group_id}/actors/peer-1/avatar",
                    data={"by": "user"},
                    files={
                        "file": (
                            "avatar.svg",
                            BytesIO(b'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" fill="#2563eb"/></svg>'),
                            "image/svg+xml",
                        )
                    },
                )
                self.assertEqual(upload_resp.status_code, 200)
                upload_body = upload_resp.json()
                self.assertTrue(bool(upload_body.get("ok")), upload_body)
                uploaded_actor = ((upload_body.get("result") or {}).get("actor")) or {}
                self.assertTrue(bool(uploaded_actor.get("has_custom_avatar")))
                self.assertIn(f"/api/v1/groups/{group_id}/actors/peer-1/avatar", str(uploaded_actor.get("avatar_url") or ""))

                group = load_group(group_id)
                self.assertIsNotNone(group)
                actor = find_actor(group, "peer-1") if group is not None else None
                self.assertIsInstance(actor, dict)
                rel_path = str((actor or {}).get("avatar_asset_path") or "")
                self.assertTrue(rel_path)
                abs_path = os.path.join(home, rel_path)
                self.assertTrue(os.path.isfile(abs_path))

                list_resp = client.get(f"/api/v1/groups/{group_id}/actors")
                self.assertEqual(list_resp.status_code, 200)
                listed_actor = (((list_resp.json().get("result") or {}).get("actors")) or [])[0]
                self.assertTrue(bool(listed_actor.get("has_custom_avatar")))
                avatar_url = str(listed_actor.get("avatar_url") or "")
                self.assertTrue(avatar_url)

                fetch_resp = client.get(avatar_url)
                self.assertEqual(fetch_resp.status_code, 200)
                self.assertIn("image/svg+xml", str(fetch_resp.headers.get("content-type") or ""))
                self.assertEqual(
                    str(fetch_resp.headers.get("cache-control") or ""),
                    "private, max-age=31536000, immutable",
                )

                clear_resp = client.delete(f"/api/v1/groups/{group_id}/actors/peer-1/avatar?by=user")
                self.assertEqual(clear_resp.status_code, 200)
                clear_body = clear_resp.json()
                self.assertTrue(bool(clear_body.get("ok")), clear_body)
                cleared_actor = ((clear_body.get("result") or {}).get("actor")) or {}
                self.assertFalse(bool(cleared_actor.get("has_custom_avatar")))
                self.assertFalse(os.path.exists(abs_path))
        finally:
            cleanup()

    def test_delete_actor_cleans_avatar_asset(self) -> None:
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group

        home, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            self._add_actor(group_id)

            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = self._client()
                upload_resp = client.post(
                    f"/api/v1/groups/{group_id}/actors/peer-1/avatar",
                    data={"by": "user"},
                    files={"file": ("avatar.png", BytesIO(b"\x89PNG\r\n\x1a\n"), "image/png")},
                )
                self.assertEqual(upload_resp.status_code, 200)

                group = load_group(group_id)
                self.assertIsNotNone(group)
                actor = find_actor(group, "peer-1") if group is not None else None
                rel_path = str((actor or {}).get("avatar_asset_path") or "")
                abs_path = os.path.join(home, rel_path)
                self.assertTrue(os.path.isfile(abs_path))

                delete_resp = client.delete(f"/api/v1/groups/{group_id}/actors/peer-1?by=user")
                self.assertEqual(delete_resp.status_code, 200)
                self.assertFalse(os.path.exists(abs_path))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
