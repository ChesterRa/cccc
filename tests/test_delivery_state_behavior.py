import os
import tempfile
import unittest


class TestDeliveryStateBehavior(unittest.TestCase):
    def test_should_deliver_message_respects_idle_and_paused_semantics(self) -> None:
        from cccc.daemon.delivery import should_deliver_message
        from cccc.kernel.group import create_group, set_group_state
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                reg = load_registry()
                group = create_group(reg, title="delivery-state")

                # active: allow chat + notify
                self.assertTrue(should_deliver_message(group, "chat.message"))
                self.assertTrue(should_deliver_message(group, "system.notify"))

                # idle: allow chat + notify; block other kinds
                group = set_group_state(group, state="idle")
                self.assertTrue(should_deliver_message(group, "chat.message"))
                self.assertTrue(should_deliver_message(group, "system.notify"))
                self.assertFalse(should_deliver_message(group, "chat.ack"))

                # paused: block all PTY delivery
                group = set_group_state(group, state="paused")
                self.assertFalse(should_deliver_message(group, "chat.message"))
                self.assertFalse(should_deliver_message(group, "system.notify"))
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
