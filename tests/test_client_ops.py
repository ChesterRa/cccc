import socket
import tempfile
import threading
import time
import unittest
from pathlib import Path

from cccc.daemon.client_ops import DaemonClientError, send_daemon_request


class TestClientOps(unittest.TestCase):
    def test_invalid_tcp_endpoint_raises(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            with self.assertRaises(DaemonClientError) as ctx:
                send_daemon_request(
                    {"transport": "tcp", "host": "127.0.0.1", "port": 0},
                    {"op": "ping", "args": {}},
                    timeout_s=0.1,
                    sock_path_default=Path(td) / "ccccd.sock",
                )
        self.assertEqual(ctx.exception.phase, "endpoint")
        self.assertEqual(ctx.exception.reason, "invalid_endpoint")
        self.assertEqual(ctx.exception.transport, "tcp")

    def test_missing_unix_socket_raises_connect_phase(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            with self.assertRaises(DaemonClientError) as ctx:
                send_daemon_request(
                    {"transport": "unix", "path": str(Path(td) / "missing.sock")},
                    {"op": "ping", "args": {}},
                    timeout_s=0.1,
                    sock_path_default=Path(td) / "ccccd.sock",
                )
        self.assertEqual(ctx.exception.phase, "connect")
        self.assertEqual(ctx.exception.transport, "unix")

    def test_unix_read_eof_raises_read_phase(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            sock_path = Path(td) / "ccccd.sock"
            thread = self._start_unix_server(
                sock_path,
                lambda conn: conn.recv(4096),
            )
            try:
                with self.assertRaises(DaemonClientError) as ctx:
                    send_daemon_request(
                        {"transport": "unix", "path": str(sock_path)},
                        {"op": "ping", "args": {}},
                        timeout_s=0.2,
                        sock_path_default=sock_path,
                    )
            finally:
                thread.join(timeout=1)
        self.assertEqual(ctx.exception.phase, "read")
        self.assertEqual(ctx.exception.reason, "eof")

    def test_unix_invalid_json_raises_decode_phase(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            sock_path = Path(td) / "ccccd.sock"
            thread = self._start_unix_server(
                sock_path,
                lambda conn: self._send_response(conn, b"not-json\n"),
            )
            try:
                with self.assertRaises(DaemonClientError) as ctx:
                    send_daemon_request(
                        {"transport": "unix", "path": str(sock_path)},
                        {"op": "ping", "args": {}},
                        timeout_s=0.2,
                        sock_path_default=sock_path,
                    )
            finally:
                thread.join(timeout=1)
        self.assertEqual(ctx.exception.phase, "decode")
        self.assertEqual(ctx.exception.reason, "invalid_json")

    def test_unix_read_timeout_raises_read_timeout(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            sock_path = Path(td) / "ccccd.sock"
            thread = self._start_unix_server(
                sock_path,
                lambda conn: self._sleep_without_reply(conn, 0.2),
            )
            try:
                with self.assertRaises(DaemonClientError) as ctx:
                    send_daemon_request(
                        {"transport": "unix", "path": str(sock_path)},
                        {"op": "ping", "args": {}},
                        timeout_s=0.05,
                        sock_path_default=sock_path,
                    )
            finally:
                thread.join(timeout=1)
        self.assertEqual(ctx.exception.phase, "read")
        self.assertEqual(ctx.exception.reason, "timeout")

    def _send_response(self, conn: socket.socket, payload: bytes) -> None:
        conn.recv(4096)
        conn.sendall(payload)

    def _sleep_without_reply(self, conn: socket.socket, seconds: float) -> None:
        conn.recv(4096)
        time.sleep(seconds)

    def _start_unix_server(self, sock_path: Path, handler) -> threading.Thread:
        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        server.bind(str(sock_path))
        server.listen(1)

        def _serve() -> None:
            try:
                conn, _ = server.accept()
                with conn:
                    handler(conn)
            finally:
                try:
                    server.close()
                except Exception:
                    pass

        thread = threading.Thread(target=_serve, daemon=True)
        thread.start()
        # The socket is already bound/listening, but a tiny delay avoids a
        # connect race on slower CI hosts.
        time.sleep(0.02)
        return thread


if __name__ == "__main__":
    unittest.main()
