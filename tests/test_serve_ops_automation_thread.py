from __future__ import annotations

import threading
import time
import unittest
from pathlib import Path


class TestServeOpsAutomationThread(unittest.TestCase):
    def test_automation_tick_honors_initial_delay_and_interval(self) -> None:
        from cccc.daemon.serve_ops import start_automation_thread

        stop_event = threading.Event()
        tick_times: list[float] = []

        def automation_tick(*, home: Path) -> None:
            tick_times.append(time.monotonic())

        thread = start_automation_thread(
            stop_event=stop_event,
            home=Path("."),
            automation_tick=automation_tick,
            load_group=lambda _gid: None,
            group_running=lambda _gid: False,
            tick_delivery=lambda _group: None,
            compact_ledgers=lambda _home: None,
            automation_interval_seconds=0.03,
            initial_automation_delay_seconds=0.05,
        )
        try:
            time.sleep(0.03)
            self.assertEqual(tick_times, [])
            time.sleep(0.08)
            self.assertGreaterEqual(len(tick_times), 1)
            first_tick = tick_times[0]
            time.sleep(0.08)
            self.assertGreaterEqual(len(tick_times), 2)
            self.assertGreaterEqual(tick_times[1] - first_tick, 0.02)
        finally:
            stop_event.set()
            thread.join(timeout=2.0)
