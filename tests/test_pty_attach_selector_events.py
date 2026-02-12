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
    def test_non_writer_client_registers_with_read_event_even_without_backlog(self) -> None:
        from cccc.runners import pty as pty_runner

        session = pty_runner.PtySession.__new__(pty_runner.PtySession)
        session._lock = threading.Lock()
        session._clients = {}
        session._writer_fd = 999  # Simulate an existing writer so this attach is non-writer.
        session._backlog = deque()
        session._selector = _FakeSelector()

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
            try:
                peer_sock.close()
            except Exception:
                pass


if __name__ == "__main__":
    unittest.main()
