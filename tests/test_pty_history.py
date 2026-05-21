import threading
import unittest
from collections import deque


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


if __name__ == "__main__":
    unittest.main()
