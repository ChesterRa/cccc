import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class _UrlopenResponse:
    def __init__(self, status: int, body: bytes = b""):
        self.status = status
        self._body = body

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self):
        body, self._body = self._body, b""
        return body


class TestWebRuntimeControl(unittest.TestCase):
    def test_wait_for_web_ready_retries_after_oserror(self) -> None:
        from cccc.ports.web.runtime_control import wait_for_web_ready

        with patch(
            "cccc.ports.web.runtime_control.urllib.request.urlopen",
            side_effect=[ConnectionResetError("not ready"), _UrlopenResponse(200)],
        ) as mock_urlopen, patch("cccc.ports.web.runtime_control.time.sleep") as mock_sleep:
            ready = wait_for_web_ready(host="127.0.0.1", port=8848, timeout_s=0.2)

        self.assertTrue(ready)
        self.assertEqual(mock_urlopen.call_count, 2)
        mock_sleep.assert_called_once_with(0.1)

    def test_wait_for_web_ready_retries_after_http_protocol_error(self) -> None:
        from cccc.ports.web.runtime_control import wait_for_web_ready
        import http.client

        with patch(
            "cccc.ports.web.runtime_control.urllib.request.urlopen",
            side_effect=[http.client.RemoteDisconnected("not ready"), _UrlopenResponse(200)],
        ) as mock_urlopen, patch("cccc.ports.web.runtime_control.time.sleep") as mock_sleep:
            ready = wait_for_web_ready(host="127.0.0.1", port=8848, timeout_s=0.2)

        self.assertTrue(ready)
        self.assertEqual(mock_urlopen.call_count, 2)
        mock_sleep.assert_called_once_with(0.1)

    def test_wait_for_web_ready_uses_web_ready_endpoint_for_wildcard_host(self) -> None:
        from cccc.ports.web.runtime_control import wait_for_web_ready

        with patch(
            "cccc.ports.web.runtime_control.urllib.request.urlopen",
            return_value=_UrlopenResponse(200),
        ) as mock_urlopen:
            ready = wait_for_web_ready(host="0.0.0.0", port=8848, timeout_s=0.2)

        self.assertTrue(ready)
        self.assertEqual(mock_urlopen.call_args.args[0], "http://127.0.0.1:8848/api/v1/ready")

    def test_wait_for_web_ready_requires_the_expected_runtime_identity(self) -> None:
        from cccc.ports.web.runtime_control import wait_for_web_ready

        body = b'{"ok":true,"result":{"web":"ready","runtime_id":"web_other"}}'
        with patch(
            "cccc.ports.web.runtime_control.urllib.request.urlopen",
            return_value=_UrlopenResponse(200, body),
        ), patch("cccc.ports.web.runtime_control.time.sleep"):
            ready = wait_for_web_ready(
                host="127.0.0.1",
                port=8848,
                timeout_s=0.01,
                expected_runtime_id="web_expected",
            )

        self.assertFalse(ready)

    def test_web_runtime_pid_candidates_prefers_launcher_pid(self) -> None:
        from cccc.ports.web import runtime_control

        candidates = runtime_control.web_runtime_pid_candidates({"pid": 4321, "launcher_pid": 9876})

        self.assertEqual(candidates, [9876, 4321])

    def test_clear_web_runtime_state_accepts_launcher_pid(self) -> None:
        from cccc.ports.web import runtime_control

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            runtime_control.write_web_runtime_state(
                home=home,
                pid=4321,
                host="127.0.0.1",
                port=8848,
                mode="normal",
                supervisor_managed=True,
                supervisor_pid=1111,
                launcher_pid=9876,
                launch_source="test",
            )

            runtime_control.clear_web_runtime_state(home=home, pid=9876)

            self.assertFalse(runtime_control.web_runtime_state_path(home).exists())


if __name__ == "__main__":
    unittest.main()
