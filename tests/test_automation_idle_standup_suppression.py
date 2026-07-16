"""Test that built-in automation rules (standup) are suppressed when group is idle (T188)."""
import os
import tempfile
import unittest
from datetime import datetime, timezone, timedelta
from unittest.mock import patch


class TestIdleStandupSuppression(unittest.TestCase):
    """Verify standup rule does not fire when group state is idle."""

    def setUp(self) -> None:
        self._old_home = os.environ.get("CCCC_HOME")
        self._td = tempfile.TemporaryDirectory()
        os.environ["CCCC_HOME"] = self._td.name

    def tearDown(self) -> None:
        self._td.cleanup()
        if self._old_home is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = self._old_home

    def _create_group_with_actor_and_rules(self, rules_payload):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group

        resp, _ = handle_request(
            DaemonRequest.model_validate(
                {"op": "group_create", "args": {"title": "idle-test", "topic": "", "by": "user"}}
            )
        )
        assert resp.ok
        group_id = str((resp.result or {}).get("group_id") or "").strip()

        # Add an actor with foreman role (avoid reserved id "foreman")
        add_resp, _ = handle_request(
            DaemonRequest.model_validate(
                {
                    "op": "actor_add",
                    "args": {
                        "group_id": group_id,
                        "by": "user",
                        "actor_id": "claude-1",
                        "role": "foreman",
                        "runner": "pty",
                    },
                }
            )
        )
        assert add_resp.ok, getattr(add_resp, "error", None)

        handle_request(
            DaemonRequest.model_validate(
                {
                    "op": "group_automation_update",
                    "args": {
                        "group_id": group_id,
                        "by": "user",
                        "ruleset": rules_payload,
                    },
                }
            )
        )
        group = load_group(group_id)
        assert group is not None
        return group

    def _standup_rules_payload(self):
        return {
            "rules": [
                {
                    "id": "standup",
                    "enabled": True,
                    "scope": "group",
                    "to": ["@foreman"],
                    "trigger": {"kind": "interval", "every_seconds": 1},
                    "action": {
                        "kind": "notify",
                        "priority": "normal",
                        "requires_ack": False,
                        "title": "Stand-up reminder",
                        "snippet_ref": "standup",
                        "message": "",
                    },
                }
            ],
            "snippets": {"standup": "test standup snippet"},
        }

    def test_standup_suppressed_when_idle(self) -> None:
        """Standup rule must NOT reach delivery when group is idle."""
        from cccc.daemon.automation.engine import AutomationManager

        group = self._create_group_with_actor_and_rules(self._standup_rules_payload())
        mgr = AutomationManager()
        now = datetime.now(timezone.utc)

        # Seed last_fired_at
        mgr._check_rules(group, now, group_state="active")
        later = now + timedelta(seconds=5)

        with (
            patch("cccc.daemon.automation.engine.append_event") as mock_append,
            patch("cccc.daemon.automation.engine.pty_runner") as mock_pty,
            patch("cccc.daemon.automation.engine.headless_runner"),
            patch("cccc.daemon.automation.engine._queue_notify_to_pty"),
        ):
            mock_pty.SUPERVISOR.actor_running.return_value = True
            mgr._check_rules(group, later, group_state="idle")
            self.assertEqual(mock_append.call_count, 0,
                             "standup must NOT fire when group is idle")

    def test_standup_fires_when_active(self) -> None:
        """Standup rule should reach delivery when group is active."""
        from cccc.daemon.automation.engine import AutomationManager

        group = self._create_group_with_actor_and_rules(self._standup_rules_payload())
        mgr = AutomationManager()
        now = datetime.now(timezone.utc)

        # Seed
        mgr._check_rules(group, now, group_state="active")
        later = now + timedelta(seconds=5)

        with (
            patch("cccc.daemon.automation.engine.append_event", return_value={"id": "ev1"}) as mock_append,
            patch("cccc.daemon.automation.engine.pty_runner") as mock_pty,
            patch("cccc.daemon.automation.engine.headless_runner"),
            patch("cccc.daemon.automation.engine._queue_notify_to_pty"),
        ):
            mock_pty.SUPERVISOR.actor_running.return_value = True
            mgr._check_rules(group, later, group_state="active")
            self.assertGreater(mock_append.call_count, 0,
                               "standup should reach delivery when active")

    def test_custom_rule_not_suppressed_when_idle(self) -> None:
        """Non-standup rules should still fire when idle."""
        from cccc.daemon.automation.engine import AutomationManager

        group = self._create_group_with_actor_and_rules(
            {
                "rules": [
                    {
                        "id": "my_custom_check",
                        "enabled": True,
                        "scope": "group",
                        "to": ["@foreman"],
                        "trigger": {"kind": "interval", "every_seconds": 1},
                        "action": {
                            "kind": "notify",
                            "priority": "normal",
                            "requires_ack": False,
                            "title": "Custom check",
                            "message": "custom body here",
                        },
                    }
                ],
                "snippets": {},
            }
        )

        mgr = AutomationManager()
        now = datetime.now(timezone.utc)

        # Seed
        mgr._check_rules(group, now, group_state="idle")
        later = now + timedelta(seconds=5)

        with (
            patch("cccc.daemon.automation.engine.append_event", return_value={"id": "ev1"}) as mock_append,
            patch("cccc.daemon.automation.engine.pty_runner") as mock_pty,
            patch("cccc.daemon.automation.engine.headless_runner"),
            patch("cccc.daemon.automation.engine._queue_notify_to_pty"),
        ):
            mock_pty.SUPERVISOR.actor_running.return_value = True
            mgr._check_rules(group, later, group_state="idle")
            self.assertGreater(mock_append.call_count, 0,
                               "custom rule should still fire when idle")

    def test_idle_suppressed_rule_ids_contains_standup(self) -> None:
        """Verify the suppression set includes 'standup'."""
        from cccc.daemon.automation.engine import AutomationManager

        self.assertIn("standup", AutomationManager._IDLE_SUPPRESSED_RULE_IDS)


if __name__ == "__main__":
    unittest.main()
