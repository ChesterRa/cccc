import sys
import threading
import time
import unittest
from collections import deque
from pathlib import Path
from unittest.mock import patch


class TestPtyHistoryPage(unittest.TestCase):
    def _session(self):
        from cccc.runners import pty as pty_runner

        session = pty_runner.PtySession.__new__(pty_runner.PtySession)
        session.group_id = "g1"
        session.actor_id = "a1"
        session._runtime = "codex"
        session._lock = threading.Lock()
        session._backlog = deque()
        session._backlog_bytes = 0
        session._max_backlog_bytes = 10
        session._first_output_at = None
        session._last_output_at = None
        session._terminal_signal_buffer = ""
        session._terminal_override = None
        session._mode_tail = b""
        session._query_tail = b""
        session._bracketed_paste = False
        session._bracketed_paste_changed_at = None
        return session

    def test_history_page_uses_absolute_byte_cursors(self) -> None:
        session = self._session()
        session._append_backlog(b"abcde")
        session._append_backlog(b"fghij")

        page = session.history_page(limit_bytes=4)

        self.assertEqual(page["data"], b"ghij")
        self.assertEqual(page["start_cursor"], 6)
        self.assertEqual(page["end_cursor"], 10)
        self.assertEqual(page["has_more"], True)
        self.assertEqual(page["cursor_expired"], False)

    def test_history_page_before_cursor_returns_older_slice(self) -> None:
        session = self._session()
        session._append_backlog(b"abcde")
        session._append_backlog(b"fghij")

        page = session.history_page(before=6, limit_bytes=3)

        self.assertEqual(page["data"], b"def")
        self.assertEqual(page["start_cursor"], 3)
        self.assertEqual(page["end_cursor"], 6)
        self.assertEqual(page["has_more"], True)

    def test_history_page_reports_expired_cursor_after_backlog_drop(self) -> None:
        session = self._session()
        session._append_backlog(b"abcde")
        session._append_backlog(b"fghij")
        session._append_backlog(b"klmno")

        page = session.history_page(before=4, limit_bytes=3)

        self.assertEqual(page["data"], b"")
        self.assertEqual(page["start_cursor"], 5)
        self.assertEqual(page["end_cursor"], 5)
        self.assertEqual(page["has_more"], False)
        self.assertEqual(page["cursor_expired"], True)

    def test_history_since_returns_backlog_after_cursor_without_duplicates(self) -> None:
        session = self._session()
        session._append_backlog(b"abcde")
        session._append_backlog(b"fghij")

        self.assertEqual(session.history_since(5), b"fghij")
        self.assertEqual(session.history_since(10), b"")

    def test_supervisor_keeps_tail_output_after_session_exit(self) -> None:
        from cccc.runners import pty as pty_runner

        supervisor = pty_runner.PtySupervisor()
        session = self._session()

        with supervisor._lock:
            supervisor._sessions[(session.group_id, session.actor_id)] = session
        session._append_backlog(b"failed\n")

        supervisor._on_session_exit(session)

        self.assertFalse(supervisor.actor_running("g1", "a1"))
        self.assertEqual(supervisor.tail_output(group_id="g1", actor_id="a1", max_bytes=200), b"failed\n")

    def test_supervisor_keeps_exit_code_when_stopped_session_has_no_output(self) -> None:
        from cccc.runners import pty as pty_runner

        class FakeProc:
            def poll(self) -> int:
                return 7

        supervisor = pty_runner.PtySupervisor()
        session = self._session()
        session._proc = FakeProc()

        with supervisor._lock:
            supervisor._sessions[(session.group_id, session.actor_id)] = session

        supervisor._on_session_exit(session)

        output = supervisor.tail_output(group_id="g1", actor_id="a1", max_bytes=200).decode("utf-8")
        self.assertIn("Process exited with code 7", output)

    def test_supervisor_limits_each_exit_snapshot_to_256_kib_tail(self) -> None:
        from cccc.runners import pty as pty_runner

        snapshot_limit = 256 * 1024
        supervisor = pty_runner.PtySupervisor()
        session = self._session()
        retained_tail = b"x" * snapshot_limit
        output = b"discarded-prefix" + retained_tail
        session._max_backlog_bytes = len(output) + 1

        with supervisor._lock:
            supervisor._sessions[(session.group_id, session.actor_id)] = session
        session._append_backlog(output)

        supervisor._on_session_exit(session)

        self.assertEqual(
            supervisor.tail_output(group_id="g1", actor_id="a1", max_bytes=len(output)),
            retained_tail,
        )
        expired_page = supervisor.history_page(
            group_id="g1",
            actor_id="a1",
            before=len(b"discarded-prefix") - 1,
            limit_bytes=100,
        )
        self.assertEqual(expired_page["start_cursor"], len(b"discarded-prefix"))
        self.assertTrue(expired_page["cursor_expired"])

    def test_supervisor_limits_exit_snapshot_cache_to_8_mib(self) -> None:
        from cccc.runners import pty as pty_runner

        snapshot_size = 256 * 1024
        supervisor = pty_runner.PtySupervisor()
        output = b"x" * snapshot_size

        for index in range(33):
            session = self._session()
            session.actor_id = f"a{index}"
            session._max_backlog_bytes = snapshot_size + 1
            with supervisor._lock:
                supervisor._sessions[(session.group_id, session.actor_id)] = session
            session._append_backlog(output)
            supervisor._on_session_exit(session)

        self.assertEqual(supervisor.tail_output(group_id="g1", actor_id="a0", max_bytes=1), b"")
        self.assertEqual(supervisor.tail_output(group_id="g1", actor_id="a1", max_bytes=1), b"x")
        self.assertEqual(supervisor.tail_output(group_id="g1", actor_id="a32", max_bytes=1), b"x")

    def _assert_stop_keeps_final_output(self, stop) -> None:
        from cccc.runners import pty as pty_runner

        supervisor = pty_runner.PtySupervisor()
        session = self._session()
        session._max_backlog_bytes = 1_000
        session._append_backlog(b"before stop\n")

        def stop_with_exit_callback() -> None:
            session._append_backlog(b"during stop\n")
            supervisor._on_session_exit(session)
            session._append_backlog(b"after exit callback\n")

        session.stop = stop_with_exit_callback
        with supervisor._lock:
            supervisor._sessions[(session.group_id, session.actor_id)] = session

        stop(supervisor, session)

        self.assertEqual(
            supervisor.tail_output(group_id=session.group_id, actor_id=session.actor_id, max_bytes=1_000),
            b"before stop\nduring stop\nafter exit callback\n",
        )

    def test_stop_actor_snapshots_after_stop_and_exit_callback(self) -> None:
        self._assert_stop_keeps_final_output(
            lambda supervisor, session: supervisor.stop_actor(
                group_id=session.group_id,
                actor_id=session.actor_id,
            )
        )

    def test_stop_group_snapshots_after_stop_and_exit_callback(self) -> None:
        self._assert_stop_keeps_final_output(
            lambda supervisor, session: supervisor.stop_group(group_id=session.group_id)
        )

    def test_stop_all_snapshots_after_stop_and_exit_callback(self) -> None:
        self._assert_stop_keeps_final_output(lambda supervisor, session: supervisor.stop_all())

    def test_session_stop_waits_for_reader_thread_to_drain_output(self) -> None:
        from cccc.runners import pty as pty_runner

        class ExitedProc:
            pid = 123

            def poll(self) -> int:
                return 0

        session = self._session()
        session._max_backlog_bytes = 1_000
        session._proc = ExitedProc()
        session._master_fd = -1
        session._running = True

        class ReaderThread:
            def join(self, timeout=None) -> None:
                session._append_backlog(b"final reader output\n")

            def is_alive(self) -> bool:
                return False

        session._thread = ReaderThread()

        with patch.object(pty_runner, "_best_effort_killpg"):
            session.stop()

        self.assertEqual(session.tail_output(max_bytes=1_000), b"final reader output\n")

    def test_stop_actor_drains_sigterm_output_before_snapshot(self) -> None:
        from cccc.runners import pty as pty_runner

        supervisor = pty_runner.PtySupervisor()
        script = (
            "import os, signal\n"
            "def stop(_signum, _frame):\n"
            "    os.write(1, b'termination marker\\n')\n"
            "    os._exit(0)\n"
            "signal.signal(signal.SIGTERM, stop)\n"
            "os.write(1, b'ready\\n')\n"
            "signal.pause()\n"
        )
        session = supervisor.start_actor(
            group_id="g-real",
            actor_id="a-real",
            cwd=Path.cwd(),
            command=[sys.executable, "-u", "-c", script],
            env={},
            max_backlog_bytes=10_000,
        )
        deadline = time.monotonic() + 2.0
        while time.monotonic() < deadline:
            if b"ready" in session.tail_output(max_bytes=1_000):
                break
            time.sleep(0.01)
        self.assertIn(b"ready", session.tail_output(max_bytes=1_000))

        supervisor.stop_actor(group_id="g-real", actor_id="a-real")

        self.assertIn(
            b"termination marker",
            supervisor.tail_output(group_id="g-real", actor_id="a-real", max_bytes=1_000),
        )

    def test_start_actor_registers_before_exit_callback_enters_blocking_hook(self) -> None:
        from cccc.runners import pty as pty_runner

        hook_entered = threading.Event()
        release_hook = threading.Event()

        class AliveReader:
            def join(self, timeout=None) -> None:
                pass

            def is_alive(self) -> bool:
                return True

        class ImmediatelyExitedSession:
            def __init__(self, **kwargs) -> None:
                self.group_id = kwargs["group_id"]
                self.actor_id = kwargs["actor_id"]
                self._thread = AliveReader()
                self._exit_thread = threading.Thread(target=kwargs["on_exit"], args=(self,))
                self._exit_thread.start()
                hook_entered.wait(timeout=0.1)

            def is_running(self) -> bool:
                return False

            def _backlog_snapshot(self):
                return b"fast exit\n", 0, len(b"fast exit\n")

            def returncode(self) -> int:
                return 1

        supervisor = pty_runner.PtySupervisor()

        def blocking_exit_hook(_exited_session) -> None:
            hook_entered.set()
            release_hook.wait(timeout=2.0)

        supervisor.set_exit_hook(blocking_exit_hook)
        with patch.object(pty_runner, "PtySession", ImmediatelyExitedSession):
            session = supervisor.start_actor(
                group_id="g-fast",
                actor_id="a-fast",
                cwd=Path.cwd(),
                command=["unused"],
                env={},
            )

        try:
            self.assertTrue(hook_entered.wait(timeout=1.0))
            self.assertFalse(supervisor.actor_running("g-fast", "a-fast"))
            self.assertEqual(
                supervisor.tail_output(group_id="g-fast", actor_id="a-fast", max_bytes=1_000),
                b"fast exit\n",
            )
            with supervisor._lock:
                self.assertNotIn(("g-fast", "a-fast"), supervisor._sessions)
        finally:
            release_hook.set()
            session._exit_thread.join(timeout=1.0)


if __name__ == "__main__":
    unittest.main()
