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


if __name__ == "__main__":
    unittest.main()
