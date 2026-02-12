import os
import tempfile
import unittest


class TestGroupAutomationBaseline(unittest.TestCase):
    def test_group_create_seeds_default_standup_and_allows_intentional_clear(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                create_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_create", "args": {"title": "baseline", "topic": "", "by": "user"}}
                    )
                )
                self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
                group_id = str((create_resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

                state_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_automation_state", "args": {"group_id": group_id, "by": "user"}}
                    )
                )
                self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))
                result = state_resp.result or {}
                ruleset = result.get("ruleset") if isinstance(result.get("ruleset"), dict) else {}
                rules = ruleset.get("rules") if isinstance(ruleset.get("rules"), list) else []
                snippets = ruleset.get("snippets") if isinstance(ruleset.get("snippets"), dict) else {}

                standup_rule = None
                for rule in rules:
                    if isinstance(rule, dict) and str(rule.get("id") or "") == "standup":
                        standup_rule = rule
                        break
                self.assertIsNotNone(standup_rule, "default standup rule should exist on group_create")
                self.assertIn("standup", snippets)

                clear_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "group_automation_update",
                            "args": {
                                "group_id": group_id,
                                "by": "user",
                                "ruleset": {"rules": [], "snippets": {}},
                            },
                        }
                    )
                )
                self.assertTrue(clear_resp.ok, getattr(clear_resp, "error", None))

                recheck_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_automation_state", "args": {"group_id": group_id, "by": "user"}}
                    )
                )
                self.assertTrue(recheck_resp.ok, getattr(recheck_resp, "error", None))
                result2 = recheck_resp.result or {}
                ruleset2 = result2.get("ruleset") if isinstance(result2.get("ruleset"), dict) else {}
                rules2 = ruleset2.get("rules") if isinstance(ruleset2.get("rules"), list) else []
                snippets2 = ruleset2.get("snippets") if isinstance(ruleset2.get("snippets"), dict) else {}
                self.assertEqual(rules2, [], "user-cleared rules should stay cleared")
                self.assertEqual(snippets2, {}, "user-cleared snippets should stay cleared")
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
