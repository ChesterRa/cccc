import argparse
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestSystemCmdsSetup(unittest.TestCase):
    def test_cmd_setup_codex_reports_added_via_shared_install_helper(self) -> None:
        from cccc.cli import system_cmds

        args = argparse.Namespace(runtime="codex", path=".")

        with patch("cccc.kernel.runtime.get_cccc_mcp_stdio_command", return_value=[r"C:\CCCC\cccc.exe", "mcp"]), patch(
            "cccc.kernel.runtime.detect_runtime",
            return_value=SimpleNamespace(available=True, path=r"C:\Tools\codex.cmd"),
        ), patch("cccc.daemon.mcp_install.is_mcp_installed", return_value=False), patch(
            "cccc.daemon.mcp_install.ensure_mcp_installed",
            return_value=True,
        ) as mock_ensure, patch.object(
            system_cmds,
            "_print_json",
        ) as mock_print:
            rc = system_cmds.cmd_setup(args)

        self.assertEqual(rc, 0)
        mock_ensure.assert_called_once_with(
            "codex",
            Path(".").resolve(),
            auto_mcp_runtimes=(
                "claude",
                "codex",
                "droid",
                "amp",
                "auggie",
                "neovate",
                "gemini",
                "hermes",
                "kimi",
                "opencode",
            ),
        )
        payload = mock_print.call_args.args[0]
        self.assertTrue(bool(payload.get("ok")))
        result = payload.get("result") if isinstance(payload.get("result"), dict) else {}
        mcp = result.get("mcp") if isinstance(result.get("mcp"), dict) else {}
        codex = mcp.get("codex") if isinstance(mcp.get("codex"), dict) else {}
        self.assertEqual(codex.get("status"), "added")

    def test_cmd_setup_kimi_reports_present_when_already_configured(self) -> None:
        from cccc.cli import system_cmds

        args = argparse.Namespace(runtime="kimi", path=".")

        with patch("cccc.kernel.runtime.get_cccc_mcp_stdio_command", return_value=[r"C:\CCCC\cccc.exe", "mcp"]), patch(
            "cccc.kernel.runtime.detect_runtime",
            return_value=SimpleNamespace(available=True, path=r"C:\Users\tester\.local\bin\kimi.exe"),
        ), patch("cccc.daemon.mcp_install.is_mcp_installed", return_value=True), patch(
            "cccc.daemon.mcp_install.ensure_mcp_installed",
            return_value=True,
        ), patch.object(
            system_cmds,
            "_print_json",
        ) as mock_print:
            rc = system_cmds.cmd_setup(args)

        self.assertEqual(rc, 0)
        payload = mock_print.call_args.args[0]
        self.assertTrue(bool(payload.get("ok")))
        result = payload.get("result") if isinstance(payload.get("result"), dict) else {}
        mcp = result.get("mcp") if isinstance(result.get("mcp"), dict) else {}
        kimi = mcp.get("kimi") if isinstance(mcp.get("kimi"), dict) else {}
        self.assertEqual(kimi.get("status"), "present")

    def test_cmd_setup_claude_manual_hint_uses_resolved_executable_path(self) -> None:
        from cccc.cli import system_cmds

        args = argparse.Namespace(runtime="claude", path=".")
        resolved_cmd = [r"C:\Users\tester\.local\bin\claude.exe", "mcp", "add", "-s", "user", "cccc", "--", r"C:\CCCC\cccc.exe", "mcp"]

        with patch("cccc.kernel.runtime.get_cccc_mcp_stdio_command", return_value=[r"C:\CCCC\cccc.exe", "mcp"]), patch(
            "cccc.daemon.mcp_install.build_mcp_add_command",
            return_value=["claude", "mcp", "add", "-s", "user", "cccc", "--", r"C:\CCCC\cccc.exe", "mcp"],
        ), patch(
            "cccc.kernel.runtime.detect_runtime",
            return_value=SimpleNamespace(available=True, path=resolved_cmd[0]),
        ), patch("cccc.daemon.mcp_install.is_mcp_installed", return_value=False), patch(
            "cccc.daemon.mcp_install.ensure_mcp_installed",
            return_value=False,
        ), patch.object(
            system_cmds,
            "resolve_subprocess_argv",
            return_value=resolved_cmd,
        ), patch.object(
            system_cmds,
            "_print_json",
        ) as mock_print:
            rc = system_cmds.cmd_setup(args)

        self.assertEqual(rc, 0)
        payload = mock_print.call_args.args[0]
        result = payload.get("result") if isinstance(payload.get("result"), dict) else {}
        mcp = result.get("mcp") if isinstance(result.get("mcp"), dict) else {}
        claude = mcp.get("claude") if isinstance(mcp.get("claude"), dict) else {}
        self.assertEqual(claude.get("mode"), "manual")
        self.assertEqual(
            claude.get("command"),
            " ".join(system_cmds.shlex.quote(part) for part in resolved_cmd),
        )

    def test_cmd_setup_opencode_reports_runtime_env_injection(self) -> None:
        from cccc.cli import system_cmds

        args = argparse.Namespace(runtime="opencode", path=".")

        with patch(
            "cccc.kernel.runtime.detect_runtime",
            return_value=SimpleNamespace(available=True, path="/usr/local/bin/opencode"),
        ), patch("cccc.daemon.mcp_install.ensure_mcp_installed") as mock_ensure, patch.object(
            system_cmds,
            "_print_json",
        ) as mock_print:
            rc = system_cmds.cmd_setup(args)

        self.assertEqual(rc, 0)
        mock_ensure.assert_not_called()
        payload = mock_print.call_args.args[0]
        self.assertTrue(bool(payload.get("ok")))
        result = payload.get("result") if isinstance(payload.get("result"), dict) else {}
        mcp = result.get("mcp") if isinstance(result.get("mcp"), dict) else {}
        opencode = mcp.get("opencode") if isinstance(mcp.get("opencode"), dict) else {}
        self.assertEqual(opencode.get("mode"), "auto")
        self.assertEqual(opencode.get("status"), "runtime_env")


if __name__ == "__main__":
    unittest.main()
