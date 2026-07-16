import unittest

from cccc.ports.im.adapters.feishu.queue import FeishuMessageQueue


class TestFeishuMessageQueue(unittest.TestCase):
    def test_drain_returns_messages_and_clears_queue(self) -> None:
        queue = FeishuMessageQueue()
        queue.append({"message_id": "om_1"})
        queue.append({"message_id": "om_2"})

        self.assertEqual(queue.drain(), [{"message_id": "om_1"}, {"message_id": "om_2"}])
        self.assertEqual(queue.drain(), [])

    def test_clear_removes_pending_messages(self) -> None:
        queue = FeishuMessageQueue()
        queue.append({"message_id": "om_1"})

        queue.clear()

        self.assertEqual(queue.drain(), [])


if __name__ == "__main__":
    unittest.main()
