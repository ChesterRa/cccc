import unittest
from unittest.mock import patch


class TestMcpAutomationManageActorIdAlias(unittest.TestCase):
    def test_automation_manage_accepts_actor_id_with_canonical_rule_shape(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        captured = {}

        def _fake_call_daemon(req):
            captured["req"] = req
            return {"ok": True, "result": {"version": 1}}

        with patch.object(mcp_server, "call_daemon", side_effect=_fake_call_daemon):
            out = mcp_server.handle_tool_call(
                "cccc_automation_manage",
                {
                    "group_id": "g_test",
                    "actor_id": "foreman",
                    "op": "create",
                    "rule": {
                        "id": "tokyo_weather_report",
                        "enabled": True,
                        "scope": "group",
                        "to": ["@foreman"],
                        "trigger": {"kind": "at", "at": "2099-01-01T00:00:00Z"},
                        "action": {"kind": "notify", "message": "30 minutes check"},
                    },
                },
            )

        self.assertEqual(out.get("version"), 1)
        req = captured.get("req") if isinstance(captured.get("req"), dict) else {}
        self.assertEqual(req.get("op"), "group_automation_manage")
        args = req.get("args") if isinstance(req.get("args"), dict) else {}
        self.assertEqual(args.get("by"), "foreman")
        actions = args.get("actions") if isinstance(args.get("actions"), list) else []
        self.assertEqual(len(actions), 1)
        action = actions[0] if isinstance(actions[0], dict) else {}
        self.assertEqual(action.get("type"), "create_rule")
        rule = action.get("rule") if isinstance(action.get("rule"), dict) else {}
        self.assertEqual(rule.get("id"), "tokyo_weather_report")
        trigger = rule.get("trigger") if isinstance(rule.get("trigger"), dict) else {}
        self.assertEqual(trigger.get("kind"), "at")
        self.assertEqual(trigger.get("at"), "2099-01-01T00:00:00Z")
        notify_action = rule.get("action") if isinstance(rule.get("action"), dict) else {}
        self.assertEqual(notify_action.get("kind"), "notify")
        self.assertEqual(notify_action.get("message"), "30 minutes check")


if __name__ == "__main__":
    unittest.main()
