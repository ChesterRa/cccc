import unittest
from unittest.mock import patch

from cccc.daemon.client_ops import DaemonClientError
from cccc.daemon import server


class TestDaemonTransportDiagnostics(unittest.TestCase):
    def setUp(self) -> None:
        server._DAEMON_CLIENT_WARN_SEEN.clear()

    def test_call_daemon_preserves_transport_details(self) -> None:
        err = DaemonClientError(
            phase="read",
            reason="timeout",
            transport="unix",
            endpoint={"transport": "unix", "path": "/tmp/ccccd.sock"},
            op="ping",
            timeout_s=0.5,
            cause=TimeoutError("timed out"),
        )
        with patch.object(server, "send_daemon_request", side_effect=err), patch.object(
            server.logger, "warning"
        ) as mock_warning, patch.object(server.logger, "debug") as mock_debug:
            resp = server.call_daemon({"op": "ping"})

        self.assertFalse(resp["ok"])
        self.assertEqual(resp["error"]["code"], "daemon_unavailable")
        self.assertEqual(resp["error"]["details"]["phase"], "read")
        self.assertEqual(resp["error"]["details"]["reason"], "timeout")
        self.assertEqual(resp["error"]["details"]["transport"], "unix")
        self.assertEqual(resp["error"]["details"]["op"], "ping")
        mock_warning.assert_not_called()
        mock_debug.assert_called_once()

    def test_call_daemon_keeps_warning_for_business_ops(self) -> None:
        err = DaemonClientError(
            phase="read",
            reason="timeout",
            transport="unix",
            endpoint={"transport": "unix", "path": "/tmp/ccccd.sock"},
            op="group_show",
            timeout_s=0.5,
            cause=TimeoutError("timed out"),
        )
        with patch.object(server, "send_daemon_request", side_effect=err), patch.object(
            server.logger, "warning"
        ) as mock_warning, patch.object(server.logger, "debug") as mock_debug:
            resp = server.call_daemon({"op": "group_show", "args": {"group_id": "g_demo"}})

        self.assertFalse(resp["ok"])
        self.assertEqual(resp["error"]["code"], "daemon_unavailable")
        self.assertEqual(resp["error"]["details"]["op"], "group_show")
        mock_warning.assert_called_once()
        mock_debug.assert_not_called()

    def test_call_daemon_throttles_repeated_business_warning(self) -> None:
        err = DaemonClientError(
            phase="read",
            reason="timeout",
            transport="unix",
            endpoint={"transport": "unix", "path": "/tmp/ccccd.sock"},
            op="group_show",
            timeout_s=0.5,
            cause=TimeoutError("timed out"),
        )
        with patch.object(server, "send_daemon_request", side_effect=[err, err]), patch.object(
            server.logger, "warning"
        ) as mock_warning, patch.object(server, "_DAEMON_CLIENT_WARNING_WINDOW_S", 60.0):
            resp1 = server.call_daemon({"op": "group_show", "args": {"group_id": "g_demo"}})
            resp2 = server.call_daemon({"op": "group_show", "args": {"group_id": "g_demo"}})

        self.assertFalse(resp1["ok"])
        self.assertFalse(resp2["ok"])
        mock_warning.assert_called_once()


if __name__ == "__main__":
    unittest.main()
