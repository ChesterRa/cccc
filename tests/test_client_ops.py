import tempfile
import unittest
from pathlib import Path

from cccc.daemon.client_ops import send_daemon_request


class TestClientOps(unittest.TestCase):
    def test_invalid_tcp_endpoint_raises(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            with self.assertRaises(RuntimeError):
                send_daemon_request(
                    {"transport": "tcp", "host": "127.0.0.1", "port": 0},
                    {"op": "ping", "args": {}},
                    timeout_s=0.1,
                    sock_path_default=Path(td) / "ccccd.sock",
                )


if __name__ == "__main__":
    unittest.main()
