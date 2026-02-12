import unittest
from unittest.mock import patch


class TestMcpGroupSetStateStopped(unittest.TestCase):
    def test_group_set_state_stopped_maps_to_group_stop(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"group_id": "g_test"}}

        with patch.object(mcp_server, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_group_set_state",
                {
                    "group_id": "g_test",
                    "actor_id": "foreman",
                    "state": "stopped",
                },
            )

        self.assertEqual(out.get("group_id"), "g_test")
        req = captured.get("req") or {}
        self.assertEqual(req.get("op"), "group_stop")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("group_id"), "g_test")
        self.assertEqual(args.get("by"), "foreman")


if __name__ == "__main__":
    unittest.main()
