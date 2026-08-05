from __future__ import annotations

import unittest
from unittest.mock import patch


class TestCliDaemonCompatibility(unittest.TestCase):
    def test_business_command_reuses_capable_daemon_from_another_version(self) -> None:
        from cccc.cli import common

        responses = [
            {"ok": True, "result": {"version": "0.4.999", "implementation": "rust"}},
            {"ok": True, "result": {}},
            {"ok": True, "result": {}},
        ]
        with patch.object(common, "call_daemon", side_effect=responses) as call_daemon:
            self.assertTrue(common._ensure_daemon_running())

        self.assertNotIn("shutdown", [call.args[0].get("op") for call in call_daemon.call_args_list])

    def test_business_command_does_not_replace_incompatible_daemon(self) -> None:
        from cccc.cli import common

        responses = [
            {"ok": True, "result": {"version": "0.4.1", "pid": 123}},
            {"ok": False, "error": {"code": "unknown_op"}},
        ]
        with (
            patch.object(common, "call_daemon", side_effect=responses) as call_daemon,
            patch.object(common.subprocess, "run") as run,
        ):
            self.assertFalse(common._ensure_daemon_running())

        self.assertNotIn("shutdown", [call.args[0].get("op") for call in call_daemon.call_args_list])
        run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
