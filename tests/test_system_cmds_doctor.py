import argparse
import io
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch


class TestSystemCmdsDoctor(unittest.TestCase):
    def _doctor_output(self, available: dict[str, str]) -> str:
        from cccc.cli import system_cmds

        def which(name: str):
            return available.get(name)

        stream = io.StringIO()
        with (
            patch.object(system_cmds.sys, "platform", "linux"),
            patch.object(system_cmds.shutil, "which", side_effect=which),
            patch.object(system_cmds, "ensure_home", return_value=Path("/tmp/cccc-home")),
            patch.object(system_cmds, "call_daemon", return_value={"ok": False}),
            patch("cccc.kernel.runtime.detect_all_runtimes", return_value=[]),
            redirect_stdout(stream),
        ):
            self.assertEqual(system_cmds.cmd_doctor(argparse.Namespace(all=False)), 0)
        return stream.getvalue()

    def test_linux_doctor_reports_projected_browser_dependencies(self) -> None:
        output = self._doctor_output(
            {
                "google-chrome": "/usr/bin/google-chrome",
                "Xvfb": "/usr/bin/Xvfb",
                "x11vnc": "/usr/bin/x11vnc",
            }
        )

        self.assertIn("Projected Browser (Linux):", output)
        self.assertIn("System Chrome/Edge: OK (/usr/bin/google-chrome)", output)
        self.assertIn("Xvfb isolation: OK (/usr/bin/Xvfb)", output)
        self.assertIn("x11vnc viewer: OK (/usr/bin/x11vnc)", output)

    def test_linux_doctor_explains_required_and_optional_missing_tools(self) -> None:
        output = self._doctor_output({})

        self.assertIn("System Chrome/Edge: NOT FOUND (required for ChatGPT Web)", output)
        self.assertIn("Xvfb isolation: NOT FOUND (required; install `xvfb`)", output)
        self.assertIn("x11vnc viewer: NOT FOUND (optional; CDP screencast remains available)", output)


if __name__ == "__main__":
    unittest.main()
