"""Keyed FIFO lanes for chat post-commit work."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
import logging
import threading
import time
from typing import Callable, Deque, Dict


@dataclass(frozen=True)
class _PostCommitTask:
    label: str
    fn: Callable[[], None]


class KeyedPostCommitLanes:
    """Run work serially per key while allowing different keys to progress."""

    def __init__(self, *, thread_name_prefix: str, logger: logging.Logger) -> None:
        self._thread_name_prefix = str(thread_name_prefix or "post-commit").strip() or "post-commit"
        self._logger = logger
        self._condition = threading.Condition()
        self._pending: Dict[str, Deque[_PostCommitTask]] = {}
        self._running: set[str] = set()

    def submit(self, key: str, label: str, fn: Callable[[], None]) -> None:
        lane_key = str(key or "global").strip() or "global"
        task = _PostCommitTask(label=str(label or "post-commit").strip() or "post-commit", fn=fn)
        should_start = False
        with self._condition:
            self._pending.setdefault(lane_key, deque()).append(task)
            if lane_key not in self._running:
                self._running.add(lane_key)
                should_start = True

        if not should_start:
            return

        thread = threading.Thread(
            target=self._drain,
            args=(lane_key,),
            name=f"{self._thread_name_prefix}-{lane_key}",
            daemon=True,
        )
        try:
            thread.start()
        except Exception:
            self._logger.exception("chat post-commit lane could not start key=%s", lane_key)
            self._drain(lane_key)

    def wait_for_idle_for_tests(self, *, timeout: float = 2.0) -> bool:
        deadline = time.monotonic() + max(0.0, float(timeout))
        with self._condition:
            while self._pending or self._running:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                self._condition.wait(timeout=remaining)
            return True

    def reset_for_tests(self) -> None:
        with self._condition:
            self._pending.clear()
            self._running.clear()
            self._condition.notify_all()

    def _drain(self, lane_key: str) -> None:
        while True:
            with self._condition:
                queue = self._pending.get(lane_key)
                if not queue:
                    self._pending.pop(lane_key, None)
                    self._running.discard(lane_key)
                    self._condition.notify_all()
                    return
                task = queue.popleft()

            try:
                task.fn()
            except Exception:
                self._logger.exception("chat post-commit task failed label=%s key=%s", task.label, lane_key)
