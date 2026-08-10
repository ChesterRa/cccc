import socket
import ssl
import unittest
from urllib.error import URLError

from cccc.kernel.group_bridge.pairing_remote_transport import (
    REMOTE_PAIRING_TIMEOUT_SECONDS,
    format_transport_error,
)
from cccc.kernel.group_bridge.pairing_remote import _safe_error, _safe_status_error


class TestGroupBridgePairingRemoteTransport(unittest.TestCase):
    def test_timeout_reports_the_production_budget(self) -> None:
        error = format_transport_error(
            "remote pairing request",
            URLError(TimeoutError("timed out")),
        )

        self.assertEqual(REMOTE_PAIRING_TIMEOUT_SECONDS, 15.0)
        self.assertEqual(
            error,
            "remote pairing request failed (timeout after 15s): timed out",
        )
        self.assertEqual(_safe_error(URLError(TimeoutError("timed out"))), error)
        self.assertEqual(
            _safe_status_error(URLError(TimeoutError("timed out"))),
            "remote pairing status failed (timeout after 15s): timed out",
        )

    def test_dns_tls_proxy_and_connect_failures_are_distinct(self) -> None:
        cases = [
            (URLError(socket.gaierror(-2, "Name or service not known")), "dns"),
            (URLError(ssl.SSLError("certificate verify failed")), "tls"),
            (URLError(OSError("proxy tunnel connection failed")), "proxy"),
            (URLError(ConnectionRefusedError(61, "Connection refused")), "connect"),
        ]

        for exc, category in cases:
            with self.subTest(category=category):
                error = format_transport_error("remote pairing request", exc)
                self.assertIsNotNone(error)
                self.assertIn(f"failed ({category})", error or "")

    def test_unknown_application_error_is_not_misreported_as_transport(self) -> None:
        self.assertIsNone(
            format_transport_error("remote pairing request", RuntimeError("application bug"))
        )


if __name__ == "__main__":
    unittest.main()
