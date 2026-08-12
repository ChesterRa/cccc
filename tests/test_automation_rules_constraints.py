import os
import tempfile
import threading
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
                            "actions": [
                                {
                                    "type": "create_rule",
                                    "rule": {
                                        "id": "bad_group_state_interval",
                                        "enabled": True,
                                        "scope": "group",
                                        "to": ["@foreman"],
                                        "trigger": {"kind": "interval", "every_seconds": 60},
                                        "action": {"kind": "group_state", "state": "paused"},
                                    },
                                }
                            ],
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

    def test_full_ruleset_rejects_empty_and_duplicate_rule_ids(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        _, cleanup = self._with_home()
        try:
            gid = self._create_group_id()
            valid_trigger = {"kind": "interval", "every_seconds": 60}
            invalid_rules = [
                [{"id": "", "trigger": valid_trigger}],
                [
                    {"id": "duplicate", "trigger": valid_trigger},
                    {"id": "duplicate", "trigger": valid_trigger},
                ],
            ]
            for rules in invalid_rules:
                with self.subTest(rules=rules):
                    response, _ = handle_request(
                        DaemonRequest(
                            op="group_automation_update",
                            args={
                                "group_id": gid,
                                "by": "user",
                                "ruleset": {"rules": rules, "snippets": {}},
                            },
                        )
                    )
                    self.assertFalse(response.ok)
                    error = response.error.model_dump() if response.error else {}
                    self.assertEqual(str(error.get("code") or ""), "group_automation_update_failed")
        finally:
            cleanup()

    def test_group_state_active_treats_running_string_false_as_not_running(self) -> None:
        from pathlib import Path
        from cccc.daemon.automation import AutomationManager
        from cccc.kernel.group import Group

        manager = AutomationManager()
        group = Group(group_id="g_test", path=Path("."), doc={})
        loaded = Group(group_id="g_test", path=Path("."), doc={"running": "false"})

        with patch("cccc.daemon.automation.load_group", return_value=loaded), patch.object(
            manager, "_daemon_automation_call", return_value=(True, "")
        ) as mock_call:
            ok, err = manager._execute_group_state_action(group, target_state="active")
            self.assertTrue(ok)
            self.assertEqual(err, "")

        self.assertEqual(mock_call.call_count, 2)
        self.assertEqual(mock_call.call_args_list[0].kwargs.get("op"), "group_start")
        self.assertEqual(mock_call.call_args_list[1].kwargs.get("op"), "group_set_state")

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
            self.assertNotIn("last_fired_at", rule_after)
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

    def test_ruleset_reconcile_serializes_with_scheduler_state_writes(self) -> None:
        from pathlib import Path

        from cccc.contracts.v1 import AutomationRuleSet
        from cccc.daemon.automation import AutomationManager
        from cccc.daemon.automation import automation_ops, engine
        from cccc.kernel.group import Group
        from cccc.util.fs import atomic_write_json, read_json

        td, cleanup = self._with_home()
        try:
            group_path = Path(td) / "groups" / "g_automation_lock"
            (group_path / "state").mkdir(parents=True)
            group = Group(
                group_id="g_automation_lock",
                path=group_path,
                doc={"automation": {"rules": []}},
            )
            state_path = group_path / "state" / "automation.json"
            old_fire = "2020-01-01T00:00:00Z"
            new_fire = "2030-01-01T00:00:00Z"
            atomic_write_json(
                state_path,
                {
                    "v": 5,
                    "rules": {
                        "keep": {"last_fired_at": old_fire},
                        "retired": {"last_fired_at": old_fire},
                    },
                },
            )
            rule = lambda rule_id: {
                "id": rule_id,
                "enabled": True,
                "scope": "group",
                "to": ["@all"],
                "trigger": {"kind": "interval", "every_seconds": 60},
                "action": {"kind": "notify", "message": "tick"},
            }
            previous = AutomationRuleSet.model_validate(
                {"rules": [rule("keep"), rule("retired")], "snippets": {}}
            )
            current = AutomationRuleSet.model_validate(
                {"rules": [rule("keep")], "snippets": {}}
            )
            manager = AutomationManager()
            scheduler_loaded = threading.Event()
            release_scheduler = threading.Event()
            reconcile_done = threading.Event()
            errors: list[BaseException] = []
            real_engine_read = engine.read_json

            def paused_engine_read(path):
                document = real_engine_read(path)
                if Path(path) == state_path and not scheduler_loaded.is_set():
                    scheduler_loaded.set()
                    release_scheduler.wait(timeout=2)
                return document

            def scheduler_write() -> None:
                try:
                    with manager._lock:
                        state = engine._load_state(group)
                        state["rules"]["keep"]["last_fired_at"] = new_fire
                        engine._save_state(group, state)
                except BaseException as exc:  # pragma: no cover - surfaced below
                    errors.append(exc)

            def reconcile() -> None:
                try:
                    automation_ops._reconcile_automation_state_after_ruleset_change(
                        group,
                        previous=previous,
                        current=current,
                    )
                except BaseException as exc:  # pragma: no cover - surfaced below
                    errors.append(exc)
                finally:
                    reconcile_done.set()

            with patch.object(engine, "read_json", side_effect=paused_engine_read):
                scheduler = threading.Thread(target=scheduler_write)
                scheduler.start()
                self.assertTrue(scheduler_loaded.wait(timeout=1))
                updater = threading.Thread(target=reconcile)
                updater.start()
                reconcile_done.wait(timeout=0.1)
                release_scheduler.set()
                scheduler.join(timeout=2)
                updater.join(timeout=2)

            self.assertFalse(scheduler.is_alive())
            self.assertFalse(updater.is_alive())
            self.assertEqual(errors, [])
            final = read_json(state_path)
            self.assertEqual(final["rules"]["keep"]["last_fired_at"], new_fire)
            self.assertNotIn("retired", final["rules"])
        finally:
            cleanup()

    def test_ruleset_update_rolls_back_when_runtime_reconcile_fails(self) -> None:
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.automation import automation_ops
        from cccc.daemon.server import handle_request
        from cccc.kernel.group import load_group
        from cccc.util.fs import atomic_write_json

        _, cleanup = self._with_home()
        try:
            gid = self._create_group_id()

            def ruleset(at: str) -> dict:
                return {
                    "rules": [
                        {
                            "id": "once",
                            "enabled": True,
                            "scope": "group",
                            "to": ["@all"],
                            "trigger": {"kind": "at", "at": at},
                            "action": {"kind": "notify", "message": "once"},
                        }
                    ],
                    "snippets": {},
                }

            first_at = "2030-01-01T00:00:00Z"
            second_at = "2030-01-02T00:00:00Z"
            created, _ = handle_request(
                DaemonRequest(
                    op="group_automation_update",
                    args={"group_id": gid, "by": "user", "ruleset": ruleset(first_at)},
                )
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            group = load_group(gid)
            self.assertIsNotNone(group)
            assert group is not None
            atomic_write_json(
                group.path / "state" / "automation.json",
                {
                    "v": 5,
                    "rules": {
                        "once": {
                            "last_fired_at": first_at,
                            "at_fired": True,
                            "last_slot_key": f"at:{first_at}",
                        }
                    },
                },
            )

            with patch.object(automation_ops, "atomic_write_json", side_effect=OSError("state write failed")):
                failed, _ = handle_request(
                    DaemonRequest(
                        op="group_automation_update",
                        args={"group_id": gid, "by": "user", "ruleset": ruleset(second_at)},
                    )
                )

            self.assertFalse(failed.ok)
            reloaded = load_group(gid)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            self.assertEqual(reloaded.doc["automation"]["rules"][0]["trigger"]["at"], first_at)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
