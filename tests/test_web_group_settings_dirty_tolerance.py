import os
import tempfile
import unittest

from fastapi.testclient import TestClient


class TestWebGroupSettingsDirtyTolerance(unittest.TestCase):
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

    def test_group_settings_get_tolerates_dirty_numeric_values(self) -> None:
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="web-settings-dirty", topic="")
            group_id = group.group_id

            loaded = load_group(group_id)
            self.assertIsNotNone(loaded)
            assert loaded is not None
            loaded.doc["automation"] = {
                "nudge_after_seconds": "oops",
                "reply_required_nudge_after_seconds": "-99",
                "attention_ack_nudge_after_seconds": None,
                "unread_nudge_after_seconds": "100",
                "nudge_digest_min_interval_seconds": "",
                "nudge_max_repeats_per_obligation": "bad",
                "nudge_escalate_after_repeats": "-1",
                "actor_idle_timeout_seconds": "abc",
                "keepalive_delay_seconds": "120",
                "keepalive_max_per_actor": "-5",
                "silence_timeout_seconds": "900",
                "help_nudge_interval_seconds": {},
                "help_nudge_min_messages": [],
            }
            loaded.doc["delivery"] = {
                "min_interval_seconds": "bad",
                "auto_mark_on_delivery": "false",
            }
            loaded.doc["terminal_transcript"] = {
                "visibility": "foreman",
                "notify_tail": "true",
                "notify_lines": "bad",
            }
            loaded.save()

            app = create_app()
            client = TestClient(app)
            resp = client.get(f"/api/v1/groups/{group_id}/settings")
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("ok"))
            settings = ((body.get("result") or {}).get("settings") or {})

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
            cleanup()


if __name__ == "__main__":
    unittest.main()
