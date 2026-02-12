import tempfile
import unittest
from pathlib import Path

from cccc.daemon.im_bridge_ops import cleanup_invalid_im_bridges, stop_all_im_bridges, stop_im_bridges_for_group


class TestImBridgeOps(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
