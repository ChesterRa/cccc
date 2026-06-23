from __future__ import annotations

import importlib
import os
import unittest
from unittest.mock import patch


class TestCliMain(unittest.TestCase):
    def test_parser_accepts_copilot_runtime_for_primary_runtime_commands(self) -> None:
        cli_main = importlib.import_module("cccc.cli.main")

        parser = cli_main.build_parser()
        self.assertEqual(parser.parse_args(["actor", "add", "peer", "--runtime", "copilot"]).runtime, "copilot")
        self.assertEqual(parser.parse_args(["actor", "update", "peer", "--runtime", "copilot"]).runtime, "copilot")
        self.assertEqual(parser.parse_args(["setup", "--runtime", "copilot"]).runtime, "copilot")

    def test_parser_still_accepts_devin_and_kiro_runtime_choices(self) -> None:
        cli_main = importlib.import_module("cccc.cli.main")

        parser = cli_main.build_parser()
        for runtime in ("devin", "kiro"):
            self.assertEqual(parser.parse_args(["actor", "add", "peer", "--runtime", runtime]).runtime, runtime)
            self.assertEqual(parser.parse_args(["actor", "update", "peer", "--runtime", runtime]).runtime, runtime)
            self.assertEqual(parser.parse_args(["setup", "--runtime", runtime]).runtime, runtime)

    def test_main_uses_default_entry_when_no_subcommand(self) -> None:
        cli_main = importlib.import_module("cccc.cli.main")

        with patch.object(cli_main, "_default_entry", return_value=7) as mock_default:
            rc = cli_main.main([])

        self.assertEqual(rc, 7)
        mock_default.assert_called_once_with(web_host_override="", web_port_override=None)

    def test_main_applies_top_level_port_override_to_default_entry_only_for_invocation(self) -> None:
        cli_main = importlib.import_module("cccc.cli.main")

        old_port = os.environ.get("CCCC_WEB_PORT")

        def _assert_port_and_return(*, web_host_override: str, web_port_override: int | None) -> int:
            self.assertEqual(os.environ.get("CCCC_WEB_PORT"), "9000")
            self.assertEqual(web_host_override, "")
            self.assertEqual(web_port_override, 9000)
            return 0

        try:
            with patch.object(cli_main, "_default_entry", side_effect=_assert_port_and_return) as mock_default:
                rc = cli_main.main(["--port", "9000"])
        finally:
            if old_port is None:
                os.environ.pop("CCCC_WEB_PORT", None)
            else:
                os.environ["CCCC_WEB_PORT"] = old_port

        self.assertEqual(rc, 0)
        mock_default.assert_called_once_with(web_host_override="", web_port_override=9000)
        self.assertEqual(os.environ.get("CCCC_WEB_PORT"), old_port)

    def test_main_accepts_top_level_port_override_before_subcommand(self) -> None:
        cli_main = importlib.import_module("cccc.cli.main")

        old_port = os.environ.get("CCCC_WEB_PORT")

        def _assert_port_and_return(_args) -> int:
            self.assertEqual(os.environ.get("CCCC_WEB_PORT"), "9000")
            return 0

        try:
            with patch.object(cli_main, "cmd_daemon", side_effect=_assert_port_and_return):
                rc = cli_main.main(["--port", "9000", "daemon", "status"])
        finally:
            if old_port is None:
                os.environ.pop("CCCC_WEB_PORT", None)
            else:
                os.environ["CCCC_WEB_PORT"] = old_port

        self.assertEqual(rc, 0)
        self.assertEqual(os.environ.get("CCCC_WEB_PORT"), old_port)


if __name__ == "__main__":
    unittest.main()
