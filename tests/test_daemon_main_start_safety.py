import io
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch


class TestDaemonMainStartSafety(unittest.TestCase):
    def test_start_refuses_duplicate_when_pid_alive_but_ipc_down(self) -> None:
        from cccc.daemon.server import DaemonPaths
        from cccc import daemon_main

        with tempfile.TemporaryDirectory() as td:
            paths = DaemonPaths(home=Path(td))
            out = io.StringIO()
            with patch.object(daemon_main, "default_paths", return_value=paths), patch.object(
                daemon_main, "call_daemon", return_value={"ok": False}
            ), patch.object(daemon_main, "read_pid", return_value=12345), patch.object(
                daemon_main.os, "kill", return_value=None
            ), patch.object(
                daemon_main, "_spawn_daemon", return_value=67890
            ) as spawn_mock, redirect_stdout(
                out
            ):
                rc = daemon_main.main(["start"])

            self.assertEqual(rc, 1)
            self.assertFalse(spawn_mock.called)
            self.assertIn("refusing to spawn duplicate daemon", out.getvalue())

    def test_start_cleans_stale_pid_and_spawns(self) -> None:
        from cccc.daemon.server import DaemonPaths
        from cccc import daemon_main

        with tempfile.TemporaryDirectory() as td:
            paths = DaemonPaths(home=Path(td))
            out = io.StringIO()
            with patch.object(daemon_main, "default_paths", return_value=paths), patch.object(
                daemon_main, "call_daemon", return_value={"ok": False}
            ), patch.object(daemon_main, "read_pid", return_value=12345), patch.object(
                daemon_main.os, "kill", side_effect=OSError("no such process")
            ), patch.object(
                daemon_main, "_spawn_daemon", return_value=67890
            ) as spawn_mock, redirect_stdout(
                out
            ):
                rc = daemon_main.main(["start"])

            self.assertEqual(rc, 0)
            self.assertTrue(spawn_mock.called)
            text = out.getvalue()
            self.assertIn("cleaning up stale state", text)
            self.assertIn("started pid=67890", text)


if __name__ == "__main__":
    unittest.main()
