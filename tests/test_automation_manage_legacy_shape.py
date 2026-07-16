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

    def test_manage_rejects_simple_mode_and_legacy_rule_shape(self) -> None:
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
            self.assertFalse(manage_resp.ok)
            err = manage_resp.error.model_dump() if manage_resp.error else {}
            self.assertEqual(str(err.get("code") or ""), "invalid_request")
            self.assertIn("actions must be a non-empty array", str(err.get("message") or ""))
        finally:
            cleanup()

    def test_manage_rejects_legacy_rule_fields_in_actions(self) -> None:
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
                            "actions": [
                                {
                                    "type": "create_rule",
                                    "rule": {
                                        "name": "Tokyo Weather Report",
                                        "enabled": True,
                                        "scope": "group",
                                        "to": ["@foreman"],
                                        "trigger": {"kind": "interval", "every_minutes": 30},
                                        "actions": [{"type": "send_message", "message": "30 minutes check"}],
                                    },
                                }
                            ],
                        },
                    }
                )
            )
            self.assertFalse(manage_resp.ok)
            err = manage_resp.error.model_dump() if manage_resp.error else {}
            self.assertEqual(str(err.get("code") or ""), "group_automation_manage_failed")
            message = str(err.get("message") or "")
            self.assertIn("legacy automation rule shape is not supported", message)
            self.assertIn("name->id", message)
            self.assertIn("actions->action", message)
        finally:
            cleanup()

    def test_update_rejects_legacy_rule_fields_in_ruleset(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_id()

            update_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "group_automation_update",
                        "args": {
                            "group_id": group_id,
                            "by": "user",
                            "ruleset": {
                                "rules": [
                                    {
                                        "name": "legacy",
                                        "enabled": True,
                                        "scope": "group",
                                        "to": ["@foreman"],
                                        "trigger": {"kind": "interval", "every_minutes": 30},
                                        "actions": [{"type": "send_message", "message": "hello"}],
                                    }
                                ],
                                "snippets": {},
                            },
                        },
                    }
                )
            )
            self.assertFalse(update_resp.ok)
            err = update_resp.error.model_dump() if update_resp.error else {}
            self.assertEqual(str(err.get("code") or ""), "group_automation_update_failed")
            message = str(err.get("message") or "")
            self.assertIn("legacy automation rule shape is not supported", message)
            self.assertIn("ruleset.rules[0]", message)
        finally:
            cleanup()

    def test_replace_all_rejects_legacy_rule_fields_in_ruleset(self) -> None:
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
                            "actions": [
                                {
                                    "type": "replace_all_rules",
                                    "ruleset": {
                                        "rules": [
                                            {
                                                "name": "legacy",
                                                "enabled": True,
                                                "scope": "group",
                                                "to": ["@foreman"],
                                                "trigger": {"kind": "interval", "every_minutes": 30},
                                                "actions": [{"type": "send_message", "message": "hello"}],
                                            }
                                        ],
                                        "snippets": {},
                                    },
                                }
                            ],
                        },
                    }
                )
            )
            self.assertFalse(manage_resp.ok)
            err = manage_resp.error.model_dump() if manage_resp.error else {}
            self.assertEqual(str(err.get("code") or ""), "group_automation_manage_failed")
            message = str(err.get("message") or "")
            self.assertIn("legacy automation rule shape is not supported", message)
            self.assertIn("action.replace_all_rules.ruleset.rules[0]", message)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
