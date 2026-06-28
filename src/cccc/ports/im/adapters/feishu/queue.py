"""Thread-safe inbound message queue for Feishu."""

from __future__ import annotations

import threading
from typing import Any, Dict, List


class FeishuMessageQueue:
    def __init__(self) -> None:
        self._messages: List[Dict[str, Any]] = []
        self._lock = threading.Lock()

    def append(self, message: Dict[str, Any]) -> None:
        with self._lock:
            self._messages.append(message)

    def drain(self) -> List[Dict[str, Any]]:
        with self._lock:
            messages = self._messages.copy()
            self._messages.clear()
            return messages

    def clear(self) -> None:
        with self._lock:
            self._messages.clear()
