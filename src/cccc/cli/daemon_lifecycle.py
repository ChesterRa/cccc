"""Daemon lifecycle helpers: stop & monitor with dependency injection.

Extracted from common.py so tests can import and drive the real logic
with injectable dependencies (call_daemon, start_daemon, etc.).
"""

from __future__ import annotations

import threading
import time
from dataclasses import dataclass, field
from typing import Any, Callable, Optional

from ..util.process import terminate_pid


# Default backoff schedule for bounded restart.
DEFAULT_RESTART_BACKOFFS: list[float] = [0.5, 1.0, 2.0]


@dataclass
class DaemonLifecycle:
    """Manages daemon process lifecycle with thread-safe stop & monitor.

    All external side-effects are injected via callables so the class
    can be tested without subprocess or IPC.
    """

    # --- injected deps ---
    call_daemon: Callable[[dict, float], dict]
    start_daemon: Callable[[], bool]
    is_shutdown_requested: Callable[[], bool]
    log: Callable[[str], None]

    # Optional: read recent log lines for crash diagnostics.
    read_log_tail: Callable[[int], list[str]] = field(
        default_factory=lambda: (lambda _n: [])
    )

    restart_backoffs: list[float] = field(
        default_factory=lambda: list(DEFAULT_RESTART_BACKOFFS)
    )

    # --- internal state ---
    _lock: threading.RLock = field(default_factory=threading.RLock)
    _process: Any = field(default=None, repr=False)

    # ------------------------------------------------------------------ #
    #  Public API                                                         #
    # ------------------------------------------------------------------ #

    @property
    def process(self) -> Any:
        with self._lock:
            return self._process

    @process.setter
    def process(self, value: Any) -> None:
        with self._lock:
            self._process = value

    def stop_daemon(self) -> None:
        """Send shutdown to owned daemon; no-op when shared (process is None)."""
        with self._lock:
            if self._process is None:
                return
            try:
                self.call_daemon({"op": "shutdown"}, 2.0)
            except Exception:
                pass
            self._wait_and_kill_locked()
            self._process = None

    def monitor_daemon(self) -> None:
        """Background thread body: watch daemon, auto-restart on crash.

        Constraints:
        1. On child exit, ping first — if another daemon answers, enter
           shared mode (process=None, stop monitoring).
        2. Bounded retry with backoff (default max 3 attempts).
        3. Thread-safe updates to process reference via _lock.
        4. Shutdown checks before/after backoff sleep and inside lock
           to avoid orphan processes on Ctrl+C.
        """
        restart_count = 0

        while not self.is_shutdown_requested():
            with self._lock:
                proc = self._process
            if proc is None:
                break

            ret = proc.poll()
            if ret is None:
                # Still running
                time.sleep(1.0)
                continue

            # Child exited
            if self.is_shutdown_requested():
                break

            self.log(f"Daemon exited (exit code {ret})")
            for line in self.read_log_tail(15):
                self.log(f"  {line}")

            # Constraint 1: ping to detect takeover
            try:
                ping_resp = self.call_daemon({"op": "ping"}, 1.0)
            except Exception:
                ping_resp = {}
            if ping_resp.get("ok"):
                self.log("Another daemon is running; entering shared mode.")
                with self._lock:
                    self._process = None
                break

            # Constraint 2: bounded retry
            if restart_count >= len(self.restart_backoffs):
                self.log("FATAL: Daemon restart limit reached. Giving up.")
                break

            backoff = self.restart_backoffs[restart_count]
            restart_count += 1
            self.log(
                f"Restarting daemon (attempt {restart_count}/"
                f"{len(self.restart_backoffs)}, backoff {backoff}s)..."
            )
            time.sleep(backoff)

            # Constraint 4: re-check shutdown after backoff, before Popen.
            # start_daemon stays inside the lock so _stop_daemon cannot race
            # past us — if Ctrl+C fires during the 5s start wait, _stop_daemon
            # blocks on the lock and will see the new process when it resumes.
            with self._lock:
                if self.is_shutdown_requested():
                    break
                ok = self.start_daemon()
                if ok:
                    self.log("Daemon restarted successfully.")
                else:
                    self.log("Daemon restart failed.")

    # ------------------------------------------------------------------ #
    #  Internal helpers                                                   #
    # ------------------------------------------------------------------ #

    def _wait_and_kill_locked(self) -> None:
        """Wait for subprocess exit, escalate to terminate/kill. Must hold _lock."""
        import subprocess as _sp

        target_pid = int(getattr(self._process, "pid", 0) or 0)
        if target_pid > 0:
            try:
                if terminate_pid(target_pid, timeout_s=12.0, include_group=True, force=True):
                    return
            except Exception:
                pass

        try:
            self._process.wait(timeout=10.0)
        except (_sp.TimeoutExpired, TimeoutError):
            try:
                self._process.terminate()
                self._process.wait(timeout=2.0)
            except Exception:
                try:
                    self._process.kill()
                except Exception:
                    pass
        except Exception:
            pass
