import json
import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from unittest.mock import patch


class TestAutomationNudgeDigest(unittest.TestCase):
    def test_nudge_scan_is_throttled_between_due_windows(self) -> None:
        from cccc.daemon.automation import AutomationManager, _cfg
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                reg = load_registry()
                group = create_group(reg, title="test")
                add_actor(group, actor_id="peer1", runtime="codex", runner="pty", enabled=True)

                manager = AutomationManager()
                cfg = _cfg(group)
                t0 = datetime.now(timezone.utc)

                with (
                    patch("cccc.daemon.automation.pty_runner.SUPERVISOR.actor_running", return_value=True),
                    patch("cccc.daemon.automation.engine.iter_events", return_value=[]) as iter_events,
                ):
                    manager._check_nudge(group, cfg, t0)
                    manager._check_nudge(group, cfg, t0 + timedelta(seconds=5))
                    manager._check_nudge(group, cfg, t0 + timedelta(seconds=35))

                self.assertEqual(iter_events.call_count, 2)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_min_interval_prevents_repeat_count_growth(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.daemon.automation import AutomationManager, _cfg
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                reg = load_registry()
                group = create_group(reg, title="test")
                add_actor(group, actor_id="peer1", runtime="codex", runner="pty", enabled=True)

                automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
                automation.update(
                    {
                        "nudge_after_seconds": 1,
                        "reply_required_nudge_after_seconds": 0,
                        "attention_ack_nudge_after_seconds": 0,
                        "unread_nudge_after_seconds": 0,
                        "nudge_digest_min_interval_seconds": 120,
                        "nudge_max_repeats_per_obligation": 10,
                        "nudge_escalate_after_repeats": 99,
                    }
                )
                group.doc["automation"] = automation
                group.save()

                msg = append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data=ChatMessageData(
                        text="please do this",
                        to=["peer1"],
                        priority="attention",
                        reply_required=True,
                    ).model_dump(),
                )
                msg_id = str(msg.get("id") or "")
                self.assertTrue(msg_id)

                manager = AutomationManager()
                cfg = _cfg(group)
                t0 = datetime.now(timezone.utc)

                with patch("cccc.daemon.automation.pty_runner.SUPERVISOR.actor_running", return_value=True):
                    manager._check_nudge(group, cfg, t0)

                    state_path = group.path / "state" / "automation.json"
                    self.assertTrue(state_path.exists())
                    st = json.loads(state_path.read_text())
                    count1 = int(
                        st.get("actors", {})
                        .get("peer1", {})
                        .get("nudge_items", {})
                        .get(f"reply_required:{msg_id}", {})
                        .get("count", 0)
                    )
                    self.assertEqual(count1, 1)

                    # Within min interval: no new digest, count must stay the same.
                    manager._check_nudge(group, cfg, t0 + timedelta(seconds=30))
                    st = json.loads(state_path.read_text())
                    count2 = int(
                        st.get("actors", {})
                        .get("peer1", {})
                        .get("nudge_items", {})
                        .get(f"reply_required:{msg_id}", {})
                        .get("count", 0)
                    )
                    self.assertEqual(count2, 1)

                    # After min interval: digest allowed again, count increments.
                    manager._check_nudge(group, cfg, t0 + timedelta(seconds=130))
                    st = json.loads(state_path.read_text())
                    count3 = int(
                        st.get("actors", {})
                        .get("peer1", {})
                        .get("nudge_items", {})
                        .get(f"reply_required:{msg_id}", {})
                        .get("count", 0)
                    )
                    self.assertEqual(count3, 2)
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
