import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebCapabilityUseApi(unittest.TestCase):
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

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        return create_group(reg, title="cap-use", topic="").group_id

    def test_capability_use_route_proxies_to_handler(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with patch(
                "cccc.ports.web.routes.base.capability_use",
                return_value={"enabled": True, "tool_called": True, "capability_id": "mcp:test-server"},
            ) as mocked:
                with self._client() as client:
                    resp = client.post(
                        f"/api/v1/groups/{group_id}/capabilities/use",
                        json={
                            "actor_id": "user",
                            "capability_id": "mcp:test-server",
                            "tool_name": "echo",
                            "tool_arguments": {"message": "hello"},
                        },
                    )
            self.assertEqual(resp.status_code, 200, resp.text)
            body = resp.json()
            self.assertTrue(bool(body.get("ok")), body)
            result = body.get("result") or {}
            self.assertEqual(str(result.get("capability_id") or ""), "mcp:test-server")
            mocked.assert_called_once()
            kwargs = mocked.call_args.kwargs
            self.assertEqual(str(kwargs.get("group_id") or ""), group_id)
            self.assertEqual(str(kwargs.get("capability_id") or ""), "mcp:test-server")
            self.assertEqual(str(kwargs.get("tool_name") or ""), "echo")
            self.assertEqual(kwargs.get("tool_arguments"), {"message": "hello"})
        finally:
            cleanup()
