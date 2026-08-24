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
    def test_status_prints_account_and_web_urls_without_actor_connector(self) -> None:
        lines = _membership_copy_lines(
            {
                "hostname": "https://d-1.cccc.foo",
                "web_url": "https://d-1.cccc.foo/ui/?token=acc_secret",
            }
        )
        self.assertEqual(len(lines), 4)
        self.assertTrue(lines[0].startswith("Hostname (people / account page):"))
        self.assertIn("https://d-1.cccc.foo", lines[0])
        self.assertNotIn("token=acc_secret", lines[0])
        self.assertTrue(
            lines[1].startswith("Web (this machine, includes admin token):")
        )
        self.assertIn("token=acc_secret", lines[1])
        self.assertIn("bearer credential", lines[2])
        self.assertIn("per actor", lines[3])

    def test_missing_urls_are_not_invented(self) -> None:
        self.assertEqual(_membership_copy_lines({}), [])
        lines = _membership_copy_lines({"hostname": "https://d-1.cccc.foo"})
        self.assertIn("(none)", lines[1])

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
