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
            "inbox_list",
            return_value={
                "messages": [
                    {"id": "ev1", "ts": "2026-03-07T00:00:00Z", "kind": "chat.message", "by": "user", "data": {"text": long_text, "reply_required": True}},
                    {"id": "ev2", "ts": "2026-03-07T00:01:00Z", "kind": "system.notify", "by": "system", "data": {"title": "Need review", "requires_ack": True}},
                    {"id": "ev3", "ts": "2026-03-07T00:02:00Z", "kind": "chat.message", "by": "user", "data": {"text": "extra"}},
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
            {"id", "ts", "by", "kind", "signal_family", "reply_required", "text_preview"},
        )
        self.assertEqual(preview["messages"][0]["kind"], "chat.message")
        self.assertEqual(preview["messages"][0]["signal_family"], "work_chat")
        self.assertTrue(preview["messages"][0]["reply_required"] is True)
        self.assertLessEqual(len(preview["messages"][0]["text_preview"]), 220)
        self.assertEqual(preview["messages"][1]["kind"], "system.notify")
        self.assertEqual(preview["messages"][1]["signal_family"], "interrupt")
        self.assertEqual(preview["messages"][1]["text_preview"], "Need review")

    def test_bootstrap_inbox_preview_exposes_notify_kind_for_interrupt_notifies(self) -> None:
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
            "inbox_list",
            return_value={
                "messages": [
                    {
                        "id": "ev1",
                        "ts": "2026-03-07T00:01:00Z",
                        "kind": "system.notify",
                        "by": "system",
                        "data": {"kind": "help_nudge", "title": "Help updated", "requires_ack": False},
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
        self.assertEqual(message["kind"], "system.notify")
        self.assertEqual(message["notify_kind"], "help_nudge")
        self.assertEqual(message["signal_family"], "interrupt")
        self.assertTrue(message["reply_required"] is False)
        self.assertEqual(message["text_preview"], "Help updated")


if __name__ == "__main__":
    unittest.main()
