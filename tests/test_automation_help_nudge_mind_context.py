import json
import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


def _iso(dt: datetime) -> str:
    return dt.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


class TestAutomationHelpNudgeMindContext(unittest.TestCase):
    def _setup_group(self, *, runner: str = "pty"):
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title="help-nudge")
        add_actor(group, actor_id="peer1", runtime="codex", runner=runner, enabled=True)
        automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
        automation.update(
            {
                "help_nudge_interval_seconds": 60,
                "help_nudge_min_messages": 1,
            }
        )
        group.doc["automation"] = automation
        group.save()
        return group

    def _seed_agent_state(
        self,
        group,
        *,
        focus: str,
        next_action: str,
        what_changed: str,
        environment_summary: str,
        user_model: str,
        persona_notes: str,
        updated_at: str,
    ) -> None:
        from cccc.kernel.context import AgentState, AgentStateHot, AgentStateWarm, AgentsData, ContextStorage

        storage = ContextStorage(group)
        storage.save_agents(
            AgentsData(
                agents=[
                    AgentState(
                        id="peer1",
                        hot=AgentStateHot(
                            active_task_id="T001" if focus else None,
                            focus=focus,
                            next_action=next_action,
                            blockers=[],
                        ),
                        warm=AgentStateWarm(
                            what_changed=what_changed,
                            environment_summary=environment_summary,
                            user_model=user_model,
                            persona_notes=persona_notes,
                        ),
                        updated_at=updated_at,
                    )
                ]
            )
        )

    def _seed_automation_state(self, group, *, now: datetime) -> None:
        state_path = group.path / "state" / "automation.json"
        payload = {
            "v": 5,
            "help_ledger_pos": int(group.ledger_path.stat().st_size),
            "actors": {
                "peer1": {
                    "help_last_nudge_at": _iso(now - timedelta(seconds=600)),
                    "help_msg_count_since": 2,
                    "help_session_key": "sess1",
                }
            },
            "rules": {},
        }
        state_path.write_text(json.dumps(payload), encoding="utf-8")

    def _write_automation_state(self, group, payload: dict) -> None:
        state_path = group.path / "state" / "automation.json"
        state_path.write_text(json.dumps(payload), encoding="utf-8")

    def _load_automation_state(self, group) -> dict:
        state_path = group.path / "state" / "automation.json"
        return json.loads(state_path.read_text(encoding="utf-8"))

    def _latest_notify(self, group):
        lines = [line for line in group.ledger_path.read_text(encoding="utf-8").splitlines() if line.strip()]
        self.assertTrue(lines)
        return json.loads(lines[-1])

    def _append_message(self, group, *, to: list[str], text: str = "ping") -> None:
        from cccc.kernel.ledger import append_event

        append_event(
            group.ledger_path,
            kind="chat.message",
            group_id=group.group_id,
            scope_key="",
            by="user",
            data={"text": text, "format": "plain", "to": to},
        )

    def test_help_nudge_prefers_execution_refresh_when_execution_state_missing(self) -> None:
        from cccc.daemon.automation import AutomationManager, _cfg

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                group = self._setup_group()
                now = datetime(2026, 3, 12, 3, 0, 0, tzinfo=timezone.utc)
                self._seed_agent_state(
                    group,
                    focus="",
                    next_action="",
                    what_changed="",
                    environment_summary="workspace is noisy",
                    user_model="prefers concise evidence",
                    persona_notes="stay low-noise",
                    updated_at=_iso(now),
                )
                self._seed_automation_state(group, now=now)

                manager = AutomationManager()
                cfg = _cfg(group)
                with patch("cccc.daemon.automation.engine.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
                    "cccc.daemon.automation.engine.pty_runner.SUPERVISOR.session_key",
                    return_value="sess1",
                ), patch("cccc.daemon.automation.engine._queue_notify_to_pty", return_value=None):
                    manager._check_help_nudge(group, cfg, now)

                ev = self._latest_notify(group)
                data = ev.get("data") if isinstance(ev.get("data"), dict) else {}
                self.assertEqual(str(data.get("kind") or ""), "help_nudge")
                self.assertIn("focus/next_action/what_changed", str(data.get("message") or ""))
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_help_nudge_switches_to_mind_context_when_execution_is_ready(self) -> None:
        from cccc.daemon.automation import AutomationManager, _cfg

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                group = self._setup_group()
                now = datetime(2026, 3, 12, 3, 10, 0, tzinfo=timezone.utc)
                self._seed_agent_state(
                    group,
                    focus="verify runtime continuity",
                    next_action="review bootstrap result",
                    what_changed="picked up the continuity pass",
                    environment_summary="",
                    user_model="",
                    persona_notes="",
                    updated_at=_iso(now),
                )
                self._seed_automation_state(group, now=now)

                manager = AutomationManager()
                cfg = _cfg(group)
                with patch("cccc.daemon.automation.engine.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
                    "cccc.daemon.automation.engine.pty_runner.SUPERVISOR.session_key",
                    return_value="sess1",
                ), patch("cccc.daemon.automation.engine._queue_notify_to_pty", return_value=None):
                    manager._check_help_nudge(group, cfg, now)

                ev = self._latest_notify(group)
                data = ev.get("data") if isinstance(ev.get("data"), dict) else {}
                self.assertEqual(str(data.get("kind") or ""), "help_nudge")
                self.assertIn("environment_summary/user_model/persona_notes", str(data.get("message") or ""))
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_help_nudge_stays_silent_when_execution_and_mind_context_are_ready(self) -> None:
        from cccc.daemon.automation import AutomationManager, _cfg

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                group = self._setup_group()
                now = datetime(2026, 3, 12, 3, 20, 0, tzinfo=timezone.utc)
                self._seed_agent_state(
                    group,
                    focus="verify runtime continuity",
                    next_action="review bootstrap result",
                    what_changed="picked up the continuity pass",
                    environment_summary="workspace is focused on a single runtime fix",
                    user_model="wants simple and high-ROI moves",
                    persona_notes="preserve continuity and avoid overbuilding",
                    updated_at=_iso(now),
                )
                self._seed_automation_state(group, now=now)
                before = group.ledger_path.read_text(encoding="utf-8")

                manager = AutomationManager()
                cfg = _cfg(group)
                with patch("cccc.daemon.automation.engine.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
                    "cccc.daemon.automation.engine.pty_runner.SUPERVISOR.session_key",
                    return_value="sess1",
                ), patch("cccc.daemon.automation.engine._queue_notify_to_pty", return_value=None):
                    manager._check_help_nudge(group, cfg, now)

                after = group.ledger_path.read_text(encoding="utf-8")
                self.assertEqual(after, before)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_help_nudge_injects_headless_codex_control_turn(self) -> None:
        from cccc.daemon.automation import AutomationManager, _cfg

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                group = self._setup_group(runner="headless")
                now = datetime(2026, 3, 12, 3, 30, 0, tzinfo=timezone.utc)
                self._seed_agent_state(
                    group,
                    focus="",
                    next_action="",
                    what_changed="",
                    environment_summary="workspace is noisy",
                    user_model="prefers concise evidence",
                    persona_notes="stay low-noise",
                    updated_at=_iso(now),
                )
                self._seed_automation_state(group, now=now)

                manager = AutomationManager()
                cfg = _cfg(group)
                with (
                    patch("cccc.daemon.automation.engine.headless_runner.SUPERVISOR.actor_running", return_value=True),
                    patch(
                        "cccc.daemon.automation.engine.headless_runner.SUPERVISOR.get_state",
                        return_value=SimpleNamespace(started_at="sess1"),
                    ),
                    patch("cccc.daemon.codex_app_sessions.SUPERVISOR.actor_running", return_value=True),
                    patch(
                        "cccc.daemon.codex_app_sessions.SUPERVISOR.submit_control_message",
                        return_value=True,
                    ) as submit_control_message,
                ):
                    manager._check_help_nudge(group, cfg, now)

                submit_control_message.assert_called_once()
                kwargs = submit_control_message.call_args.kwargs
                self.assertEqual(kwargs.get("group_id"), group.group_id)
                self.assertEqual(kwargs.get("actor_id"), "peer1")
                self.assertEqual(kwargs.get("control_kind"), "system_notify")

                ev = self._latest_notify(group)
                self.assertEqual(kwargs.get("event_id"), str(ev.get("id") or ""))
                self.assertIn("Refresh collaboration context", str(kwargs.get("text") or ""))
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_help_nudge_counts_new_messages_without_holding_lock_for_ledger_read(self) -> None:
        from cccc.daemon.automation import AutomationManager, _cfg

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                group = self._setup_group()
                now = datetime(2026, 3, 12, 3, 30, 0, tzinfo=timezone.utc)
                self._write_automation_state(
                    group,
                    {
                        "v": 5,
                        "help_ledger_pos": int(group.ledger_path.stat().st_size),
                        "actors": {
                            "peer1": {
                                "help_last_nudge_at": _iso(now),
                                "help_msg_count_since": 0,
                                "help_session_key": "sess1",
                            }
                        },
                        "rules": {},
                    },
                )
                self._append_message(group, to=["peer1"])

                manager = AutomationManager()
                cfg = _cfg(group)
                with patch("cccc.daemon.automation.engine.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
                    "cccc.daemon.automation.engine.pty_runner.SUPERVISOR.session_key",
                    return_value="sess1",
                ), patch("cccc.daemon.automation.engine._queue_notify_to_pty", return_value=None):
                    manager._check_help_nudge(group, cfg, now)

                state = self._load_automation_state(group)
                actor_state = state.get("actors", {}).get("peer1", {})
                self.assertEqual(int(actor_state.get("help_msg_count_since") or 0), 1)
                self.assertEqual(int(state.get("help_ledger_pos") or 0), int(group.ledger_path.stat().st_size))
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_help_nudge_does_not_advance_cursor_on_partial_ledger_line(self) -> None:
        from cccc.daemon.automation import AutomationManager, _cfg

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td
                group = self._setup_group()
                now = datetime(2026, 3, 12, 3, 40, 0, tzinfo=timezone.utc)
                start_pos = int(group.ledger_path.stat().st_size)
                self._write_automation_state(
                    group,
                    {
                        "v": 5,
                        "help_ledger_pos": start_pos,
                        "actors": {
                            "peer1": {
                                "help_last_nudge_at": _iso(now),
                                "help_msg_count_since": 0,
                                "help_session_key": "sess1",
                            }
                        },
                        "rules": {},
                    },
                )
                partial = json.dumps(
                    {
                        "kind": "chat.message",
                        "group_id": group.group_id,
                        "scope_key": "",
                        "by": "user",
                        "data": {"text": "partial", "format": "plain", "to": ["peer1"]},
                    },
                    ensure_ascii=False,
                )
                Path(group.ledger_path).write_text(group.ledger_path.read_text(encoding="utf-8") + partial, encoding="utf-8")

                manager = AutomationManager()
                cfg = _cfg(group)
                with patch("cccc.daemon.automation.engine.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
                    "cccc.daemon.automation.engine.pty_runner.SUPERVISOR.session_key",
                    return_value="sess1",
                ), patch("cccc.daemon.automation.engine._queue_notify_to_pty", return_value=None):
                    manager._check_help_nudge(group, cfg, now)

                state = self._load_automation_state(group)
                actor_state = state.get("actors", {}).get("peer1", {})
                self.assertEqual(int(actor_state.get("help_msg_count_since") or 0), 0)
                self.assertEqual(int(state.get("help_ledger_pos") or 0), start_pos)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
