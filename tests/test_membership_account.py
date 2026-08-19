from __future__ import annotations

import json
import unittest
from typing import Any, Dict, Optional
from unittest.mock import patch

from cccc.kernel.membership_account import (
    MAX_RESPONSE_BYTES,
    VERSION_HEADER,
    AccountError,
    default_transport,
    fetch_device,
    issue_reach,
    poll_device_login,
    start_device_login,
)


class FakeAccount:
    def __init__(self) -> None:
        self.approved = False
        self.disabled = False
        self.force_unsupported = False
        self.calls: list[tuple[str, str]] = []
        self.payloads: list[Dict[str, Any]] = []

    def __call__(
        self,
        method: str,
        url: str,
        headers: Dict[str, str],
        body: Optional[bytes],
        timeout_s: float,
    ) -> tuple[int, Dict[str, Any]]:
        self.calls.append((method, url))
        self.payloads.append(json.loads(body.decode("utf-8")) if body else {})
        if self.force_unsupported or headers.get(VERSION_HEADER) != "1":
            return 426, {
                "error": {
                    "code": "unsupported_version",
                    "message": "please upgrade CCCC",
                }
            }
        if url.endswith("/v1/device/code"):
            return 200, {
                "device_code": "dc-1",
                "user_code": "WDJB-MJHT",
                "verification_uri": "https://account.test/device",
                "expires_in": 600,
                "interval": 1,
            }
        if url.endswith("/v1/device/token"):
            if not self.approved:
                return 400, {"error": "authorization_pending"}
            return 200, {
                "access_token": "devtok",
                "device_id": "d_abc",
                "hostname": "https://d-abc.example.test",
            }
        if url.endswith("/v1/reach"):
            if self.disabled:
                return 403, {
                    "error": {"code": "disabled", "message": "device disabled"}
                }
            return 200, {
                "hostname": "https://d-abc.example.test",
                "tunnel_token": "tun-1",
            }
        if url.endswith("/v1/device"):
            return 200, {
                "device_id": "d_abc",
                "hostname": "https://d-abc.example.test",
                "disabled": self.disabled,
                "online": True,
            }
        return 404, {}


class TestMembershipAccountClient(unittest.TestCase):
    def test_default_transport_rejects_oversized_responses(self) -> None:
        class OversizedResponse:
            status = 200

            def __enter__(self):
                return self

            def __exit__(self, *_args):
                return False

            @staticmethod
            def read(_limit: int) -> bytes:
                return b"x" * (MAX_RESPONSE_BYTES + 1)

        with patch(
            "cccc.kernel.membership_account._open_no_redirect",
            return_value=OversizedResponse(),
        ):
            with self.assertRaisesRegex(AccountError, "exceeded size limit"):
                default_transport("GET", "https://account.test/v1/device", {}, None, 1)

    def test_redirects_are_rejected_as_account_errors(self) -> None:
        def transport(method, url, headers, body, timeout_s):
            _ = method, url, headers, body, timeout_s
            return 302, {}

        with self.assertRaises(AccountError) as redirected:
            start_device_login("https://account.test", transport=transport)
        self.assertEqual(redirected.exception.code, "membership_network")

    def test_start_device_login_requires_complete_payload(self) -> None:
        started = start_device_login("https://account.test", transport=FakeAccount())
        self.assertEqual(started["user_code"], "WDJB-MJHT")
        self.assertEqual(started["verification_uri"], "https://account.test/device")

    def test_start_device_login_honors_long_server_polling_interval(self) -> None:
        def transport(method, url, headers, body, timeout_s):
            _ = method, url, headers, body, timeout_s
            return 200, {
                "device_code": "dc-1",
                "user_code": "WDJB-MJHT",
                "verification_uri": "https://account.test/device",
                "expires_in": 600,
                "interval": 120,
            }

        self.assertEqual(
            start_device_login("https://account.test", transport=transport)["interval"],
            120,
        )

    def test_poll_pending_then_granted(self) -> None:
        account = FakeAccount()
        with self.assertRaises(AccountError) as pending:
            poll_device_login("https://account.test", "dc-1", transport=account)
        self.assertTrue(pending.exception.retryable)
        account.approved = True
        grant = poll_device_login("https://account.test", "dc-1", transport=account)
        self.assertEqual(grant["device_id"], "d_abc")
        self.assertEqual(grant["device_token"], "devtok")

    def test_slow_down_requests_five_second_backoff(self) -> None:
        def transport(method, url, headers, body, timeout_s):
            _ = method, url, headers, body, timeout_s
            return 400, {"error": {"code": "slow_down", "message": "slow_down"}}

        with self.assertRaises(AccountError) as slowed:
            poll_device_login("https://account.test", "dc-1", transport=transport)
        self.assertTrue(slowed.exception.retryable)
        self.assertEqual(slowed.exception.retry_after_delta, 5)

    def test_unsupported_version_is_stable_class(self) -> None:
        account = FakeAccount()
        account.force_unsupported = True
        with self.assertRaises(AccountError) as ctx:
            start_device_login("https://account.test", transport=account)
        self.assertEqual(ctx.exception.code, "membership_unsupported_version")

    def test_disabled_device_is_stable_class(self) -> None:
        account = FakeAccount()
        account.disabled = True
        with self.assertRaises(AccountError) as ctx:
            issue_reach("https://account.test", "devtok", transport=account)
        self.assertEqual(ctx.exception.code, "membership_disabled")

    def test_issue_reach_and_fetch_device(self) -> None:
        account = FakeAccount()
        creds = issue_reach(
            "https://account.test", "devtok", origin_port=9000, transport=account
        )
        self.assertEqual(creds["hostname"], "https://d-abc.example.test")
        self.assertIn({"origin_port": 9000}, account.payloads)
        device = fetch_device("https://account.test", "devtok", transport=account)
        self.assertFalse(device["disabled"])

    def test_rejects_non_url_origin(self) -> None:
        with self.assertRaises(AccountError) as ctx:
            start_device_login("not-a-url")
        self.assertEqual(ctx.exception.code, "membership_unavailable")

    def test_rejects_plain_http_except_for_loopback_development(self) -> None:
        with self.assertRaises(AccountError) as ctx:
            start_device_login("http://account.example.test")
        self.assertEqual(ctx.exception.code, "membership_unavailable")

        started = start_device_login("http://127.0.0.1:8787", transport=FakeAccount())
        self.assertEqual(started["user_code"], "WDJB-MJHT")


if __name__ == "__main__":
    unittest.main()
