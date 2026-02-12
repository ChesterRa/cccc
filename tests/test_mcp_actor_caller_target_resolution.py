import os
import unittest
from unittest.mock import patch


class TestMcpActorCallerTargetResolution(unittest.TestCase):
    def test_actor_add_uses_env_actor_as_caller(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"ok": True}}

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_test", "CCCC_ACTOR_ID": "foreman"}, clear=False):
            with patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
                out = mcp_server.handle_tool_call(
                    "cccc_actor_add",
                    {
                        "actor_id": "peer_new",
                        "runtime": "codex",
                        "runner": "pty",
                    },
                )

        self.assertEqual(out.get("ok"), True)
        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        self.assertEqual(req.get("op"), "actor_add")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), "g_test")
        self.assertEqual(args.get("by"), "foreman")
        self.assertEqual(args.get("actor_id"), "peer_new")

    def test_actor_start_uses_env_actor_as_caller(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"ok": True}}

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_test", "CCCC_ACTOR_ID": "foreman"}, clear=False):
            with patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
                out = mcp_server.handle_tool_call(
                    "cccc_actor_start",
                    {
                        "actor_id": "peer_new",
                    },
                )

        self.assertEqual(out.get("ok"), True)
        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        self.assertEqual(req.get("op"), "actor_start")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), "g_test")
        self.assertEqual(args.get("by"), "foreman")
        self.assertEqual(args.get("actor_id"), "peer_new")

    def test_actor_add_requires_caller_identity(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import MCPError

        with patch.dict(os.environ, {}, clear=False):
            with self.assertRaises(MCPError) as raised:
                mcp_server.handle_tool_call(
                    "cccc_actor_add",
                    {
                        "group_id": "g_test",
                        "actor_id": "peer_new",
                        "runtime": "codex",
                        "runner": "pty",
                    },
                )
        self.assertEqual(getattr(raised.exception, "code", ""), "missing_actor_id")


if __name__ == "__main__":
    unittest.main()
