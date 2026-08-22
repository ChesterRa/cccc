import os
import tempfile
import unittest


class TestDaemonGroupSettingsDirtyTolerance(unittest.TestCase):
    def test_group_load_promotes_only_known_legacy_flat_settings(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                create_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_create", "args": {"title": "legacy-flat", "by": "user"}}
                    )
                )
                self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
                group_id = str((create_resp.result or {}).get("group_id") or "").strip()
                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                group.doc["automation"]["keepalive_delay_seconds"] = 123
                group.doc["messaging"] = {"default_send_to": "foreman"}
                group.doc["settings"] = {
                    "keepalive_delay_seconds": 999,
                    "default_send_to": "broadcast",
                    "min_interval_seconds": 42,
                    "terminal_transcript_visibility": "all",
                    "terminal_transcript_notify_tail": False,
                    "terminal_transcript_notify_lines": 37,
                    "panorama_enabled": True,
                    "native_extension": {"keep": True},
                }
                group.save()

                promoted = load_group(group_id)
                self.assertIsNotNone(promoted)
                assert promoted is not None
                self.assertEqual(promoted.doc["automation"]["keepalive_delay_seconds"], 123)
                self.assertEqual(promoted.doc["messaging"]["default_send_to"], "foreman")
                self.assertEqual(promoted.doc["delivery"]["min_interval_seconds"], 0)
                self.assertEqual(promoted.doc["terminal_transcript"]["visibility"], "all")
                self.assertFalse(promoted.doc["terminal_transcript"]["notify_tail"])
                self.assertEqual(promoted.doc["terminal_transcript"]["notify_lines"], 37)
                self.assertTrue(promoted.doc["features"]["panorama_enabled"])
                self.assertEqual(promoted.doc["settings"], {"native_extension": {"keep": True}})
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_group_settings_update_returns_delivery_notice_defaults(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                create_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "group_create", "args": {"title": "daemon-settings-defaults", "topic": "", "by": "user"}}
                    )
                )
                self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
                group_id = str((create_resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)

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
                self.assertEqual(settings.get("mail_notice_after_seconds"), 1800)
                self.assertEqual(settings.get("reply_notice_after_seconds"), 900)
                self.assertNotIn("auto_mark_on_delivery", settings)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

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
                    "actor_idle_timeout_seconds": "bad",
                    "keepalive_delay_seconds": "bad",
                    "keepalive_max_per_actor": -2,
                    "silence_timeout_seconds": "bad",
                    "help_nudge_interval_seconds": "bad",
                    "help_nudge_min_messages": "bad",
                }
                group.doc["delivery"] = {
                    "min_interval_seconds": "bad",
                    "mail_notice_after_seconds": -1,
                    "reply_notice_after_seconds": "bad",
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
                self.assertEqual(settings.get("actor_idle_timeout_seconds"), 0)
                self.assertEqual(settings.get("keepalive_delay_seconds"), 120)
                self.assertEqual(settings.get("keepalive_max_per_actor"), 0)
                self.assertEqual(settings.get("silence_timeout_seconds"), 0)
                self.assertEqual(settings.get("help_nudge_interval_seconds"), 600)
                self.assertEqual(settings.get("help_nudge_min_messages"), 10)
                self.assertEqual(settings.get("min_interval_seconds"), 0)
                self.assertEqual(settings.get("mail_notice_after_seconds"), 0)
                self.assertEqual(settings.get("reply_notice_after_seconds"), 900)
                self.assertNotIn("auto_mark_on_delivery", settings)
                self.assertEqual(settings.get("terminal_transcript_notify_lines"), 20)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
