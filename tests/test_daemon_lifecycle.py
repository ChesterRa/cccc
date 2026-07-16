"""Tests for DaemonLifecycle in cccc/cli/daemon_lifecycle.py.

Imports the REAL DaemonLifecycle class and drives it with injectable
fake dependencies — no reimplemented mirror logic.

Covers:
1. Shared daemon: stop_daemon does NOT send shutdown when process is None
2. Owned daemon: stop_daemon sends shutdown
3. Owned child crash + ping fails: bounded restart with backoff
4. Child exit + ping succeeds: enter shared mode (process=None)
5. Ctrl+C race with restart: no orphan processes
"""

from __future__ import annotations

import threading
import time
import unittest

from cccc.cli.daemon_lifecycle import DaemonLifecycle


class _FakeProcess:
    """Minimal subprocess.Popen stand-in."""

    def __init__(self, *, exit_code: int | None = None, pid: int = 4321):
        self._exit_code = exit_code
        self.pid = pid
        self.terminate_called = False
        self.kill_called = False

    def poll(self) -> int | None:
        return self._exit_code

    def wait(self, timeout: float = 0) -> int:
        if self._exit_code is None:
            raise TimeoutError
        return self._exit_code

    def terminate(self) -> None:
        self.terminate_called = True
        self._exit_code = -15

    def kill(self) -> None:
        self.kill_called = True
        self._exit_code = -9

    def set_exit(self, code: int) -> None:
        self._exit_code = code


class TestStopDaemonGuard(unittest.TestCase):
    """Fix 1: stop_daemon only sends shutdown when process is not None."""

    def test_shared_daemon_no_shutdown(self) -> None:
        """When process is None (shared mode), stop_daemon must NOT call shutdown."""
        call_log: list[dict] = []

        lc = DaemonLifecycle(
            call_daemon=lambda req, t: (call_log.append(req), {"ok": True})[1],
            start_daemon=lambda: True,
            is_shutdown_requested=lambda: False,
            log=lambda _: None,
        )
        # process defaults to None → shared mode
        lc.stop_daemon()
        self.assertEqual(call_log, [], "Should not send shutdown when process is None")

    def test_owned_daemon_sends_shutdown(self) -> None:
        """When process is set (we own it), stop_daemon sends shutdown."""
        call_log: list[dict] = []

        lc = DaemonLifecycle(
            call_daemon=lambda req, t: (call_log.append(req), {"ok": True})[1],
            start_daemon=lambda: True,
            is_shutdown_requested=lambda: False,
            log=lambda _: None,
        )
        lc.process = _FakeProcess(exit_code=0)

        lc.stop_daemon()
        self.assertEqual(len(call_log), 1)
        self.assertEqual(call_log[0]["op"], "shutdown")

    def test_owned_daemon_prefers_tree_termination_for_pid_process(self) -> None:
        """Owned daemon stop should use shared tree termination when a PID is available."""
        from cccc.cli import daemon_lifecycle as lifecycle_mod

        call_log: list[dict] = []
        lc = DaemonLifecycle(
            call_daemon=lambda req, t: (call_log.append(req), {"ok": True})[1],
            start_daemon=lambda: True,
            is_shutdown_requested=lambda: False,
            log=lambda _: None,
        )
        proc = _FakeProcess(exit_code=None, pid=9876)
        lc.process = proc

        with unittest.mock.patch.object(lifecycle_mod, "terminate_pid", return_value=True) as mock_terminate:
            lc.stop_daemon()

        self.assertEqual(len(call_log), 1)
        self.assertEqual(call_log[0]["op"], "shutdown")
        mock_terminate.assert_called_once_with(9876, timeout_s=12.0, include_group=True, force=True)
        self.assertIsNone(lc.process)


class TestMonitorDaemon(unittest.TestCase):
    """Fix 2: monitor_daemon auto-restart with constraints."""

    def test_crash_ping_fail_bounded_restart(self) -> None:
        """Owned child crashes, ping fails → restart up to 3 times then give up."""
        restart_attempts: list[float] = []

        def fake_call_daemon(req: dict, timeout: float) -> dict:
            if req.get("op") == "ping":
                return {"ok": False}
            return {"ok": True}

        def fake_start_daemon() -> bool:
            restart_attempts.append(time.monotonic())
            # Each restart also "crashes" immediately
            lc.process = _FakeProcess(exit_code=1)
            return True

        lc = DaemonLifecycle(
            call_daemon=fake_call_daemon,
            start_daemon=fake_start_daemon,
            is_shutdown_requested=lambda: False,
            log=lambda _: None,
            restart_backoffs=[0.05, 0.1, 0.15],  # Fast for testing
        )
        lc.process = _FakeProcess(exit_code=1)

        t = threading.Thread(target=lc.monitor_daemon, daemon=True)
        t.start()
        t.join(timeout=5.0)

        self.assertEqual(len(restart_attempts), 3, "Should attempt exactly 3 restarts")

    def test_exit_ping_success_shared_mode(self) -> None:
        """Child exits but another daemon responds to ping → enter shared mode."""
        entered_shared = threading.Event()

        def fake_call_daemon(req: dict, timeout: float) -> dict:
            if req.get("op") == "ping":
                return {"ok": True, "result": {"version": "0.4.3", "pid": 12345}}
            return {"ok": True}

        lc = DaemonLifecycle(
            call_daemon=fake_call_daemon,
            start_daemon=lambda: True,
            is_shutdown_requested=lambda: False,
            log=lambda _: None,
        )
        lc.process = _FakeProcess(exit_code=0)

        original_monitor = lc.monitor_daemon

        def monitor_and_signal() -> None:
            original_monitor()
            if lc.process is None:
                entered_shared.set()

        t = threading.Thread(target=monitor_and_signal, daemon=True)
        t.start()
        t.join(timeout=2.0)

        self.assertTrue(entered_shared.is_set(), "Should enter shared mode")
        self.assertIsNone(lc.process, "process should be None in shared mode")

    def test_shutdown_race_no_orphan(self) -> None:
        """Ctrl+C sets shutdown_requested while monitor is in backoff → no restart."""
        restart_attempts: list[int] = []
        shutdown_flag = [False]

        def fake_call_daemon(req: dict, timeout: float) -> dict:
            if req.get("op") == "ping":
                return {"ok": False}
            return {"ok": True}

        def fake_start_daemon() -> bool:
            restart_attempts.append(1)
            lc.process = _FakeProcess(exit_code=None)
            return True

        lc = DaemonLifecycle(
            call_daemon=fake_call_daemon,
            start_daemon=fake_start_daemon,
            is_shutdown_requested=lambda: shutdown_flag[0],
            log=lambda _: None,
            restart_backoffs=[0.1, 0.2, 0.3],
        )
        lc.process = _FakeProcess(exit_code=1)

        # Set shutdown after a tiny delay (before first backoff completes)
        def _set_shutdown() -> None:
            time.sleep(0.05)
            shutdown_flag[0] = True

        threading.Thread(target=_set_shutdown, daemon=True).start()
        t = threading.Thread(target=lc.monitor_daemon, daemon=True)
        t.start()
        t.join(timeout=3.0)

        self.assertEqual(len(restart_attempts), 0, "Should not restart when shutdown requested")

    def test_shutdown_during_start_daemon_no_orphan(self) -> None:
        """Shutdown fires while start_daemon() is executing inside the lock.

        Because start_daemon runs under _lock, _stop_daemon blocks until
        start_daemon finishes. When _stop_daemon finally acquires the lock
        it must see and clean up the newly started process — no orphan.
        """
        shutdown_flag = [False]
        started_processes: list[_FakeProcess] = []
        stop_saw_process = [None]  # what _stop_daemon sees

        def fake_call_daemon(req: dict, timeout: float) -> dict:
            if req.get("op") == "ping":
                return {"ok": False}
            return {"ok": True}

        def slow_start_daemon() -> bool:
            """Simulate a start that takes a while (the 5s wait loop)."""
            time.sleep(0.15)  # Simulate startup latency
            new_proc = _FakeProcess(exit_code=None)  # running
            started_processes.append(new_proc)
            # Directly set _process under the lock (we're already inside it
            # because monitor_daemon holds the lock when calling start_daemon).
            lc._process = new_proc
            return True

        lc = DaemonLifecycle(
            call_daemon=fake_call_daemon,
            start_daemon=slow_start_daemon,
            is_shutdown_requested=lambda: shutdown_flag[0],
            log=lambda _: None,
            restart_backoffs=[0.01],  # Tiny backoff so we get to start quickly
        )
        lc.process = _FakeProcess(exit_code=1)  # Crashed child

        monitor_done = threading.Event()

        def run_monitor() -> None:
            lc.monitor_daemon()
            monitor_done.set()

        t_monitor = threading.Thread(target=run_monitor, daemon=True)
        t_monitor.start()

        # Wait a bit for monitor to enter start_daemon (inside lock)
        time.sleep(0.08)

        # Now fire shutdown + stop_daemon concurrently
        shutdown_flag[0] = True

        def run_stop() -> None:
            lc.stop_daemon()
            stop_saw_process[0] = "called"

        t_stop = threading.Thread(target=run_stop, daemon=True)
        t_stop.start()

        t_monitor.join(timeout=3.0)
        t_stop.join(timeout=3.0)

        # After stop_daemon, process must be None — no orphan
        self.assertIsNone(lc.process, "stop_daemon must clean up process started during race")
        # The new process that was started should have been seen by stop_daemon
        self.assertEqual(len(started_processes), 1, "start_daemon should have been called once")


if __name__ == "__main__":
    unittest.main()
