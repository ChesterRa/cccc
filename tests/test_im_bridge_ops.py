import tempfile
import unittest
import os
from pathlib import Path
from unittest.mock import patch

from cccc.daemon.im.im_bridge_ops import (
    cleanup_invalid_im_bridges,
    read_live_im_bridge_pid,
    stop_all_im_bridges,
    stop_im_bridges_for_group,
)
from cccc.util.file_lock import acquire_lockfile, release_lockfile


class TestImBridgeOps(unittest.TestCase):
    def test_stop_group_refuses_live_pid_without_owned_lock(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            pid_path = home / "groups" / "g_test" / "state" / "im_bridge.pid"
            pid_path.parent.mkdir(parents=True, exist_ok=True)
            pid_path.write_text("424242", encoding="utf-8")
            signaled: list[int] = []

            with patch("cccc.daemon.im.im_bridge_ops.pid_is_alive", return_value=True):
                stopped = stop_im_bridges_for_group(
                    home,
                    group_id="g_test",
                    best_effort_killpg=lambda pid, _sig: signaled.append(pid),
                )

            self.assertEqual(stopped, 0)
            self.assertEqual(signaled, [])
            self.assertFalse(pid_path.exists())

    def test_stop_group_signals_the_current_singleton_lock_owner(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            state_dir = home / "groups" / "g_test" / "state"
            state_dir.mkdir(parents=True, exist_ok=True)
            pid_path = state_dir / "im_bridge.pid"
            pid_path.write_text(str(os.getpid()), encoding="utf-8")
            lock = acquire_lockfile(state_dir / "im_bridge.lock", blocking=False)
            lock.seek(0)
            lock.write(f"{os.getpid()}\n".encode())
            lock.truncate()
            lock.flush()
            signaled: list[int] = []
            try:
                stopped = stop_im_bridges_for_group(
                    home,
                    group_id="g_test",
                    best_effort_killpg=lambda pid, _sig: signaled.append(pid),
                )
            finally:
                release_lockfile(lock)

            self.assertEqual(stopped, 1)
            self.assertEqual(signaled, [os.getpid()])
            self.assertFalse(pid_path.exists())

    def test_stop_group_no_group_id(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            killed = stop_im_bridges_for_group(home, group_id="", best_effort_killpg=lambda _pid, _sig: None)
            self.assertEqual(killed, 0)

    def test_stop_all_no_groups(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            killed = stop_all_im_bridges(home, best_effort_killpg=lambda _pid, _sig: None)
            self.assertEqual(killed, 0)

    def test_cleanup_invalid_no_groups(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            result = cleanup_invalid_im_bridges(
                home,
                pid_alive=lambda _pid: False,
                best_effort_killpg=lambda _pid, _sig: None,
            )
            self.assertEqual(result, {"killed": 0, "stale_pidfiles": 0})

    def test_cleanup_removes_stale_pidfile(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            pid_path = home / "groups" / "g_test" / "state" / "im_bridge.pid"
            pid_path.parent.mkdir(parents=True, exist_ok=True)
            pid_path.write_text("999999", encoding="utf-8")

            result = cleanup_invalid_im_bridges(
                home,
                pid_alive=lambda _pid: False,
                best_effort_killpg=lambda _pid, _sig: None,
            )
            self.assertEqual(int(result.get("stale_pidfiles") or 0), 1)
            self.assertFalse(pid_path.exists())

    def test_read_live_pid_removes_dead_or_zombie_pidfile(self) -> None:
        from unittest.mock import patch

        with tempfile.TemporaryDirectory() as td:
            pid_path = Path(td) / "im_bridge.pid"
            pid_path.write_text("4321", encoding="utf-8")

            with (
                patch("cccc.daemon.im.im_bridge_ops.os.waitpid", side_effect=ChildProcessError()),
                patch("cccc.daemon.im.im_bridge_ops.pid_is_alive", return_value=False),
            ):
                pid = read_live_im_bridge_pid(pid_path)

            self.assertIsNone(pid)
            self.assertFalse(pid_path.exists())

    def test_read_live_pid_reaps_exited_child_pidfile(self) -> None:
        from unittest.mock import patch

        with tempfile.TemporaryDirectory() as td:
            pid_path = Path(td) / "im_bridge.pid"
            pid_path.write_text("4321", encoding="utf-8")

            with patch("cccc.daemon.im.im_bridge_ops.os.waitpid", return_value=(4321, 0)):
                pid = read_live_im_bridge_pid(pid_path)

            self.assertIsNone(pid)
            self.assertFalse(pid_path.exists())


if __name__ == "__main__":
    unittest.main()
