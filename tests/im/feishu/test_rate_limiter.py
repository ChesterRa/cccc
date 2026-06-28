import unittest
from unittest.mock import patch

from cccc.ports.im.adapters.feishu.rate_limiter import FeishuRateLimiter


class TestFeishuRateLimiter(unittest.TestCase):
    def test_acquire_tracks_rate_per_chat(self) -> None:
        limiter = FeishuRateLimiter(max_per_second=2.0)

        with patch("cccc.ports.im.adapters.feishu.rate_limiter.time.time", side_effect=[10.0, 10.1, 10.1]):
            self.assertEqual(limiter.acquire("chat-1"), 0.0)
            self.assertAlmostEqual(limiter.acquire("chat-1"), 0.4)
            self.assertEqual(limiter.acquire("chat-2"), 0.0)

    def test_wait_and_acquire_sleeps_then_marks_send(self) -> None:
        limiter = FeishuRateLimiter(max_per_second=2.0)

        with (
            patch("cccc.ports.im.adapters.feishu.rate_limiter.time.time", side_effect=[10.0, 10.1, 10.6]),
            patch("cccc.ports.im.adapters.feishu.rate_limiter.time.sleep") as sleep,
        ):
            limiter.wait_and_acquire("chat-1")
            limiter.wait_and_acquire("chat-1")

        sleep.assert_called_once()
        self.assertAlmostEqual(float(sleep.call_args.args[0]), 0.4)
        self.assertEqual(limiter.last_send["chat-1"], 10.6)
