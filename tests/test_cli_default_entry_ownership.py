import tempfile
import unittest
from pathlib import Path
from typing import Callable
from unittest.mock import patch


class _DaemonProc:
    def __init__(self, pid: int = 1234) -> None:
        self.pid = pid
        self.terminate_called = False
        self.kill_called = False

    def poll(self):
        return None

    def wait(self, timeout: float | None = None):
        _ = timeout
        return 0

    def terminate(self) -> None:
        self.terminate_called = True

    def kill(self) -> None:
        self.kill_called = True


class TestCliDefaultEntryOwnership(unittest.TestCase):
    def _with_home(self) -> tuple[Path, Callable[[], None]]:
        td_ctx = tempfile.TemporaryDirectory()
        td = Path(td_ctx.__enter__())

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)

        return td, cleanup

    def test_acquire_default_entry_lock_reports_existing_owner(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            with patch.object(common, "acquire_lockfile", side_effect=common.LockUnavailableError("busy")):
                handle, error = common._acquire_default_entry_lock(home)
            self.assertIsNone(handle)
            self.assertIn("already running", str(error or ""))
        finally:
            cleanup()

    def test_stop_existing_web_runtime_terminates_live_pid(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            with patch.object(common, "read_web_runtime_state", return_value={"pid": 4321, "launcher_pid": 9876}), patch.object(
                common, "pid_is_alive", side_effect=[True, False]
            ), patch.object(common, "terminate_pid") as mock_terminate, patch.object(
                common, "clear_web_runtime_state"
            ) as mock_clear:
                common._stop_existing_web_runtime(home)

            mock_terminate.assert_called_once_with(9876, timeout_s=2.0, include_group=True, force=True)
            mock_clear.assert_called_once_with(home=home, pid=9876)
        finally:
            cleanup()

    def test_stop_existing_daemon_shuts_down_and_cleans_state(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            daemon_dir = home / "daemon"
            daemon_dir.mkdir(parents=True, exist_ok=True)
            for name in ("ccccd.sock", "ccccd.addr.json", "ccccd.pid"):
                (daemon_dir / name).write_text("1234", encoding="utf-8")

            with patch.object(
                common,
                "call_daemon",
                side_effect=[
                    {"ok": True, "result": {"pid": 1234}},
                    {"ok": True, "result": {"message": "shutting down"}},
                    {"ok": False, "error": {"code": "daemon_unavailable"}},
                    {"ok": False, "error": {"code": "daemon_unavailable"}},
                ],
            ), patch.object(common, "terminate_pid") as mock_terminate:
                ok = common._stop_existing_daemon(home)

            self.assertTrue(ok)
            mock_terminate.assert_not_called()
            self.assertFalse((daemon_dir / "ccccd.sock").exists())
            self.assertFalse((daemon_dir / "ccccd.addr.json").exists())
            self.assertFalse((daemon_dir / "ccccd.pid").exists())
        finally:
            cleanup()

    def test_stop_existing_daemon_kills_stale_pid_file_when_ping_unavailable(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            daemon_dir = home / "daemon"
            daemon_dir.mkdir(parents=True, exist_ok=True)
            (daemon_dir / "ccccd.pid").write_text("2468", encoding="utf-8")

            with patch.object(common, "call_daemon", return_value={"ok": False}), patch.object(
                common, "pid_is_alive", return_value=True
            ), patch.object(common, "terminate_pid") as mock_terminate:
                ok = common._stop_existing_daemon(home)

            self.assertTrue(ok)
            mock_terminate.assert_called_once_with(2468, timeout_s=2.0, include_group=True, force=True)
            self.assertFalse((daemon_dir / "ccccd.pid").exists())
        finally:
            cleanup()

    def test_stop_existing_daemon_terminates_same_home_orphans(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            daemon_dir = home / "daemon"
            daemon_dir.mkdir(parents=True, exist_ok=True)

            with patch.object(common, "call_daemon", return_value={"ok": False}), patch.object(
                common, "_same_home_daemon_pids", return_value=[2468, 9753]
            ), patch.object(common, "terminate_pid") as mock_terminate:
                ok = common._stop_existing_daemon(home)

            self.assertTrue(ok)
            self.assertEqual(
                mock_terminate.call_args_list,
                [
                    unittest.mock.call(2468, timeout_s=2.0, include_group=True, force=True),
                    unittest.mock.call(9753, timeout_s=2.0, include_group=True, force=True),
                ],
            )
        finally:
            cleanup()

    def test_stop_existing_daemon_fails_when_orphan_termination_fails(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            with patch.object(common, "call_daemon", return_value={"ok": False}), patch.object(
                common, "_same_home_daemon_pids", return_value=[9753]
            ), patch.object(common, "terminate_pid", return_value=False):
                ok = common._stop_existing_daemon(home)

            self.assertFalse(ok)
        finally:
            cleanup()

    def test_same_home_daemon_pids_falls_back_to_lock_holders_when_proc_missing(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            daemon_dir = home / "daemon"
            daemon_dir.mkdir(parents=True, exist_ok=True)
            lock_path = daemon_dir / "ccccd.lock"
            lock_path.write_text("", encoding="utf-8")

            with patch.object(common, "ensure_home", return_value=home), patch.object(common.Path, "is_dir", return_value=False), patch.object(
                common.shutil, "which", return_value="/usr/sbin/lsof"
            ), patch.object(
                common.subprocess,
                "run",
                return_value=unittest.mock.Mock(returncode=0, stdout="2468\n9753\n2468\n", stderr=""),
            ) as mock_run:
                pids = common._same_home_daemon_pids(home)

            self.assertEqual(pids, [2468, 9753])
            mock_run.assert_called_once_with(
                ["lsof", "-t", str(lock_path)],
                capture_output=True,
                text=True,
                check=False,
            )
        finally:
            cleanup()

    def test_default_entry_ctrl_c_while_waiting_for_web_child_stops_web_and_daemon(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            daemon_proc = _DaemonProc()
            web_proc = unittest.mock.Mock(pid=4321)

            class _DummyThread:
                def __init__(self, *args, **kwargs) -> None:
                    _ = args, kwargs

                def start(self) -> None:
                    return None

            with patch.object(common, "_is_first_run", return_value=False), patch(
                "cccc.paths.ensure_home", return_value=home
            ), patch.object(common, "_acquire_default_entry_lock", return_value=("lock", None)), patch.object(
                common, "_stop_existing_web_runtime", return_value=True
            ), patch.object(common, "_stop_existing_daemon", return_value=True), patch.object(
                common, "_resolve_web_server_binding", return_value=("127.0.0.1", 8848)
            ), patch.object(common, "call_daemon", return_value={"ok": True}), patch.object(
                common.subprocess, "Popen", return_value=daemon_proc
            ), patch.object(common, "start_supervised_web_child", return_value=(web_proc, None)), patch.object(
                common, "wait_for_child_exit_interruptibly", side_effect=KeyboardInterrupt()
            ), patch.object(common, "stop_web_child", return_value=True) as mock_stop_web, patch.object(
                common, "clear_web_runtime_state"
            ) as mock_clear_web_state, patch.object(
                common, "release_lockfile"
            ) as mock_release, patch("threading.Thread", _DummyThread):
                ret = common._default_entry()

            self.assertEqual(ret, 0)
            mock_stop_web.assert_called_once_with(web_proc, timeout_s=2.0)
            mock_clear_web_state.assert_called_once_with(home=home, pid=4321)
            mock_release.assert_called_once_with("lock")
        finally:
            cleanup()

    def test_default_entry_registers_sigint_and_sigbreak_handlers(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            sigbreak_value = 21
            with patch.object(common, "_is_first_run", return_value=False), patch(
                "cccc.paths.ensure_home", return_value=home
            ), patch.object(common, "_acquire_default_entry_lock", return_value=("lock", None)), patch.object(
                common, "_stop_existing_web_runtime", return_value=False
            ), patch.object(common, "release_lockfile"), patch.object(
                common.signal, "getsignal", return_value="previous"
            ), patch.object(common.signal, "signal") as mock_signal, patch.object(
                common.signal, "SIGBREAK", sigbreak_value, create=True
            ):
                ret = common._default_entry()

            self.assertEqual(ret, 1)
            registered = [call.args[0] for call in mock_signal.call_args_list]
            self.assertIn(common.signal.SIGINT, registered)
            self.assertIn(sigbreak_value, registered)
        finally:
            cleanup()

    def test_default_entry_starts_daemon_with_supervised_process_kwargs(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            daemon_proc = _DaemonProc()
            web_proc = object()

            class _DummyThread:
                def __init__(self, *args, **kwargs) -> None:
                    _ = args, kwargs

                def start(self) -> None:
                    return None

            with patch.object(common, "_is_first_run", return_value=False), patch(
                "cccc.paths.ensure_home", return_value=home
            ), patch.object(common, "_acquire_default_entry_lock", return_value=("lock", None)), patch.object(
                common, "_stop_existing_web_runtime", return_value=True
            ), patch.object(common, "_stop_existing_daemon", return_value=True), patch.object(
                common, "_resolve_web_server_binding", return_value=("127.0.0.1", 8848)
            ), patch.object(common, "call_daemon", return_value={"ok": True}), patch.object(
                common.os, "getpid", return_value=9999
            ), patch.object(
                common, "supervised_process_popen_kwargs", return_value={"creationflags": 0x208}
            ), patch.object(common.subprocess, "Popen", return_value=daemon_proc) as mock_popen, patch.object(
                common, "start_supervised_web_child", return_value=(web_proc, None)
            ), patch.object(common, "wait_for_child_exit_interruptibly", side_effect=KeyboardInterrupt()), patch.object(
                common, "stop_web_child", return_value=True
            ), patch.object(common, "release_lockfile"), patch("threading.Thread", _DummyThread):
                ret = common._default_entry()

            self.assertEqual(ret, 0)
            kwargs = mock_popen.call_args.kwargs
            self.assertEqual(kwargs.get("creationflags"), 0x208)
            self.assertEqual(kwargs.get("stdin"), common.subprocess.DEVNULL)
            self.assertEqual(kwargs.get("cwd"), str(home))
            self.assertEqual(kwargs.get("env", {}).get("CCCC_HOME"), str(home))
            self.assertEqual(kwargs.get("env", {}).get("CCCC_DAEMON_SUPERVISOR_PID"), "9999")
        finally:
            cleanup()

    def test_default_entry_applies_invocation_port_override_to_initial_web_start(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            daemon_proc = _DaemonProc()
            web_proc = object()

            class _DummyThread:
                def __init__(self, *args, **kwargs) -> None:
                    _ = args, kwargs

                def start(self) -> None:
                    return None

            with patch.object(common, "_is_first_run", return_value=False), patch(
                "cccc.paths.ensure_home", return_value=home
            ), patch.object(common, "_acquire_default_entry_lock", return_value=("lock", None)), patch.object(
                common, "_stop_existing_web_runtime", return_value=True
            ), patch.object(common, "_stop_existing_daemon", return_value=True), patch.object(
                common, "_resolve_web_server_binding", return_value=("0.0.0.0", 8848)
            ), patch.object(common, "call_daemon", return_value={"ok": True}), patch.object(
                common.subprocess, "Popen", return_value=daemon_proc
            ), patch.object(common, "start_supervised_web_child", return_value=(web_proc, None)) as mock_start_web, patch.object(
                common, "wait_for_child_exit_interruptibly", side_effect=KeyboardInterrupt()
            ), patch.object(common, "stop_web_child", return_value=True), patch.object(
                common, "release_lockfile"
            ), patch("threading.Thread", _DummyThread):
                ret = common._default_entry(web_port_override=9000)

            self.assertEqual(ret, 0)
            self.assertEqual(mock_start_web.call_args.kwargs["host"], "0.0.0.0")
            self.assertEqual(mock_start_web.call_args.kwargs["port"], 9000)
        finally:
            cleanup()

    def test_default_entry_keeps_invocation_port_override_on_web_restart(self) -> None:
        from cccc.cli import common

        home, cleanup = self._with_home()
        try:
            daemon_proc = _DaemonProc()
            first_web_proc = unittest.mock.Mock(pid=1111)
            restarted_web_proc = unittest.mock.Mock(pid=2222)

            class _DummyThread:
                def __init__(self, *args, **kwargs) -> None:
                    _ = args, kwargs

                def start(self) -> None:
                    return None

            def _restart_with_assertion(**kwargs):
                self.assertEqual(kwargs["resolve_binding"](), ("0.0.0.0", 9000))
                return restarted_web_proc, "0.0.0.0", 9000

            with patch.object(common, "_is_first_run", return_value=False), patch(
                "cccc.paths.ensure_home", return_value=home
            ), patch.object(common, "_acquire_default_entry_lock", return_value=("lock", None)), patch.object(
                common, "_stop_existing_web_runtime", return_value=True
            ), patch.object(common, "_stop_existing_daemon", return_value=True), patch.object(
                common, "_resolve_web_server_binding", return_value=("0.0.0.0", 8848)
            ), patch.object(common, "call_daemon", return_value={"ok": True}), patch.object(
                common.subprocess, "Popen", return_value=daemon_proc
            ), patch.object(common, "start_supervised_web_child", return_value=(first_web_proc, None)), patch.object(
                common, "wait_for_child_exit_interruptibly", side_effect=[common.WEB_RUNTIME_RESTART_EXIT_CODE, KeyboardInterrupt()]
            ), patch.object(
                common, "restart_supervised_web_child_with_fallback", side_effect=_restart_with_assertion
            ) as mock_restart, patch.object(common, "stop_web_child", return_value=True), patch.object(
                common, "release_lockfile"
            ), patch("threading.Thread", _DummyThread):
                ret = common._default_entry(web_port_override=9000)

            self.assertEqual(ret, 0)
            self.assertEqual(mock_restart.call_count, 1)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
