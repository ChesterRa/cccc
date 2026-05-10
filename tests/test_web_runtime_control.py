import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class _PollingProc:
    def __init__(self, responses):
        self._responses = list(responses)

    def poll(self):
        if self._responses:
            return self._responses.pop(0)
        return None


class _UrlopenResponse:
    def __init__(self, status: int):
        self.status = status

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False


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

    def test_wait_for_child_exit_interruptibly_returns_exit_code(self) -> None:
        from cccc.ports.web.runtime_control import wait_for_child_exit_interruptibly

        proc = _PollingProc([None, None, 75])
        with patch("cccc.ports.web.runtime_control.time.sleep") as mock_sleep:
            ret = wait_for_child_exit_interruptibly(proc, poll_interval_s=0.01)

        self.assertEqual(ret, 75)
        self.assertEqual(mock_sleep.call_count, 2)

    def test_wait_for_child_exit_interruptibly_propagates_keyboard_interrupt(self) -> None:
        from cccc.ports.web.runtime_control import wait_for_child_exit_interruptibly

        proc = _PollingProc([None])
        with patch(
            "cccc.ports.web.runtime_control.time.sleep",
            side_effect=KeyboardInterrupt(),
        ):
            with self.assertRaises(KeyboardInterrupt):
                wait_for_child_exit_interruptibly(proc, poll_interval_s=0.01)

    def test_run_supervised_web_stops_child_on_keyboard_interrupt(self) -> None:
        from cccc.ports.web import main as web_main

        td_ctx = tempfile.TemporaryDirectory()
        home = Path(td_ctx.__enter__())
        try:
            proc = unittest.mock.Mock(pid=4321)
            with patch.object(web_main, "ensure_home", return_value=home), patch.object(
                web_main, "_check_daemon_running", return_value=True
            ), patch.object(web_main, "start_supervised_web_child", return_value=(proc, None)), patch.object(
                web_main, "wait_for_child_exit_interruptibly", side_effect=KeyboardInterrupt()
            ), patch.object(web_main, "stop_web_child", return_value=True) as mock_stop, patch.object(
                web_main, "clear_web_runtime_state"
            ) as mock_clear:
                ret = web_main._run_supervised_web(
                    host="127.0.0.1",
                    port=8848,
                    mode="normal",
                    reload=False,
                    log_level="info",
                    launch_source="test",
                )

            self.assertEqual(ret, 0)
            mock_stop.assert_called_once_with(proc, timeout_s=2.0)
            mock_clear.assert_called_once_with(home=home, pid=4321)
        finally:
            td_ctx.__exit__(None, None, None)

    def test_spawn_web_child_windows_uses_supervised_process_kwargs_and_log_file(self) -> None:
        from cccc.ports.web import runtime_control

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            fake_proc = object()
            with patch.object(runtime_control.os, "name", "nt"), patch.object(
                runtime_control,
                "resolve_background_python_argv",
                return_value=[r"D:\dev\cccc\.venv\Scripts\pythonw.exe", "-m", "cccc.ports.web.main", "--serve-child"],
            ) as mock_argv, patch.object(
                runtime_control,
                "supervised_process_popen_kwargs",
                return_value={"creationflags": 0x208},
            ), patch.object(
                runtime_control,
                "web_runtime_log_path",
                return_value=home / "daemon" / "cccc-web.log",
            ), patch.object(runtime_control.subprocess, "Popen", return_value=fake_proc) as mock_popen:
                proc = runtime_control.spawn_web_child(
                    home=home,
                    host="127.0.0.1",
                    port=8848,
                    mode="normal",
                    log_level="info",
                    reload=False,
                    launch_source="test",
                )

            self.assertIs(proc, fake_proc)
            mock_argv.assert_called_once()
            kwargs = mock_popen.call_args.kwargs
            self.assertEqual(kwargs.get("creationflags"), 0x208)
            self.assertEqual(kwargs.get("stdin"), runtime_control.subprocess.DEVNULL)
            self.assertEqual(kwargs.get("cwd"), str(home))
            self.assertTrue(str(getattr(kwargs.get("stdout"), "name", "")).endswith("cccc-web.log"))
            self.assertIs(kwargs.get("stdout"), kwargs.get("stderr"))

    def test_start_supervised_web_child_cleans_up_on_keyboard_interrupt(self) -> None:
        from cccc.ports.web import runtime_control
        from cccc.ports.web import bind_preflight

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            proc = unittest.mock.Mock(pid=4321)
            proc.poll.return_value = None
            with patch.object(bind_preflight, "ensure_tcp_port_bindable", return_value=None), patch.object(runtime_control, "spawn_web_child", return_value=proc), patch.object(
                runtime_control,
                "wait_for_web_ready",
                side_effect=lambda **_kwargs: (_ for _ in ()).throw(KeyboardInterrupt()),
            ), patch.object(runtime_control, "stop_web_child", return_value=True) as mock_stop, patch.object(
                runtime_control,
                "clear_web_runtime_state",
            ) as mock_clear:
                with self.assertRaises(KeyboardInterrupt):
                    runtime_control.start_supervised_web_child(
                        home=home,
                        host="127.0.0.1",
                        port=8848,
                        mode="normal",
                        reload=False,
                        log_level="info",
                        launch_source="test",
                    )

            mock_stop.assert_called_once_with(proc, timeout_s=1.0)
            mock_clear.assert_called_once_with(home=home, pid=4321)

    def test_start_supervised_web_child_uses_default_ready_timeout(self) -> None:
        from cccc.ports.web import runtime_control
        from cccc.ports.web import bind_preflight

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            proc = unittest.mock.Mock(pid=4321)
            with patch.object(bind_preflight, "ensure_tcp_port_bindable", return_value=None), patch.object(
                runtime_control, "spawn_web_child", return_value=proc
            ), patch.object(runtime_control, "wait_for_web_ready", return_value=True) as wait_ready, patch.dict(
                runtime_control.os.environ, {}, clear=True
            ):
                result, error = runtime_control.start_supervised_web_child(
                    home=home,
                    host="127.0.0.1",
                    port=8848,
                    mode="normal",
                    reload=False,
                    log_level="info",
                    launch_source="test",
                )

        self.assertIs(result, proc)
        self.assertIsNone(error)
        self.assertEqual(wait_ready.call_args.kwargs.get("timeout_s"), 10.0)

    def test_start_supervised_web_child_allows_ready_timeout_env_override(self) -> None:
        from cccc.ports.web import runtime_control
        from cccc.ports.web import bind_preflight

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            proc = unittest.mock.Mock(pid=4321)
            proc.poll.return_value = None
            with patch.object(bind_preflight, "ensure_tcp_port_bindable", return_value=None), patch.object(
                runtime_control, "spawn_web_child", return_value=proc
            ), patch.object(runtime_control, "wait_for_web_ready", return_value=False) as wait_ready, patch.object(
                runtime_control, "stop_web_child", return_value=True
            ), patch.object(runtime_control, "clear_web_runtime_state"), patch.dict(
                runtime_control.os.environ, {"CCCC_WEB_READY_TIMEOUT_SECONDS": "15"}, clear=True
            ):
                result, error = runtime_control.start_supervised_web_child(
                    home=home,
                    host="127.0.0.1",
                    port=8848,
                    mode="normal",
                    reload=False,
                    log_level="info",
                    launch_source="test",
                )

        self.assertIsNone(result)
        self.assertIn("within 15s", str(error or ""))
        self.assertEqual(wait_ready.call_args.kwargs.get("timeout_s"), 15.0)

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
