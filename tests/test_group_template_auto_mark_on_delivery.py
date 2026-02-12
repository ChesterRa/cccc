import os
import tempfile
import unittest


class TestGroupTemplateAutoMarkOnDelivery(unittest.TestCase):
    def test_template_import_replace_applies_auto_mark_on_delivery(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                # Create group.
                resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_create", "args": {"title": "t", "topic": "", "by": "user"}}
                    )
                )
                self.assertTrue(resp.ok, getattr(resp, "error", None))
                group_id = str((resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

                # Attach a scope (required for import-replace which writes prompt files).
                scope_dir = os.path.join(td, "scope")
                os.makedirs(scope_dir, exist_ok=True)
                att, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "attach", "args": {"path": scope_dir, "group_id": group_id, "by": "user"}}
                    )
                )
                self.assertTrue(att.ok, getattr(att, "error", None))

                # Apply template with auto_mark_on_delivery=true.
                template = f"""
kind: cccc.group_template
v: 1
actors: []
settings:
  default_send_to: foreman
  nudge_after_seconds: 300
  reply_required_nudge_after_seconds: 111
  attention_ack_nudge_after_seconds: 222
  unread_nudge_after_seconds: 333
  nudge_digest_min_interval_seconds: 44
  nudge_max_repeats_per_obligation: 5
  nudge_escalate_after_repeats: 3
  auto_mark_on_delivery: true
  min_interval_seconds: 0
  standup_interval_seconds: 900
prompts: {{}}
"""
                imp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "group_template_import_replace",
                            "args": {
                                "group_id": group_id,
                                "by": "user",
                                "confirm": group_id,
                                "template": template,
                            },
                        }
                    )
                )
                self.assertTrue(imp.ok, getattr(imp, "error", None))

                group = load_group(group_id)
                self.assertIsNotNone(group)
                delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
                automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
                self.assertTrue(bool(delivery.get("auto_mark_on_delivery")))
                self.assertEqual(int(automation.get("reply_required_nudge_after_seconds", -1)), 111)
                self.assertEqual(int(automation.get("attention_ack_nudge_after_seconds", -1)), 222)
                self.assertEqual(int(automation.get("unread_nudge_after_seconds", -1)), 333)
                self.assertEqual(int(automation.get("nudge_digest_min_interval_seconds", -1)), 44)
                self.assertEqual(int(automation.get("nudge_max_repeats_per_obligation", -1)), 5)
                self.assertEqual(int(automation.get("nudge_escalate_after_repeats", -1)), 3)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_apply_settings_replace_coerces_falsey_string_toggles(self) -> None:
        from cccc.daemon.ops.template_ops import _apply_settings_replace
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                reg = load_registry()
                group = create_group(reg, title="t", topic="")

                patch = _apply_settings_replace(
                    group,
                    {
                        "auto_mark_on_delivery": "false",
                        "terminal_transcript_notify_tail": "false",
                    },
                )
                self.assertFalse(bool(patch.get("auto_mark_on_delivery")))
                self.assertFalse(bool(patch.get("terminal_transcript_notify_tail")))

                delivery = group.doc.get("delivery") if isinstance(group.doc.get("delivery"), dict) else {}
                self.assertFalse(bool(delivery.get("auto_mark_on_delivery")))
                terminal = group.doc.get("terminal_transcript") if isinstance(group.doc.get("terminal_transcript"), dict) else {}
                self.assertFalse(bool(terminal.get("notify_tail")))
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
