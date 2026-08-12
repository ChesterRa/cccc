import argparse
import io
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestSystemCmdsDoctor(unittest.TestCase):
    def _doctor_output(
        self,
        available: dict[str, str],
        *,
        platform: str = "linux",
        runtimes: list[object] | None = None,
    ) -> str:
        from cccc.cli import system_cmds

        def which(name: str):
            return available.get(name)

        stream = io.StringIO()
        browser = next(
            (
                available[name]
                for name in (
                    "google-chrome",
                    "google-chrome-stable",
                    "microsoft-edge",
                    "microsoft-edge-stable",
                )
                if name in available
            ),
            None,
        )
        with (
            patch.object(system_cmds.sys, "platform", platform),
            patch.object(system_cmds.shutil, "which", side_effect=which),
            patch.object(
                system_cmds,
                "_projected_browser_path",
                return_value=Path(browser) if browser else None,
                create=True,
            ),
            patch.object(system_cmds, "ensure_home", return_value=Path("/tmp/cccc-home")),
            patch.object(system_cmds, "call_daemon", return_value={"ok": False}),
            patch.object(
                system_cmds,
                "inspect_cccc_installation",
                return_value={
                    "current_executable": "/opt/cccc/bin/cccc",
                    "resolved_command": "/usr/local/bin/cccc",
                    "command_candidates": ["/usr/local/bin/cccc", "/opt/cccc/bin/cccc"],
                    "conflicting_commands": ["/usr/local/bin/cccc"],
                    "path_status": "conflict",
                    "path_conflict": True,
                },
            ),
            patch("cccc.kernel.runtime.detect_all_runtimes", return_value=runtimes or []),
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
        self.assertIn("Current executable: /opt/cccc/bin/cccc", output)
        self.assertIn("PATH resolves to: /usr/local/bin/cccc", output)
        self.assertIn("PATH status: CONFLICT", output)
        self.assertIn("Other CCCC commands left unchanged:", output)

    def test_linux_doctor_explains_required_and_optional_missing_tools(self) -> None:
        output = self._doctor_output({})

        self.assertIn("System Chrome/Edge: NOT FOUND (required for ChatGPT Web)", output)
        self.assertIn("Xvfb isolation: NOT FOUND (required; install `xvfb`)", output)
        self.assertIn("x11vnc viewer: NOT FOUND (optional; CDP screencast remains available)", output)

    def test_macos_doctor_reports_the_browser_used_by_the_projected_runtime(self) -> None:
        output = self._doctor_output(
            {"google-chrome": "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"},
            platform="darwin",
        )

        self.assertIn("Projected Browser:", output)
        self.assertIn(
            "System Chrome/Edge: OK (/Applications/Google Chrome.app/Contents/MacOS/Google Chrome)",
            output,
        )

    def test_doctor_does_not_render_none_as_an_available_runtime_path(self) -> None:
        output = self._doctor_output(
            {},
            runtimes=[SimpleNamespace(name="Web Model", available=True, path=None)],
        )

        self.assertIn("[OK] Web Model: OK", output)
        self.assertNotIn("Web Model: OK (None)", output)


if __name__ == "__main__":
    unittest.main()
