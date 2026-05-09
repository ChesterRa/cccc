from __future__ import annotations

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

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        return create_group(reg, title="web-capability-use-test", topic="").group_id

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def test_capability_use_route_allows_skill_activation_without_tool_name(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with patch(
                "cccc.ports.web.routes.base.mcp_capability_use",
                return_value={"capability_id": "skill:agent_self_proposed:triage", "tool_called": False},
            ) as use_mock:
                resp = self._client().post(
                    f"/api/v1/groups/{group_id}/capabilities/use",
                    json={"capability_id": "skill:agent_self_proposed:triage"},
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(bool(body.get("ok")))
            self.assertEqual(str(((body.get("result") or {}).get("capability_id")) or ""), "skill:agent_self_proposed:triage")
            kwargs = use_mock.call_args.kwargs if use_mock.call_args else {}
            self.assertEqual(str(kwargs.get("group_id") or ""), group_id)
            self.assertEqual(str(kwargs.get("by") or ""), "user")
            self.assertEqual(str(kwargs.get("actor_id") or ""), "user")
            self.assertEqual(str(kwargs.get("capability_id") or ""), "skill:agent_self_proposed:triage")
            self.assertEqual(str(kwargs.get("tool_name") or ""), "")
            self.assertEqual(kwargs.get("tool_arguments"), {})
            self.assertEqual(str(kwargs.get("scope") or ""), "session")
        finally:
            cleanup()

    def test_capability_install_route_uses_install_lifecycle(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with patch(
                "cccc.ports.web.routes.base.mcp_capability_install",
                return_value={"state": "ready", "use_ready_capability_ids": ["skill:github:demo:triage"]},
            ) as install_mock:
                resp = self._client().post(
                    f"/api/v1/groups/{group_id}/capabilities/install",
                    json={
                        "actor_id": "peer-1",
                        "target": "demo/triage",
                        "scope": "actor",
                        "reason": "web install",
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(bool(body.get("ok")))
            self.assertEqual(str(((body.get("result") or {}).get("state")) or ""), "ready")
            kwargs = install_mock.call_args.kwargs if install_mock.call_args else {}
            self.assertEqual(str(kwargs.get("group_id") or ""), group_id)
            self.assertEqual(str(kwargs.get("by") or ""), "user")
            self.assertEqual(str(kwargs.get("actor_id") or ""), "peer-1")
            self.assertEqual(str(kwargs.get("target") or ""), "demo/triage")
            self.assertEqual(str(kwargs.get("scope") or ""), "actor")
        finally:
            cleanup()

    def test_capability_use_route_passes_tool_call_payload(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with patch(
                "cccc.ports.web.routes.base.mcp_capability_use",
                return_value={"capability_id": "mcp:test-server", "tool_called": True, "tool_name": "echo"},
            ) as use_mock:
                resp = self._client().post(
                    f"/api/v1/groups/{group_id}/capabilities/use",
                    json={
                        "actor_id": "peer-1",
                        "capability_id": "mcp:test-server",
                        "tool_name": "echo",
                        "tool_arguments": {"message": "hello"},
                        "scope": "actor",
                    },
                )

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(bool(body.get("ok")))
            self.assertTrue(bool((body.get("result") or {}).get("tool_called")))
            kwargs = use_mock.call_args.kwargs if use_mock.call_args else {}
            self.assertEqual(str(kwargs.get("actor_id") or ""), "peer-1")
            self.assertEqual(str(kwargs.get("capability_id") or ""), "mcp:test-server")
            self.assertEqual(str(kwargs.get("tool_name") or ""), "echo")
            self.assertEqual(kwargs.get("tool_arguments"), {"message": "hello"})
            self.assertEqual(str(kwargs.get("scope") or ""), "actor")
        finally:
            cleanup()

    def test_capability_use_route_wraps_mcp_runtime_context(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            seen: dict[str, str] = {}

            def fake_use(**kwargs):
                from cccc.ports.mcp.common import _runtime_context

                ctx = _runtime_context()
                seen["home"] = str(ctx.home)
                seen["group_id"] = str(ctx.group_id)
                seen["actor_id"] = str(ctx.actor_id)
                return {"capability_id": kwargs.get("capability_id"), "tool_called": False}

            with patch("cccc.ports.web.routes.base.mcp_capability_use", side_effect=fake_use):
                resp = self._client().post(
                    f"/api/v1/groups/{group_id}/capabilities/use",
                    json={"actor_id": "peer-ctx", "capability_id": "skill:agent_self_proposed:triage"},
                )

            self.assertEqual(resp.status_code, 200)
            self.assertTrue(seen.get("home", "").endswith(os.environ["CCCC_HOME"]))
            self.assertEqual(seen.get("group_id"), group_id)
            self.assertEqual(seen.get("actor_id"), "peer-ctx")
        finally:
            cleanup()

    def test_capability_use_route_rejects_non_object_tool_arguments(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with patch("cccc.ports.web.routes.base.mcp_capability_use") as use_mock:
                resp = self._client().post(
                    f"/api/v1/groups/{group_id}/capabilities/use",
                    json={
                        "capability_id": "mcp:test-server",
                        "tool_name": "echo",
                        "tool_arguments": ["not", "object"],
                    },
                )

            self.assertEqual(resp.status_code, 400)
            body = resp.json()
            self.assertFalse(bool(body.get("ok")))
            self.assertEqual(str(((body.get("error") or {}).get("code")) or ""), "invalid_tool_arguments")
            use_mock.assert_not_called()
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
