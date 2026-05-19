import selectors
import socket
import threading
import unittest
from collections import deque


class _FakeSelector:
    def __init__(self) -> None:
        self.register_calls = []

    def register(self, sock, events, data=None):
        self.register_calls.append((sock, events, data))

    def unregister(self, sock):
        return None

    def modify(self, sock, events, data=None):
        return None

    def get_key(self, sock):
        class _Key:
            events = selectors.EVENT_READ

        return _Key()


class TestPtyAttachSelectorEvents(unittest.TestCase):
    def _session(self):
        from cccc.runners import pty as pty_runner

        session = pty_runner.PtySession.__new__(pty_runner.PtySession)
        session.group_id = "g1"
        session.actor_id = "a1"
        session._runtime = "codex"
        session._lock = threading.Lock()
        session._clients = {}
        session._writer_fd = None
        session._backlog = deque()
        session._backlog_bytes = 0
        session._backlog_start_offset = 0
        session._backlog_end_offset = 0
        session._max_backlog_bytes = 1024
        session._first_output_at = None
        session._last_output_at = None
        session._terminal_signal_buffer = ""
        session._terminal_override = None
        session._mode_tail = b""
        session._query_tail = b""
        session._bracketed_paste = False
        session._bracketed_paste_changed_at = None
        session._selector = _FakeSelector()
        return session

    def test_non_writer_client_registers_with_read_event_even_without_backlog(self) -> None:
        session = self._session()
        session._writer_fd = 999  # Simulate an existing writer so this attach is non-writer.

        client_sock, peer_sock = socket.socketpair()
        try:
            session._attach_client_now(client_sock)
            self.assertEqual(len(session._selector.register_calls), 1)
            _, events, _ = session._selector.register_calls[0]
            self.assertTrue(bool(events & selectors.EVENT_READ))
        finally:
            try:
                client_sock.close()
            except Exception:
                pass

    def test_attach_from_current_cursor_does_not_replay_old_tui_backlog(self) -> None:
        session = self._session()
        session._append_backlog(b"\x1b[?1049h\r\n\xe2\x80\xa2 Working (14h 17m 43s \xe2\x80\xa2 esc to interrupt)\r\n")
        cursor = session.history_page(limit_bytes=1)["end_cursor"]

        client_sock, peer_sock = socket.socketpair()
        try:
            session._attach_client_now(client_sock, since=cursor)
            fileno = client_sock.fileno()
            client = session._clients[fileno]
            self.assertEqual(bytes(client.outbuf), b"")
            _, events, _ = session._selector.register_calls[-1]
            self.assertFalse(bool(events & selectors.EVENT_WRITE))
        finally:
            try:
                client_sock.close()
            except Exception:
                pass
            try:
                peer_sock.close()
            except Exception:
                pass


if __name__ == "__main__":
    unittest.main()
