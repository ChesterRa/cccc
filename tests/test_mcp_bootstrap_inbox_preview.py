import os
import unittest
from unittest.mock import patch


class TestMcpBootstrapInboxPreview(unittest.TestCase):
    def test_bootstrap_inbox_preview_is_trimmed_and_shape_stable(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_core, cccc_group_actor
        from cccc.ports.mcp.handlers import context as cccc_context

        long_text = "x" * 400

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_test", "CCCC_ACTOR_ID": "peer1"}, clear=False), patch.object(
            cccc_group_actor,
            "group_info",
            return_value={"group": {"group_id": "g_test", "title": "temp_task", "active_scope_key": "s1", "scopes": []}},
        ), patch.object(
            cccc_group_actor,
            "actor_list",
            return_value={"actors": [{"id": "peer1", "role": "peer", "runner": "pty"}]},
        ), patch.object(
            cccc_core,
            "project_info",
            return_value={"found": False, "path": None},
        ), patch.object(
            cccc_context,
            "context_get",
            return_value={"coordination": {"brief": {}, "tasks": [], "recent_decisions": [], "recent_handoffs": []}, "agent_states": []},
        ), patch.object(
            cccc_core,
            "inbox_peek",
            return_value={
                "messages": [
                    {"id": "ev1", "ts": "2026-03-07T00:00:00Z", "kind": "chat.message", "by": "user", "data": {"text": long_text, "message_mode": "mail"}},
                    {"id": "ev2", "ts": "2026-03-07T00:01:00Z", "kind": "chat.message", "by": "peer2", "data": {"text": "another Mail", "message_mode": "mail"}},
                    {"id": "ev3", "ts": "2026-03-07T00:02:00Z", "kind": "chat.message", "by": "user", "data": {"text": "extra", "message_mode": "mail"}},
                ]
            },
        ), patch.object(
            cccc_core,
            "_call_daemon_or_raise",
            return_value={"hits": []},
        ):
            out = mcp_server.bootstrap(group_id="g_test", actor_id="peer1", inbox_limit=2)

        preview = out["inbox_preview"]
        self.assertTrue(preview["truncated"] is True)
        self.assertEqual(len(preview["messages"]), 2)
        self.assertEqual(preview["messages"][0]["id"], "ev1")
        self.assertEqual(preview["messages"][1]["id"], "ev2")
        self.assertEqual(
            set(preview["messages"][0].keys()),
            {"id", "ts", "by", "kind", "message_mode", "text_preview"},
        )
        self.assertEqual(preview["messages"][0]["kind"], "chat.message")
        self.assertEqual(preview["messages"][0]["message_mode"], "mail")
        self.assertLessEqual(len(preview["messages"][0]["text_preview"]), 220)
        self.assertEqual(preview["messages"][1]["kind"], "chat.message")
        self.assertEqual(preview["messages"][1]["message_mode"], "mail")
        self.assertEqual(preview["messages"][1]["text_preview"], "another Mail")

    def test_bootstrap_inbox_preview_never_creates_a_reply_obligation(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_core, cccc_group_actor
        from cccc.ports.mcp.handlers import context as cccc_context

        with patch.dict(os.environ, {"CCCC_GROUP_ID": "g_test", "CCCC_ACTOR_ID": "peer1"}, clear=False), patch.object(
            cccc_group_actor,
            "group_info",
            return_value={"group": {"group_id": "g_test", "title": "temp_task", "active_scope_key": "s1", "scopes": []}},
        ), patch.object(
            cccc_group_actor,
            "actor_list",
            return_value={"actors": [{"id": "peer1", "role": "peer", "runner": "pty"}]},
        ), patch.object(
            cccc_core,
            "project_info",
            return_value={"found": False, "path": None},
        ), patch.object(
            cccc_context,
            "context_get",
            return_value={"coordination": {"brief": {}, "tasks": [], "recent_decisions": [], "recent_handoffs": []}, "agent_states": []},
        ), patch.object(
            cccc_core,
            "inbox_peek",
            return_value={
                "messages": [
                    {
                        "id": "ev1",
                        "ts": "2026-03-07T00:01:00Z",
                        "kind": "chat.message",
                        "by": "user",
                        "data": {"text": "Read this when convenient", "message_mode": "mail"},
                    }
                ]
            },
        ), patch.object(
            cccc_core,
            "_call_daemon_or_raise",
            return_value={"hits": []},
        ):
            out = mcp_server.bootstrap(group_id="g_test", actor_id="peer1", inbox_limit=2)

        message = out["inbox_preview"]["messages"][0]
        self.assertEqual(message["kind"], "chat.message")
        self.assertEqual(message["message_mode"], "mail")
        self.assertNotIn("reply_requested", message)
        self.assertEqual(message["text_preview"], "Read this when convenient")


if __name__ == "__main__":
    unittest.main()
