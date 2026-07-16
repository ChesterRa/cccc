import tempfile
import unittest
from pathlib import Path

from cccc.daemon.im.im_bridge_ops import (
    cleanup_invalid_im_bridges,
    read_live_im_bridge_pid,
    stop_all_im_bridges,
    stop_im_bridges_for_group,
)


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


class TestImUnsetOrphanScan(unittest.TestCase):
    """T208: cmd_im_unset must terminate orphan bridge processes even when pid file is missing."""

    def test_unset_kills_orphan_when_no_pidfile(self) -> None:
        from unittest.mock import MagicMock, patch

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            group_id = "g_test_orphan"
            group_dir = home / "groups" / group_id
            state_dir = group_dir / "state"
            state_dir.mkdir(parents=True, exist_ok=True)
            # No pid file — simulate the orphan scenario

            # Create a fake group.yaml so load_group succeeds
            group_yaml = group_dir / "group.yaml"
            group_yaml.write_text(
                f"v: 1\ngroup_id: {group_id}\ntitle: test\ntopic: ''\n"
                f"created_at: '2026-01-01T00:00:00Z'\nupdated_at: '2026-01-01T00:00:00Z'\n"
                f"running: true\nstate: active\nactive_scope_key: s_test\nscopes: []\nactors: []\n"
                f"im:\n  platform: dingtalk\n",
                encoding="utf-8",
            )

            # Track which pids got killed
            killed_pids: list[int] = []

            def mock_signal(pid, sig, include_group=False):  # noqa: ARG001
                killed_pids.append(pid)

            orphan_pid = 99999

            # Mock _im_find_bridge_pids_by_script to return the orphan pid
            # Mock _resolve_group_id to return our test group_id
            # Mock ensure_home to return our temp dir
            with (
                patch("cccc.cli.im_cmds._im_find_bridge_pids_by_script", return_value=[orphan_pid]),
                patch("cccc.cli.im_cmds._resolve_group_id", return_value=group_id),
                patch("cccc.cli.im_cmds.best_effort_signal_pid", side_effect=mock_signal),
                patch("cccc.kernel.group.ensure_home", return_value=home),
                patch("cccc.cli.im_cmds.ensure_home", return_value=home),
            ):
                import argparse

                from cccc.cli.im_cmds import cmd_im_unset

                args = argparse.Namespace(group=group_id)
                rc = cmd_im_unset(args)

            self.assertEqual(rc, 0)
            # The orphan bridge process must have been killed
            self.assertIn(orphan_pid, killed_pids)
            # IM config should be removed (group.yaml re-read won't have 'im' key)
            import yaml

            with open(group_yaml, encoding="utf-8") as f:
                doc = yaml.safe_load(f)
            self.assertNotIn("im", doc)


if __name__ == "__main__":
    unittest.main()
