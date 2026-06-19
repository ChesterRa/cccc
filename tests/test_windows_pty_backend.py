import os
import queue
import selectors
import threading
import time
import unittest
from types import SimpleNamespace
from collections import deque
from pathlib import Path


def _windows_pty_diagnostics() -> str:
    from cccc.runners.platform_support import pty_support_details

    return f"details={pty_support_details()!r}"


class _WakeSocket:
    def __init__(self) -> None:
        self._reads = 0

    def recv(self, _size: int) -> bytes:
        self._reads += 1
        return b"x" if self._reads == 1 else b""


class _NonReentrantLock:
    def __init__(self) -> None:
        self._held = False

    def __enter__(self):
        if self._held:
            raise AssertionError("lock re-entered")
        self._held = True
        return self

    def __exit__(self, exc_type, exc, tb) -> bool:
        self._held = False
        return False


class TestWindowsPtyBackendInternals(unittest.TestCase):
    def test_on_wake_readable_does_not_reenter_session_lock(self) -> None:
        from cccc.runners.pty_win import PtySession

        session = object.__new__(PtySession)
        session._wake_r = _WakeSocket()
        session._attach_q = queue.Queue()
        session._output_q = queue.Queue()
        session._output_q.put(b"hello")
        session._lock = _NonReentrantLock()
        session._clients = {}
        session._running = True
        session._backlog = deque()
        session._backlog_bytes = 0
        session._first_output_at = None
        session._last_output_at = None
        session._max_backlog_bytes = 1024
        session._terminal_signal_buffer = ""
        session._runtime = "codex"
        session._terminal_override = None
        session._max_client_buffer_bytes = 0

        session._on_wake_readable()

        self.assertEqual(session.tail_output(max_bytes=32), b"hello")

    def test_reader_loop_drains_output_after_fast_process_exit(self) -> None:
        from cccc.runners.pty_win import PtySession

        class _FastExitProc:
            def __init__(self) -> None:
                self._reads = 0

            def isalive(self) -> bool:
                return False

            def read(self, _size: int) -> bytes:
                self._reads += 1
                return b"CCCC_CONPTY_OK\r\n" if self._reads == 1 else b""

        session = object.__new__(PtySession)
        session._running = True
        session._proc = _FastExitProc()
        session._output_q = queue.Queue()
        session._maybe_reply_to_terminal_queries = lambda _data: None
        session._update_input_modes = lambda _data: None
        session._notify_wake = lambda: None

        session._reader_loop()

        self.assertEqual(session._output_q.get_nowait(), b"CCCC_CONPTY_OK\r\n")
        self.assertIsNone(session._output_q.get_nowait())

    def test_loop_waits_for_reader_sentinel_when_process_already_exited(self) -> None:
        from cccc.runners.pty_win import PtySession

        class _FakeSelector:
            def __init__(self, session: PtySession) -> None:
                self._session = session
                self._calls = 0

            def select(self, timeout: float = 0.0):
                _ = timeout
                self._calls += 1
                if self._calls == 1:
                    self._session._output_q.put(b"late output")
                else:
                    self._session._output_q.put(None)
                return [(SimpleNamespace(data=("wake", None)), selectors.EVENT_READ)]

        session = object.__new__(PtySession)
        session._running = True
        session._proc_alive = lambda: False
        session._selector = _FakeSelector(session)
        session._wake_r = _WakeSocket()
        session._attach_q = queue.Queue()
        session._output_q = queue.Queue()
        session._lock = threading.Lock()
        session._clients = {}
        session._backlog = deque()
        session._backlog_bytes = 0
        session._backlog_start_offset = 0
        session._backlog_end_offset = 0
        session._first_output_at = None
        session._last_output_at = None
        session._max_backlog_bytes = 1024
        session._terminal_signal_buffer = ""
        session._runtime = ""
        session._terminal_override = None
        session._max_client_buffer_bytes = 0
        session._terminate_process = lambda: None
        session._close_all = lambda: None
        session._on_exit = None

        session._loop()

        self.assertEqual(session.tail_output(max_bytes=64), b"late output")


@unittest.skipUnless(os.name == "nt", "Windows-only ConPTY backend check")
class TestWindowsPtyBackend(unittest.TestCase):
    def test_windows_pty_backend_is_available(self) -> None:
        from cccc.runners import pty as pty_runner

        self.assertTrue(
            bool(getattr(pty_runner, "PTY_SUPPORTED", False)),
            msg=f"Windows PTY backend unavailable (expected ConPTY via pywinpty). {_windows_pty_diagnostics()}",
        )

    def test_conpty_session_smoke_echo_output(self) -> None:
        from cccc.runners import pty as pty_runner

        self.assertTrue(
            bool(getattr(pty_runner, "PTY_SUPPORTED", False)),
            msg=f"Windows PTY backend unavailable before smoke echo. {_windows_pty_diagnostics()}",
        )

        session = pty_runner.PtySession(
            group_id="g_win",
            actor_id="a_win",
            cwd=Path.cwd(),
            command=["cmd.exe", "/c", "echo", "CCCC_CONPTY_OK"],
            env={},
        )
        try:
            deadline = time.time() + 8.0
            output = b""
            while time.time() < deadline:
                output = session.tail_output(max_bytes=200_000)
                if b"CCCC_CONPTY_OK" in output:
                    break
                if not session.is_running() and output:
                    break
                time.sleep(0.1)
            self.assertIn(
                b"CCCC_CONPTY_OK",
                output,
                msg=f"ConPTY session did not emit expected echo output. tail={output[-200:]}",
            )
        finally:
            session.stop()


if __name__ == "__main__":
    unittest.main()
