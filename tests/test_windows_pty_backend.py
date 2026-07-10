import os
import queue
import selectors
import threading
import time
import unittest
from types import SimpleNamespace
from collections import deque
from pathlib import Path
from unittest.mock import patch


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
    def _snapshot_session(self, *, group_id: str = "g1", actor_id: str = "a1"):
        from cccc.runners.pty_win import PtySession

        session = object.__new__(PtySession)
        session.group_id = group_id
        session.actor_id = actor_id
        session._runtime = ""
        session._lock = threading.Lock()
        session._backlog = deque()
        session._backlog_bytes = 0
        session._backlog_start_offset = 0
        session._backlog_end_offset = 0
        session._first_output_at = None
        session._last_output_at = None
        session._max_backlog_bytes = 1024
        session._terminal_signal_buffer = ""
        session._terminal_override = None
        session._mode_tail = b""
        session._query_tail = b""
        session._bracketed_paste = False
        session._bracketed_paste_changed_at = None
        return session

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

    def test_supervisor_keeps_tail_and_history_after_session_exit(self) -> None:
        from cccc.runners.pty_win import PtySupervisor

        supervisor = PtySupervisor()
        session = self._snapshot_session()
        session._append_backlog(b"failed\r\n")

        with supervisor._lock:
            supervisor._sessions[(session.group_id, session.actor_id)] = session

        supervisor._on_session_exit(session)

        self.assertFalse(supervisor.actor_running("g1", "a1"))
        self.assertEqual(
            supervisor.tail_output(group_id="g1", actor_id="a1", max_bytes=200),
            b"failed\r\n",
        )
        page = supervisor.history_page(group_id="g1", actor_id="a1", limit_bytes=200)
        self.assertEqual(page["data"], b"failed\r\n")
        self.assertEqual(page["start_cursor"], 0)
        self.assertEqual(page["end_cursor"], len(b"failed\r\n"))

    def test_supervisor_keeps_nonzero_exit_evidence_when_session_has_no_output(self) -> None:
        from cccc.runners.pty_win import PtySupervisor

        supervisor = PtySupervisor()
        session = self._snapshot_session()
        session._proc = SimpleNamespace(exitstatus=7)

        with supervisor._lock:
            supervisor._sessions[(session.group_id, session.actor_id)] = session

        supervisor._on_session_exit(session)

        output = supervisor.tail_output(group_id="g1", actor_id="a1", max_bytes=200)
        self.assertIn(b"Process exited with code 7 before producing terminal output.", output)

    def test_stop_waits_for_reader_output_before_closing_event_loop(self) -> None:
        from cccc.runners.pty_win import PtySession

        class _JoinThread:
            def __init__(self, on_join) -> None:
                self._on_join = on_join

            def is_alive(self) -> bool:
                return True

            def join(self, timeout: float) -> None:
                _ = timeout
                self._on_join()

        session = self._snapshot_session()
        session._running = True
        session._proc = SimpleNamespace(pid=0)
        session._output_q = queue.Queue()
        session._attach_q = queue.Queue()
        session._wake_r = _WakeSocket()
        session._clients = {}
        session._max_client_buffer_bytes = 0
        session._terminate_process = lambda: None
        session._notify_wake = lambda: None

        def _reader_finishes() -> None:
            if session._running:
                session._output_q.put(b"final output")
            session._output_q.put(None)

        session._reader_thread = _JoinThread(_reader_finishes)
        session._thread = _JoinThread(session._on_wake_readable)

        PtySession.stop(session)

        self.assertEqual(session.tail_output(max_bytes=64), b"final output")

    def test_stop_drains_output_queued_during_reader_timeout_fallback(self) -> None:
        from cccc.runners.pty_win import PtySession

        class _SlowReaderThread:
            def __init__(self) -> None:
                self._join_count = 0

            def is_alive(self) -> bool:
                return True

            def join(self, timeout: float) -> None:
                _ = timeout
                self._join_count += 1
                if self._join_count == 2:
                    session._output_q.put(b"late final output")
                    session._output_q.put(None)

        class _SlowLoopThread:
            def is_alive(self) -> bool:
                return True

            def join(self, timeout: float) -> None:
                _ = timeout

        session = self._snapshot_session()
        session._running = True
        session._proc = SimpleNamespace(pid=0)
        session._output_q = queue.Queue()
        session._clients = {}
        session._max_client_buffer_bytes = 0
        session._terminate_process = lambda: None
        session._notify_wake = lambda: None
        session._reader_thread = _SlowReaderThread()
        session._thread = _SlowLoopThread()

        PtySession.stop(session)

        self.assertEqual(session.tail_output(max_bytes=64), b"late final output")
        self.assertTrue(session._output_q.empty())

    def test_start_actor_finalizes_session_that_exits_before_registration(self) -> None:
        from cccc.runners.pty_snapshot import MAX_EXIT_SNAPSHOT_BYTES
        from cccc.runners.pty_win import PtySupervisor

        class _FastExitSession:
            def __init__(self, *, group_id: str, actor_id: str, on_exit, **_kwargs) -> None:
                self.group_id = group_id
                self.actor_id = actor_id
                self._data = b"discarded" + (b"x" * MAX_EXIT_SNAPSHOT_BYTES)
                self._exit_thread = threading.Thread(target=on_exit, args=(self,))
                self._exit_thread.start()
                self._exit_thread.join(timeout=0.2)

            def is_running(self) -> bool:
                return False

            def _backlog_snapshot(self):
                return self._data, 0, len(self._data)

            def returncode(self):
                return 1

            def tail_output(self, *, max_bytes: int) -> bytes:
                return self._data[-max_bytes:]

        supervisor = PtySupervisor()
        with patch("cccc.runners.pty_win.PtySession", _FastExitSession):
            session = supervisor.start_actor(
                group_id="g1",
                actor_id="a1",
                cwd=Path.cwd(),
                command=["cmd.exe", "/c", "exit", "1"],
                env={},
            )
        session._exit_thread.join(timeout=1.0)

        self.assertFalse(session._exit_thread.is_alive())
        self.assertNotIn(("g1", "a1"), supervisor._sessions)
        self.assertEqual(
            supervisor.tail_output(
                group_id="g1",
                actor_id="a1",
                max_bytes=MAX_EXIT_SNAPSHOT_BYTES + len(b"discarded"),
            ),
            b"x" * MAX_EXIT_SNAPSHOT_BYTES,
        )

    def test_supervisor_stop_methods_snapshot_output_produced_during_stop(self) -> None:
        from cccc.runners.pty_win import PtySupervisor

        class _OutputDuringStopSession:
            def __init__(self, supervisor: PtySupervisor, actor_id: str) -> None:
                self.group_id = "g1"
                self.actor_id = actor_id
                self._supervisor = supervisor
                self._data = b"before stop\n"

            def is_running(self) -> bool:
                return True

            def stop(self) -> None:
                self._data += b"during stop\n"
                self._supervisor._on_session_exit(self)
                self._data += b"after exit callback\n"

            def _backlog_snapshot(self):
                return self._data, 0, len(self._data)

            def returncode(self):
                return 0

        cases = (
            ("stop_actor", {"group_id": "g1", "actor_id": "a1"}),
            ("stop_group", {"group_id": "g1"}),
            ("stop_all", {}),
        )
        for index, (method_name, kwargs) in enumerate(cases, start=1):
            with self.subTest(method=method_name):
                supervisor = PtySupervisor()
                session = _OutputDuringStopSession(supervisor, f"a{index}")
                if method_name == "stop_actor":
                    kwargs = {"group_id": "g1", "actor_id": session.actor_id}
                with supervisor._lock:
                    supervisor._sessions[(session.group_id, session.actor_id)] = session

                getattr(supervisor, method_name)(**kwargs)

                self.assertEqual(
                    supervisor.tail_output(group_id="g1", actor_id=session.actor_id, max_bytes=200),
                    b"before stop\nduring stop\nafter exit callback\n",
                )


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
