import unittest
from unittest.mock import patch


class TestMcpMessageSendReplyRequired(unittest.TestCase):
    def test_message_send_coerces_reply_required_string(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp import common as mcp_common

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"event_id": "ev_test"}}

        with patch.object(mcp_common, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_message_send",
                {
                    "group_id": "g_test",
                    "actor_id": "peer1",
                    "text": "hello",
                    "to": ["user"],
                    "reply_required": "true",
                },
            )

        self.assertEqual(out.get("event_id"), "ev_test")
        req = captured.get("req") or {}
        self.assertEqual(req.get("op"), "send")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertTrue(args.get("reply_required") is True)


if __name__ == "__main__":
    unittest.main()
