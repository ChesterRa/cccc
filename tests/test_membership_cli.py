from __future__ import annotations

import unittest
from argparse import Namespace
from unittest.mock import patch

from cccc.cli.membership_cmds import (
    REACH_LONG_OPERATION_TIMEOUT_SECONDS,
    _membership_copy_lines,
    cmd_reach,
)


class TestMembershipCliCopyLines(unittest.TestCase):
    def test_status_prints_three_separate_strings(self) -> None:
        lines = _membership_copy_lines(
            {
                "hostname": "https://d-1.cccc.foo",
                "web_url": "https://d-1.cccc.foo/ui/?token=acc_secret",
                "connector_url": "https://d-1.cccc.foo/mcp/web-model/wmc_1/token/secret",
            }
        )
        self.assertEqual(len(lines), 5)
        self.assertTrue(lines[0].startswith("Hostname (people / account page):"))
        self.assertIn("https://d-1.cccc.foo", lines[0])
        self.assertNotIn("token=acc_secret", lines[0])
        self.assertTrue(
            lines[1].startswith("Web (this machine, includes admin token):")
        )
        self.assertIn("token=acc_secret", lines[1])
        self.assertTrue(lines[2].startswith("ChatGPT connector (secret in the path):"))
        self.assertIn("/token/secret", lines[2])
        self.assertIn("three different strings", lines[3])
        self.assertIn("Paste it again", lines[4])

    def test_missing_urls_are_not_invented(self) -> None:
        self.assertEqual(_membership_copy_lines({}), [])
        lines = _membership_copy_lines({"hostname": "https://d-1.cccc.foo"})
        self.assertIn("(none)", lines[1])
        self.assertIn("(none)", lines[2])

    def test_reach_install_and_on_use_a_long_operation_timeout(self) -> None:
        for action in ("install", "on"):
            with self.subTest(action=action), patch(
                "cccc.cli.membership_cmds._ensure_daemon_running", return_value=True
            ), patch("cccc.cli.membership_cmds.call_daemon") as call:
                call.return_value = {"ok": True, "result": {"membership": {}}}
                self.assertEqual(cmd_reach(Namespace(action=action)), 0)
                self.assertEqual(
                    call.call_args.kwargs["timeout_s"],
                    REACH_LONG_OPERATION_TIMEOUT_SECONDS,
                )
