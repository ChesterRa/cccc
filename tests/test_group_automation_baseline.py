import os
import tempfile
import unittest


class TestGroupAutomationBaseline(unittest.TestCase):
    def test_group_create_exposes_default_standup_and_preserves_builtin_after_clear(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group

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
                self.assertFalse(bool(standup_rule.get("enabled")), "default standup should be seeded but disabled")
                self.assertIn("standup", snippets)
                standup_snippet = str(snippets.get("standup") or "")
                self.assertIn("Keep this short.", standup_snippet)
                self.assertIn("coordination interrupt", standup_snippet)
                self.assertIn("cccc_help", standup_snippet)
                self.assertNotIn("Checklist:", standup_snippet)
                self.assertNotIn("Recall:", standup_snippet)

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
                clear_result = clear_resp.result or {}
                clear_ruleset = clear_result.get("ruleset") if isinstance(clear_result.get("ruleset"), dict) else {}
                clear_snippets = clear_ruleset.get("snippets") if isinstance(clear_ruleset.get("snippets"), dict) else {}
                self.assertIn("standup", clear_snippets, "update response should expose effective built-in snippets")

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
                self.assertIn("standup", snippets2, "built-in standup should still be available after clear")
                self.assertEqual(str(snippets2.get("standup") or ""), standup_snippet)

                group = load_group(group_id)
                self.assertIsNotNone(group)
                automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
                expected_interrupt_defaults = {
                    "nudge_after_seconds": 0,
                    "reply_required_nudge_after_seconds": 300,
                    "attention_ack_nudge_after_seconds": 600,
                    "unread_nudge_after_seconds": 0,
                    "actor_idle_timeout_seconds": 0,
                    "keepalive_delay_seconds": 0,
                    "silence_timeout_seconds": 0,
                    "help_nudge_interval_seconds": 0,
                    "help_nudge_min_messages": 0,
                }
                for key, expected in expected_interrupt_defaults.items():
                    self.assertEqual(automation.get(key), expected, f"unexpected new-group default for {key}")
                self.assertEqual(automation.get("snippets"), {}, "stored custom snippets should be cleared")
                self.assertEqual(automation.get("snippet_overrides"), {}, "built-in overrides should be cleared")
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_legacy_group_without_interrupt_fields_keeps_compatibility_defaults(self) -> None:
        from pathlib import Path

        from cccc.daemon.automation.engine import _cfg
        from cccc.kernel.group import Group

        group = Group(group_id="g_legacy", path=Path("/unused"), doc={"automation": {}})
        cfg = _cfg(group)

        self.assertEqual(cfg.nudge_after_seconds, 300)
        self.assertEqual(cfg.reply_required_nudge_after_seconds, 300)
        self.assertEqual(cfg.attention_ack_nudge_after_seconds, 600)
        self.assertEqual(cfg.unread_nudge_after_seconds, 900)
        self.assertEqual(cfg.keepalive_delay_seconds, 120)
        self.assertEqual(cfg.help_nudge_interval_seconds, 600)
        self.assertEqual(cfg.help_nudge_min_messages, 10)

    def test_baseline_ruleset_seed_does_not_migrate_legacy_interrupt_fields(self) -> None:
        from cccc.kernel.group import default_automation_ruleset_doc

        seed = default_automation_ruleset_doc()

        self.assertNotIn("nudge_after_seconds", seed)
        self.assertNotIn("keepalive_delay_seconds", seed)
        self.assertNotIn("help_nudge_interval_seconds", seed)

    def test_legacy_flat_snippets_migrate_to_custom_and_builtin_override_layers(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import _DEFAULT_AUTOMATION_STANDUP_SNIPPET, load_group

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

                group = load_group(group_id)
                self.assertIsNotNone(group)
                automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
                automation["snippets"] = {
                    "standup": "custom standup override",
                    "custom_note": "hello",
                    "legacy_default": _DEFAULT_AUTOMATION_STANDUP_SNIPPET,
                }
                automation.pop("snippet_overrides", None)
                group.doc["automation"] = automation
                group.save()

                state_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_automation_state", "args": {"group_id": group_id, "by": "user"}}
                    )
                )
                self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))

                group = load_group(group_id)
                self.assertIsNotNone(group)
                automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
                self.assertEqual(
                    automation.get("snippets"),
                    {"custom_note": "hello", "legacy_default": _DEFAULT_AUTOMATION_STANDUP_SNIPPET},
                )
                self.assertEqual(automation.get("snippet_overrides"), {"standup": "custom standup override"})

                result = state_resp.result or {}
                catalog = result.get("snippet_catalog") if isinstance(result.get("snippet_catalog"), dict) else {}
                self.assertEqual((catalog.get("built_in_overrides") or {}).get("standup"), "custom standup override")
                self.assertEqual((catalog.get("custom") or {}).get("custom_note"), "hello")
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_explicit_snippet_override_wins_over_legacy_flat_builtin_copy(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import _DEFAULT_AUTOMATION_STANDUP_SNIPPET, load_group

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

                group = load_group(group_id)
                self.assertIsNotNone(group)
                automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
                automation["snippets"] = {"standup": _DEFAULT_AUTOMATION_STANDUP_SNIPPET}
                automation["snippet_overrides"] = {"standup": "explicit override"}
                group.doc["automation"] = automation
                group.save()

                state_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_automation_state", "args": {"group_id": group_id, "by": "user"}}
                    )
                )
                self.assertTrue(state_resp.ok, getattr(state_resp, "error", None))

                group = load_group(group_id)
                self.assertIsNotNone(group)
                automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
                self.assertEqual(automation.get("snippet_overrides"), {"standup": "explicit override"})
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
