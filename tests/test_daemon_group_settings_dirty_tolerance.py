import os
import tempfile
import unittest


class TestDaemonGroupSettingsDirtyTolerance(unittest.TestCase):
    def test_group_settings_update_tolerates_dirty_numeric_values(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                create_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_create", "args": {"title": "daemon-settings-dirty", "topic": "", "by": "user"}}
                    )
                )
                self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
                group_id = str((create_resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                group.doc["automation"] = {
                    "nudge_after_seconds": "bad",
                    "reply_required_nudge_after_seconds": -1,
                    "attention_ack_nudge_after_seconds": "bad",
                    "unread_nudge_after_seconds": 100,
                    "nudge_digest_min_interval_seconds": "bad",
                    "nudge_max_repeats_per_obligation": "bad",
                    "nudge_escalate_after_repeats": -3,
                    "actor_idle_timeout_seconds": "bad",
                    "keepalive_delay_seconds": "bad",
                    "keepalive_max_per_actor": -2,
                    "silence_timeout_seconds": 900,
                    "help_nudge_interval_seconds": "bad",
                    "help_nudge_min_messages": "bad",
                }
                group.doc["delivery"] = {
                    "auto_mark_on_delivery": "false",
                    "min_interval_seconds": "bad",
                }
                group.doc["terminal_transcript"] = {
                    "visibility": "foreman",
                    "notify_tail": "true",
                    "notify_lines": "bad",
                }
                group.save()

                update_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {
                            "op": "group_settings_update",
                            "args": {
                                "group_id": group_id,
                                "by": "user",
                                "patch": {"default_send_to": "foreman"},
                            },
                        }
                    )
                )
                self.assertTrue(update_resp.ok, getattr(update_resp, "error", None))

                settings = ((update_resp.result or {}).get("settings") or {})
                self.assertEqual(settings.get("nudge_after_seconds"), 300)
                self.assertEqual(settings.get("reply_required_nudge_after_seconds"), 0)
                self.assertEqual(settings.get("attention_ack_nudge_after_seconds"), 600)
                self.assertEqual(settings.get("unread_nudge_after_seconds"), 100)
                self.assertEqual(settings.get("nudge_digest_min_interval_seconds"), 120)
                self.assertEqual(settings.get("nudge_max_repeats_per_obligation"), 3)
                self.assertEqual(settings.get("nudge_escalate_after_repeats"), 0)
                self.assertEqual(settings.get("actor_idle_timeout_seconds"), 600)
                self.assertEqual(settings.get("keepalive_delay_seconds"), 120)
                self.assertEqual(settings.get("keepalive_max_per_actor"), 0)
                self.assertEqual(settings.get("silence_timeout_seconds"), 900)
                self.assertEqual(settings.get("help_nudge_interval_seconds"), 600)
                self.assertEqual(settings.get("help_nudge_min_messages"), 10)
                self.assertEqual(settings.get("min_interval_seconds"), 0)
                self.assertFalse(bool(settings.get("auto_mark_on_delivery")))
                self.assertEqual(settings.get("terminal_transcript_notify_lines"), 20)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
