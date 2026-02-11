import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from unittest.mock import patch


class TestAutomationRulesConstraints(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def _create_group_id(self) -> str:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        resp, _ = handle_request(
            DaemonRequest.model_validate(
                {"op": "group_create", "args": {"title": "automation-constraints", "topic": "", "by": "user"}}
            )
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))
        gid = str((resp.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def test_group_state_rejects_non_one_time_schedule(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        _, cleanup = self._with_home()
        try:
            gid = self._create_group_id()
            resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "group_automation_manage",
                        "args": {
                            "group_id": gid,
                            "by": "user",
                            "op": "create",
                            "rule": {
                                "id": "bad_group_state_interval",
                                "enabled": True,
                                "scope": "group",
                                "to": ["@foreman"],
                                "trigger": {"kind": "interval", "every_seconds": 60},
                                "action": {"kind": "group_state", "state": "paused"},
                            },
                        },
                    }
                )
            )
            self.assertFalse(resp.ok)
            err = resp.error.model_dump() if resp.error else {}
            self.assertEqual(str(err.get("code") or ""), "group_automation_manage_failed")
            self.assertIn("only supports trigger.kind=at", str(err.get("message") or ""))
        finally:
            cleanup()

    def test_at_rule_retime_clears_completion_state(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group
        from cccc.util.fs import read_json, atomic_write_json

        _, cleanup = self._with_home()
        try:
            gid = self._create_group_id()
            create_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "group_automation_update",
                        "args": {
                            "group_id": gid,
                            "by": "user",
                            "ruleset": {
                                "rules": [
                                    {
                                        "id": "once_rule",
                                        "enabled": True,
                                        "scope": "group",
                                        "to": ["@foreman"],
                                        "trigger": {"kind": "at", "at": "2030-01-01T00:00:00Z"},
                                        "action": {"kind": "notify", "message": "hello"},
                                    }
                                ],
                                "snippets": {},
                            },
                        },
                    }
                )
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))

            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            state_path = group.path / "state" / "automation.json"
            state = read_json(state_path)
            if not isinstance(state, dict):
                state = {}
            rules_state = state.get("rules")
            if not isinstance(rules_state, dict):
                rules_state = {}
            rules_state["once_rule"] = {
                "at_fired": True,
                "last_slot_key": "at:2030-01-01T00:00:00Z",
                "last_fired_at": "2030-01-01T00:00:00Z",
            }
            state["rules"] = rules_state
            atomic_write_json(state_path, state)

            update_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "group_automation_update",
                        "args": {
                            "group_id": gid,
                            "by": "user",
                            "ruleset": {
                                "rules": [
                                    {
                                        "id": "once_rule",
                                        "enabled": True,
                                        "scope": "group",
                                        "to": ["@foreman"],
                                        "trigger": {"kind": "at", "at": "2030-01-02T00:00:00Z"},
                                        "action": {"kind": "notify", "message": "hello"},
                                    }
                                ],
                                "snippets": {},
                            },
                        },
                    }
                )
            )
            self.assertTrue(update_resp.ok, getattr(update_resp, "error", None))

            state_after = read_json(state_path)
            self.assertIsInstance(state_after, dict)
            assert isinstance(state_after, dict)
            rule_after = (state_after.get("rules") or {}).get("once_rule") if isinstance(state_after.get("rules"), dict) else {}
            self.assertIsInstance(rule_after, dict)
            assert isinstance(rule_after, dict)
            self.assertNotIn("at_fired", rule_after)
            self.assertNotIn("last_slot_key", rule_after)
        finally:
            cleanup()

    def test_one_time_rule_auto_disables_after_successful_delivery(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request
        from cccc.daemon.automation import AutomationManager
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            gid = self._create_group_id()

            add_actor_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "actor_add",
                        "args": {
                            "group_id": gid,
                            "by": "user",
                            "actor_id": "peer1",
                            "title": "Peer 1",
                            "runtime": "codex",
                            "runner": "pty",
                        },
                    }
                )
            )
            self.assertTrue(add_actor_resp.ok, getattr(add_actor_resp, "error", None))

            at = (datetime.now(timezone.utc) - timedelta(minutes=1)).replace(microsecond=0).isoformat().replace("+00:00", "Z")
            set_rule_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {
                        "op": "group_automation_update",
                        "args": {
                            "group_id": gid,
                            "by": "user",
                            "ruleset": {
                                "rules": [
                                    {
                                        "id": "once_notify",
                                        "enabled": True,
                                        "scope": "group",
                                        "to": ["peer1"],
                                        "trigger": {"kind": "at", "at": at},
                                        "action": {
                                            "kind": "notify",
                                            "message": "fire once",
                                            "priority": "normal",
                                            "requires_ack": False,
                                        },
                                    }
                                ],
                                "snippets": {},
                            },
                        },
                    }
                )
            )
            self.assertTrue(set_rule_resp.ok, getattr(set_rule_resp, "error", None))

            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None

            manager = AutomationManager()
            with patch("cccc.daemon.automation.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
                "cccc.daemon.automation._queue_notify_to_pty", return_value=None
            ):
                manager._check_rules(group, datetime.now(timezone.utc))

            reloaded = load_group(gid)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            automation = reloaded.doc.get("automation") if isinstance(reloaded.doc.get("automation"), dict) else {}
            rules = automation.get("rules") if isinstance(automation.get("rules"), list) else []
            once_rule = None
            for r in rules:
                if isinstance(r, dict) and str(r.get("id") or "") == "once_notify":
                    once_rule = r
                    break
            self.assertIsNotNone(once_rule)
            assert isinstance(once_rule, dict)
            self.assertFalse(bool(once_rule.get("enabled", True)))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
