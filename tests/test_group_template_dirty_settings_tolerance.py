import os
import tempfile
import unittest


class TestGroupTemplateDirtySettingsTolerance(unittest.TestCase):
    def test_build_template_tolerates_dirty_numeric_and_bool_settings(self) -> None:
        from cccc.kernel.group import create_group
        from cccc.kernel.group_template import build_group_template_from_group, preview_group_template_replace
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                reg = load_registry()
                group = create_group(reg, title="dirty", topic="")

                group.doc["automation"] = {
                    "nudge_after_seconds": "bad",
                    "reply_required_nudge_after_seconds": "bad",
                    "attention_ack_nudge_after_seconds": "bad",
                    "unread_nudge_after_seconds": "bad",
                    "nudge_digest_min_interval_seconds": "bad",
                    "nudge_max_repeats_per_obligation": "bad",
                    "nudge_escalate_after_repeats": "bad",
                    "actor_idle_timeout_seconds": "bad",
                    "keepalive_delay_seconds": "bad",
                    "keepalive_max_per_actor": "bad",
                    "silence_timeout_seconds": "bad",
                    "help_nudge_interval_seconds": "bad",
                    "help_nudge_min_messages": "bad",
                    "rules": [],
                    "snippets": {},
                }
                group.doc["delivery"] = {
                    "auto_mark_on_delivery": "false",
                    "min_interval_seconds": "bad",
                }
                group.save()

                tpl = build_group_template_from_group(group)
                self.assertEqual(tpl.settings.nudge_after_seconds, 300)
                self.assertEqual(tpl.settings.reply_required_nudge_after_seconds, 300)
                self.assertEqual(tpl.settings.min_interval_seconds, 0)
                self.assertFalse(bool(tpl.settings.auto_mark_on_delivery))

                diff = preview_group_template_replace(group, tpl)
                self.assertEqual(diff.settings_changed, {})
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
