import os
import tempfile
import unittest


class TestAutomationManageLegacyShape(unittest.TestCase):
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

    def _create_group_id(self) -> str:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        resp, _ = handle_request(
            DaemonRequest.model_validate(
                {"op": "group_create", "args": {"title": "automation-legacy", "topic": "", "by": "user"}}
            )
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        group_id = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(group_id)
        return group_id

    def test_manage_accepts_legacy_rule_shape_via_simple_mode(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_id()

            manage_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "group_automation_manage",
                        "args": {
                            "group_id": group_id,
                            "by": "user",
                            "op": "create",
                            "rule": {
                                "name": "Tokyo Weather Report",
                                "enabled": True,
                                "scope": "group",
                                "to": ["@foreman"],
                                "trigger": {"kind": "interval", "every_minutes": 30},
                                "actions": [{"type": "send_message", "message": "30 minutes check"}],
                            },
                        },
                    }
                )
            )
            self.assertTrue(manage_resp.ok, getattr(manage_resp, "error", None))

            state_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {"op": "group_automation_state", "args": {"group_id": group_id, "by": "user"}}
                )
            )
            self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
            ruleset = (state_resp.result or {}).get("ruleset") if isinstance(state_resp.result, dict) else {}
            rules = ruleset.get("rules") if isinstance(ruleset, dict) else []
            matched = [r for r in rules if isinstance(r, dict) and str(r.get("id") or "") == "tokyo_weather_report"]
            self.assertEqual(len(matched), 1)
            rule = matched[0]
            self.assertEqual(str(rule.get("id") or ""), "tokyo_weather_report")
            trigger = rule.get("trigger") if isinstance(rule.get("trigger"), dict) else {}
            self.assertEqual(str(trigger.get("kind") or ""), "interval")
            self.assertEqual(int(trigger.get("every_seconds") or 0), 1800)
            action = rule.get("action") if isinstance(rule.get("action"), dict) else {}
            self.assertEqual(str(action.get("kind") or ""), "notify")
            self.assertEqual(str(action.get("message") or ""), "30 minutes check")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
