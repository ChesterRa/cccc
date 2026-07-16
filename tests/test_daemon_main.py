from __future__ import annotations

import io
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestDaemonMain(unittest.TestCase):
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

    def test_stop_also_stops_supervised_web_runtime_after_daemon_shutdown(self) -> None:
        from cccc import daemon_main

        _, cleanup = self._with_home()
        stdout = io.StringIO()
        try:
            with patch.object(daemon_main, "call_daemon", return_value={"ok": True}), patch.object(
                daemon_main, "read_web_runtime_state", return_value={"pid": 4321, "launcher_pid": 9876}
            ), patch.object(
                daemon_main, "pid_is_alive", side_effect=[True, False]
            ), patch.object(daemon_main, "terminate_pid", return_value=True) as mock_terminate, patch.object(
                daemon_main, "clear_web_runtime_state"
            ) as mock_clear, patch.object(
                daemon_main.sys, "stdout", stdout
            ):
                rc = daemon_main.main(["stop"])
        finally:
            cleanup()

        self.assertEqual(rc, 0)
        mock_terminate.assert_called_once_with(9876, timeout_s=2.0, include_group=True, force=True)
        mock_clear.assert_called_once_with(home=unittest.mock.ANY, pid=9876)
        self.assertIn("shutdown requested", stdout.getvalue())

    def test_stop_reports_failure_when_supervised_web_runtime_cannot_stop(self) -> None:
        from cccc import daemon_main

        _, cleanup = self._with_home()
        stdout = io.StringIO()
        try:
            with patch.object(daemon_main, "call_daemon", return_value={"ok": True}), patch.object(
                daemon_main, "read_web_runtime_state", return_value={"pid": 4321, "launcher_pid": 9876}
            ), patch.object(daemon_main, "pid_is_alive", return_value=True), patch.object(
                daemon_main, "terminate_pid", return_value=False
            ), patch.object(
                daemon_main.sys, "stdout", stdout
            ):
                rc = daemon_main.main(["stop"])
        finally:
            cleanup()

        self.assertEqual(rc, 1)
        self.assertIn("failed to stop supervised web runtime", stdout.getvalue())

    def test_spawn_daemon_uses_supervised_process_kwargs(self) -> None:
        from cccc.daemon.server import DaemonPaths
        from cccc import daemon_main

        _, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as td:
                paths = DaemonPaths(home=Path(td))
                fake_proc = unittest.mock.Mock(pid=67890)
                with patch.object(daemon_main, "resolve_background_python_argv", return_value=[r"D:\dev\cccc\.venv\Scripts\pythonw.exe", "-m", "cccc.daemon_main", "run"]) as mock_argv, patch.object(daemon_main, "supervised_process_popen_kwargs", return_value={"creationflags": 0x208}), patch.object(
                    daemon_main.subprocess,
                    "Popen",
                    return_value=fake_proc,
                ) as mock_popen:
                    pid = daemon_main._spawn_daemon(paths)

                self.assertEqual(pid, 67890)
                mock_argv.assert_called_once_with([daemon_main.sys.executable, "-m", "cccc.daemon_main", "run"])
                kwargs = mock_popen.call_args.kwargs
                self.assertEqual(kwargs.get("creationflags"), 0x208)
                self.assertEqual(kwargs.get("stdin"), daemon_main.subprocess.DEVNULL)
                self.assertEqual(kwargs.get("cwd"), str(paths.home))
                self.assertEqual(kwargs.get("env", {}).get("CCCC_HOME"), str(paths.home))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
