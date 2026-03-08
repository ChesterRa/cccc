import os
import tempfile
import unittest

from fastapi.testclient import TestClient
from unittest.mock import patch


class TestWebActorExtraPrompt(unittest.TestCase):
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

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title="web-actor-extra-prompt", topic="")
        return group.group_id

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

    def test_web_actor_create_and_update_accept_extra_prompt(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            gid = self._create_group()
            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                client = TestClient(create_app())

                create_resp = client.post(
                    f"/api/v1/groups/{gid}/actors",
                    json={
                        "actor_id": "peer1",
                        "title": "Peer 1",
                        "runtime": "codex",
                        "runner": "pty",
                        "command": "",
                        "env": {},
                        "extra_prompt": "Handle implementation only; do not merge unrelated refactors.",
                        "capability_autoload": [],
                        "by": "user",
                    },
                )
                self.assertEqual(create_resp.status_code, 200)
                create_body = create_resp.json()
                self.assertTrue(create_body.get("ok"))
                created_actor = ((create_body.get("result") or {}).get("actor") or {})
                self.assertEqual(
                    str(created_actor.get("extra_prompt") or ""),
                    "Handle implementation only; do not merge unrelated refactors.",
                )

                update_resp = client.post(
                    f"/api/v1/groups/{gid}/actors/peer1",
                    json={
                        "by": "user",
                        "extra_prompt": "Focus on implementation details and flag risky migrations.",
                    },
                )
                self.assertEqual(update_resp.status_code, 200)
                update_body = update_resp.json()
                self.assertTrue(update_body.get("ok"))
                updated_actor = ((update_body.get("result") or {}).get("actor") or {})
                self.assertEqual(
                    str(updated_actor.get("extra_prompt") or ""),
                    "Focus on implementation details and flag risky migrations.",
                )

                list_resp = client.get(f"/api/v1/groups/{gid}/actors")
                self.assertEqual(list_resp.status_code, 200)
                list_body = list_resp.json()
                self.assertTrue(list_body.get("ok"))
                actors = ((list_body.get("result") or {}).get("actors") or [])
                actor = next((item for item in actors if str(item.get("id") or "") == "peer1"), None)
                self.assertIsNotNone(actor)
                self.assertEqual(
                    str((actor or {}).get("extra_prompt") or ""),
                    "Focus on implementation details and flag risky migrations.",
                )
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
