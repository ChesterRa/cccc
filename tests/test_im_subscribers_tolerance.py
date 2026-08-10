import json
import tempfile
import unittest
from pathlib import Path


class TestIMSubscribersTolerance(unittest.TestCase):
    def test_load_tolerates_dirty_bool_and_int_values(self) -> None:
        from cccc.ports.im.subscribers import SubscriberManager

        with tempfile.TemporaryDirectory() as td:
            state_dir = Path(td)
            payload = {
                "good": {"subscribed": True, "verbose": True, "thread_id": 0, "platform": "telegram"},
                "bools": {"subscribed": "false", "verbose": "false", "thread_id": "0"},
                "bad-int": {"subscribed": True, "verbose": True, "thread_id": "oops"},
            }
            (state_dir / "im_subscribers.json").write_text(json.dumps(payload), encoding="utf-8")

            manager = SubscriberManager(state_dir)

            self.assertFalse(manager.is_subscribed("bools"))
            self.assertFalse(manager.is_verbose("bools"))
            self.assertTrue(manager.is_subscribed("good"))
            self.assertTrue(manager.is_subscribed("bad-int"))
            self.assertEqual(manager.count(), 2)

    def test_missing_verbose_defaults_to_user_only_mode(self) -> None:
        from cccc.ports.im.subscribers import SubscriberManager

        with tempfile.TemporaryDirectory() as td:
            state_dir = Path(td)
            payload = {
                "legacy": {"subscribed": True, "thread_id": 0},
            }
            (state_dir / "im_subscribers.json").write_text(json.dumps(payload), encoding="utf-8")

            manager = SubscriberManager(state_dir)

            self.assertTrue(manager.is_subscribed("legacy"))
            self.assertFalse(manager.is_verbose("legacy"))

    def test_provider_owned_opaque_thread_ids_are_preserved(self) -> None:
        from cccc.ports.im.subscribers import SubscriberManager

        with tempfile.TemporaryDirectory() as td:
            state_dir = Path(td)
            payload = {
                "slack-channel": {
                    "subscribed": True,
                    "thread_id": "1710000000.100",
                    "platform": "slack",
                },
                "feishu-chat": {
                    "subscribed": True,
                    "thread_id": "om_thread_root",
                    "platform": "feishu",
                },
                "slack-channel:1710000000.200": {
                    "subscribed": True,
                    "thread_id": "oops",
                    "platform": "slack",
                },
            }
            (state_dir / "im_subscribers.json").write_text(json.dumps(payload), encoding="utf-8")

            manager = SubscriberManager(state_dir)

            self.assertTrue(manager.is_subscribed("slack-channel", "1710000000.100"))
            self.assertTrue(manager.is_subscribed("feishu-chat", "om_thread_root"))
            self.assertTrue(manager.is_subscribed("slack-channel", "1710000000.200"))
            self.assertFalse(manager.is_subscribed("slack-channel", "oops"))


if __name__ == "__main__":
    unittest.main()
