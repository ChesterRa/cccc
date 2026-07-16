from __future__ import annotations

import errno
import io
import os
import socket
import tempfile
import unittest
from unittest.mock import patch


class _DeniedBindSocket:
    def bind(self, _sockaddr) -> None:
        exc = OSError(errno.EACCES, "forbidden")
        exc.winerror = 10013  # type: ignore[attr-defined]
        raise exc

    def setsockopt(self, *args) -> None:
        pass

    def close(self) -> None:
        return None


class _AddrInUseSocket:
    def bind(self, _sockaddr) -> None:
        exc = OSError(errno.EADDRINUSE, "in use")
        exc.winerror = 10048  # type: ignore[attr-defined]
        raise exc

    def setsockopt(self, *args) -> None:
        pass

    def close(self) -> None:
        return None


class _CaptureSocket:
    def __init__(self) -> None:
        self.setsockopt_calls: list[tuple[int, int, int]] = []
        self.bound = False

    def setsockopt(self, level: int, optname: int, value: int) -> None:
        self.setsockopt_calls.append((level, optname, value))

    def bind(self, _sockaddr) -> None:
        self.bound = True

    def close(self) -> None:
        return None


class TestWebBindPreflight(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def test_in_use_port_reports_clear_message(self) -> None:
        from cccc.ports.web.bind_preflight import ensure_tcp_port_bindable

        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        listener.bind(("127.0.0.1", 0))
        port = int(listener.getsockname()[1])
        try:
            with self.assertRaises(RuntimeError) as ctx:
                ensure_tcp_port_bindable(host="127.0.0.1", port=port)
        finally:
            listener.close()

        message = str(ctx.exception)
        self.assertIn(f"Web port {port} is unavailable", message)
        self.assertIn("already using that port", message)
        self.assertIn("cccc --port 9000", message)
        self.assertIn("CCCC_WEB_PORT=9000 cccc", message)

    def test_windows_access_denied_points_to_excluded_port_ranges(self) -> None:
        from cccc.ports.web import bind_preflight

        with patch.object(bind_preflight.sys, "platform", "win32"), patch.object(
            bind_preflight.socket,
            "getaddrinfo",
            return_value=[(socket.AF_INET, socket.SOCK_STREAM, 0, "", ("127.0.0.1", 8848))],
        ), patch.object(bind_preflight.socket, "socket", return_value=_DeniedBindSocket()):
            with self.assertRaises(RuntimeError) as ctx:
                bind_preflight.ensure_tcp_port_bindable(host="127.0.0.1", port=8848)

        message = str(ctx.exception)
        self.assertIn("excluded TCP port range", message)
        self.assertIn("netsh interface ipv4 show excludedportrange protocol=tcp", message)
        self.assertIn("cccc --port 9000", message)
        self.assertIn("$env:CCCC_WEB_PORT=9000; cccc", message)

    def test_windows_addr_in_use_without_listener_points_to_excluded_port_ranges(self) -> None:
        from cccc.ports.web import bind_preflight

        with patch.object(bind_preflight.sys, "platform", "win32"), patch.object(
            bind_preflight.socket,
            "getaddrinfo",
            return_value=[(socket.AF_INET, socket.SOCK_STREAM, 0, "", ("127.0.0.1", 8848))],
        ), patch.object(bind_preflight, "_listener_detected", return_value=False), patch.object(
            bind_preflight.socket, "socket", return_value=_AddrInUseSocket()
        ):
            with self.assertRaises(RuntimeError) as ctx:
                bind_preflight.ensure_tcp_port_bindable(host="127.0.0.1", port=8848)

        message = str(ctx.exception)
        self.assertIn("no TCP listener was detected", message)
        self.assertIn("excluded TCP port range", message)
        self.assertNotIn("Another process is already using that port", message)

    def test_posix_preflight_sets_reuseaddr(self) -> None:
        from cccc.ports.web import bind_preflight

        fake_socket = _CaptureSocket()
        with patch.object(bind_preflight.sys, "platform", "linux"), patch.object(
            bind_preflight.socket,
            "getaddrinfo",
            return_value=[(socket.AF_INET, socket.SOCK_STREAM, 0, "", ("127.0.0.1", 8848))],
        ), patch.object(bind_preflight.socket, "socket", return_value=fake_socket):
            bind_preflight.ensure_tcp_port_bindable(host="127.0.0.1", port=8848)

        self.assertTrue(fake_socket.bound)
        self.assertIn((socket.SOL_SOCKET, socket.SO_REUSEADDR, 1), fake_socket.setsockopt_calls)

    def test_web_main_returns_error_when_preflight_fails(self) -> None:
        from cccc.ports.web import main as web_main

        stderr = io.StringIO()
        with patch.object(web_main, "_check_daemon_running", return_value=True), patch.object(
            web_main,
            "start_supervised_web_child",
            return_value=(None, "boom"),
        ), patch.object(web_main.sys, "stderr", stderr):
            rc = web_main.main(["--host", "127.0.0.1", "--port", "8848"])

        self.assertEqual(rc, 1)
        self.assertIn("boom", stderr.getvalue())

    def test_web_main_uses_fast_shutdown_timeout(self) -> None:
        from cccc.ports.web import main as web_main

        server_instance = unittest.mock.Mock()
        with patch.object(web_main.uvicorn, "Config") as mock_config, patch.object(
            web_main.uvicorn, "Server",
            return_value=server_instance,
        ) as mock_server:
            rc = web_main.main(["--serve-child", "--host", "127.0.0.1", "--port", "8848"])

        self.assertEqual(rc, 0)
        mock_config.assert_called_once()
        self.assertEqual(mock_config.call_args.kwargs.get("timeout_graceful_shutdown"), 0.2)
        mock_server.assert_called_once()
        server_instance.run.assert_called_once_with()

    def test_web_main_uses_shared_binding_defaults(self) -> None:
        from cccc.kernel.settings import update_remote_access_settings
        from cccc.ports.web import main as web_main

        _, cleanup = self._with_home()
        proc = unittest.mock.Mock()
        proc.poll.return_value = 0
        try:
            update_remote_access_settings({"web_host": "127.0.0.1", "web_port": 9001})
            with patch.object(web_main, "_check_daemon_running", return_value=True), patch.object(
                web_main,
                "start_supervised_web_child",
                return_value=(proc, None),
            ) as mock_start, patch.object(
                web_main,
                "_print_web_banner",
            ):
                rc = web_main.main([])
        finally:
            cleanup()

        self.assertEqual(rc, 0)
        self.assertEqual(mock_start.call_args.kwargs.get("host"), "127.0.0.1")
        self.assertEqual(mock_start.call_args.kwargs.get("port"), 9001)
        proc.poll.assert_called()


if __name__ == "__main__":
    unittest.main()
