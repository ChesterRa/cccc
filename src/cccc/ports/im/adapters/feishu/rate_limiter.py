"""Feishu outbound per-chat rate limiting."""

from __future__ import annotations

import threading
import time
from typing import Dict


class FeishuRateLimiter:
    """
    Rate limiter for Feishu API.

    Feishu limits:
    - Same chat: ~5 msg/sec
    - Total: ~100 msg/sec
    """

    def __init__(self, max_per_second: float = 5.0):
        self.min_interval = 1.0 / max_per_second
        self.last_send: Dict[str, float] = {}
        self.lock = threading.Lock()

    def acquire(self, chat_id: str) -> float:
        """Return wait time in seconds, or 0 when sending can proceed now."""
        with self.lock:
            now = time.time()
            last = self.last_send.get(chat_id, 0)
            elapsed = now - last

            if elapsed >= self.min_interval:
                self.last_send[chat_id] = now
                return 0.0
            return self.min_interval - elapsed

    def wait_and_acquire(self, chat_id: str) -> None:
        wait_time = self.acquire(chat_id)
        if wait_time > 0:
            time.sleep(wait_time)
            self.acquire(chat_id)
