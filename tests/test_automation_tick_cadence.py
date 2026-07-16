from __future__ import annotations

import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestAutomationTickCadence(unittest.TestCase):
    def test_idle_group_rules_are_throttled_between_ticks(self) -> None:
        from cccc.daemon.automation.engine import AutomationManager

        manager = AutomationManager()
        fake_group = SimpleNamespace(group_id="g-idle", doc={})

        with patch("cccc.daemon.automation.engine.load_group", return_value=fake_group), patch(
            "cccc.daemon.automation.engine.get_group_state", return_value="idle"
        ), patch(
            "cccc.daemon.automation.engine.headless_runner.SUPERVISOR.group_running", return_value=False
        ), patch(
            "cccc.daemon.automation.engine.pty_runner.SUPERVISOR.group_running", return_value=True
        ), patch.object(
            manager, "_check_rules"
        ) as check_rules:
            home = Path("/tmp/cccc-automation-cadence")
            with patch.object(Path, "exists", return_value=True), patch.object(
                Path, "glob", return_value=[home / "groups" / "g-idle" / "group.yaml"]
            ):
                manager.tick(home=home)
                manager.tick(home=home)

        self.assertEqual(check_rules.call_count, 1)


if __name__ == "__main__":
    unittest.main()
