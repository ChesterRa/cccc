import json
import os
import tempfile
import unittest
from datetime import timedelta


class TestAutomationSilenceActivityFilter(unittest.TestCase):
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

    def test_get_last_group_activity_keeps_business_replies_but_ignores_silence_ack(self) -> None:
        from cccc.contracts.v1 import ChatMessageData, SystemNotifyData
        from cccc.daemon.automation.engine import _get_last_group_activity
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry
        from cccc.util.time import parse_utc_iso

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="silence-filter")
            add_actor(group, actor_id="foreman1", runtime="codex", runner="headless", enabled=True)
            add_actor(group, actor_id="peer1", runtime="codex", runner="headless", enabled=True)

            business = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data=ChatMessageData(text="start work").model_dump(),
            )

            silence_notify = append_event(
                group.ledger_path,
                kind="system.notify",
                group_id=group.group_id,
                scope_key="",
                by="system",
                data=SystemNotifyData(
                    kind="silence_check",
                    title="Group is quiet",
                    message="ping foreman",
                    target_actor_id="foreman1",
                ).model_dump(),
            )
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="foreman1",
                data=ChatMessageData(text="received", reply_to=str(silence_notify.get("id") or "")).model_dump(),
            )

            last_activity = _get_last_group_activity(group)
            self.assertEqual(last_activity, parse_utc_iso(str(business.get("ts") or "")))

            standup_notify = append_event(
                group.ledger_path,
                kind="system.notify",
                group_id=group.group_id,
                scope_key="",
                by="system",
                data=SystemNotifyData(
                    kind="automation",
                    title="Stand-up",
                    message="share progress",
                    target_actor_id="foreman1",
                    context={"rule_id": "standup"},
                ).model_dump(),
            )
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="foreman1",
                data=ChatMessageData(text="standup reply", reply_to=str(standup_notify.get("id") or "")).model_dump(),
            )

            # Foreman's reply to standup should NOT count as activity (prevents auto-idle blocking).
            last_activity = _get_last_group_activity(group)
            self.assertEqual(last_activity, parse_utc_iso(str(business.get("ts") or "")))

            # But foreman's reply to a NON-standup automation rule SHOULD count as activity.
            custom_notify = append_event(
                group.ledger_path,
                kind="system.notify",
                group_id=group.group_id,
                scope_key="",
                by="system",
                data=SystemNotifyData(
                    kind="automation",
                    title="Custom check",
                    message="custom reminder",
                    target_actor_id="foreman1",
                    context={"rule_id": "my_custom_rule"},
                ).model_dump(),
            )
            custom_reply = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="foreman1",
                data=ChatMessageData(text="custom reply", reply_to=str(custom_notify.get("id") or "")).model_dump(),
            )
            last_activity = _get_last_group_activity(group)
            self.assertEqual(last_activity, parse_utc_iso(str(custom_reply.get("ts") or "")))

            peer_business = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="peer1",
                data=ChatMessageData(text="working on it").model_dump(),
            )
            last_activity = _get_last_group_activity(group)
            self.assertEqual(last_activity, parse_utc_iso(str(peer_business.get("ts") or "")))
        finally:
            cleanup()

    def test_check_silence_ignores_foreman_reply_and_reaches_auto_idle(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.daemon.automation import AutomationManager, _cfg
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, get_group_state
        from cccc.kernel.inbox import iter_events
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry
        from cccc.util.time import parse_utc_iso

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="auto-idle")
            add_actor(group, actor_id="foreman1", runtime="codex", runner="headless", enabled=True)
            automation = group.doc.get("automation") if isinstance(group.doc.get("automation"), dict) else {}
            automation["silence_timeout_seconds"] = 1
            group.doc["automation"] = automation
            group.save()

            business = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data=ChatMessageData(text="kickoff").model_dump(),
            )
            business_ts = parse_utc_iso(str(business.get("ts") or ""))
            assert business_ts is not None

            manager = AutomationManager()
            cfg = _cfg(group)

            manager._check_silence(group, cfg, business_ts + timedelta(seconds=2))
            state = json.loads((group.path / "state" / "automation.json").read_text(encoding="utf-8"))
            self.assertEqual(int(state.get("consecutive_silence_count") or 0), 1)
            self.assertEqual(get_group_state(group), "active")

            events = list(iter_events(group.ledger_path))
            silence_notify = next(
                ev
                for ev in reversed(events)
                if str(ev.get("kind") or "") == "system.notify"
                and str(((ev.get("data") or {}) if isinstance(ev.get("data"), dict) else {}).get("kind") or "") == "silence_check"
            )
            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="foreman1",
                data=ChatMessageData(text="checking", reply_to=str(silence_notify.get("id") or "")).model_dump(),
            )

            manager._check_silence(group, cfg, business_ts + timedelta(seconds=4))
            state = json.loads((group.path / "state" / "automation.json").read_text(encoding="utf-8"))
            self.assertEqual(int(state.get("consecutive_silence_count") or 0), 2)
            self.assertEqual(get_group_state(group), "idle")

            events = list(iter_events(group.ledger_path))
            self.assertTrue(
                any(
                    str(ev.get("kind") or "") == "system.notify"
                    and str(((ev.get("data") or {}) if isinstance(ev.get("data"), dict) else {}).get("kind") or "") == "auto_idle"
                    for ev in events
                )
            )
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
