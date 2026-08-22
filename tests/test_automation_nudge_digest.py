import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from importlib import import_module
from unittest.mock import patch


class TestAutomationReminderNotices(unittest.TestCase):
    def _with_group(self):
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        os.environ["CCCC_HOME"] = td_ctx.__enter__()
        group = create_group(load_registry(), title="reminders")
        add_actor(
            group,
            actor_id="peer1",
            runtime="codex",
            runner="pty",
            enabled=True,
        )
        group.doc["delivery"] = {
            "mail_notice_after_seconds": 1,
            "reply_notice_after_seconds": 1,
        }
        group.save()

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return group, cleanup

    @staticmethod
    def _append_message(group, *, text: str, mode: str):
        from cccc.contracts.v1 import ChatMessageData
        from cccc.kernel.ledger import append_event

        return append_event(
            group.ledger_path,
            kind="chat.message",
            group_id=group.group_id,
            scope_key="",
            by="user",
            data=ChatMessageData(
                text=text,
                to=["peer1"],
                message_mode=mode,
            ).model_dump(),
        )

    @staticmethod
    def _notice_events(group, kind: str):
        from cccc.kernel.inbox import iter_events

        return [
            event
            for event in iter_events(group.ledger_path)
            if str(event.get("kind") or "") == "system.notify"
            and str((event.get("data") or {}).get("kind") or "") == kind
        ]

    def test_mail_batch_emits_one_content_free_notice(self) -> None:
        from cccc.daemon.automation import AutomationManager, _cfg

        group, cleanup = self._with_group()
        try:
            first = self._append_message(group, text="secret first body", mode="mail")
            second = self._append_message(group, text="secret second body", mode="mail")
            manager = AutomationManager()
            due = datetime.now(timezone.utc) + timedelta(seconds=2)

            with patch(
                "cccc.daemon.automation.engine.actor_runtime_running",
                return_value=True,
            ), patch(
                "cccc.daemon.automation.engine._queue_notify_to_pty",
                return_value=True,
            ):
                manager._check_reminders(group, _cfg(group), due)
                manager._check_reminders(group, _cfg(group), due + timedelta(hours=1))

            notices = self._notice_events(group, "mail_notice")
            self.assertEqual(len(notices), 1)
            data = notices[0]["data"]
            self.assertEqual(data["context"]["source_event_ids"], [first["id"], second["id"]])
            self.assertEqual(data["context"]["count"], 2)
            self.assertIn("cccc_inbox_read", data["message"])
            self.assertNotIn("secret first body", data["message"])
            self.assertNotIn("secret second body", data["message"])
        finally:
            cleanup()

    def test_actor_start_begins_a_fresh_mail_notice_window(self) -> None:
        from cccc.daemon.automation import AutomationManager, _cfg
        from cccc.kernel.ledger import append_event
        from cccc.util.time import parse_utc_iso

        group, cleanup = self._with_group()
        try:
            with patch(
                "cccc.contracts.v1.event.utc_now_iso",
                return_value="2020-01-01T00:00:00Z",
            ):
                self._append_message(group, text="old Mail", mode="mail")
            started = append_event(
                group.ledger_path,
                kind="actor.start",
                group_id=group.group_id,
                scope_key="",
                by="user",
                data={"actor_id": "peer1", "runner": "pty"},
            )
            started_at = parse_utc_iso(str(started.get("ts") or ""))
            self.assertIsNotNone(started_at)
            assert started_at is not None
            manager = AutomationManager()

            with patch(
                "cccc.daemon.automation.engine.actor_runtime_running",
                return_value=True,
            ), patch(
                "cccc.daemon.automation.engine._queue_notify_to_pty",
                return_value=True,
            ):
                manager._check_reminders(
                    group,
                    _cfg(group),
                    started_at + timedelta(milliseconds=500),
                )
                self.assertEqual(self._notice_events(group, "mail_notice"), [])
                manager._check_reminders(
                    group,
                    _cfg(group),
                    started_at + timedelta(seconds=2),
                )

            self.assertEqual(len(self._notice_events(group, "mail_notice")), 1)
        finally:
            cleanup()

    def test_mail_arriving_before_batch_closure_shares_the_existing_notice(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.daemon.automation import AutomationManager, _cfg
        from cccc.kernel.ledger import append_event

        group, cleanup = self._with_group()
        try:
            manager = AutomationManager()
            due = datetime.now(timezone.utc) + timedelta(hours=1)

            def reply_to(source_event_id: str) -> None:
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="peer1",
                    data=ChatMessageData(
                        text="handled",
                        to=["user"],
                        reply_to=source_event_id,
                        message_mode="send",
                    ).model_dump(),
                )

            with patch(
                "cccc.daemon.automation.engine.actor_runtime_running",
                return_value=True,
            ), patch(
                "cccc.daemon.automation.engine._queue_notify_to_pty",
                return_value=True,
            ):
                first = self._append_message(group, text="first batch item", mode="mail")
                manager._check_reminders(group, _cfg(group), due)
                self.assertEqual(len(self._notice_events(group, "mail_notice")), 1)

                joined = self._append_message(group, text="joined before closure", mode="mail")
                reply_to(first["id"])
                manager._check_reminders(group, _cfg(group), due + timedelta(hours=1))
                self.assertEqual(
                    len(self._notice_events(group, "mail_notice")),
                    1,
                    "Mail joining an open batch must not create another prompt",
                )

                reply_to(joined["id"])
                next_mail = self._append_message(group, text="next batch item", mode="mail")
                manager._check_reminders(group, _cfg(group), due + timedelta(hours=2))

            notices = self._notice_events(group, "mail_notice")
            self.assertEqual(len(notices), 2)
            self.assertEqual(
                notices[-1]["data"]["context"]["source_event_ids"],
                [next_mail["id"]],
            )
        finally:
            cleanup()

    def test_reply_notice_starts_after_delivery_and_is_one_shot(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.daemon.automation import AutomationManager, _cfg
        from cccc.daemon.messaging.runtime_delivery import append_delivery_state
        from cccc.kernel.actors import find_actor
        from cccc.kernel.ledger import append_event

        group, cleanup = self._with_group()
        try:
            source = self._append_message(
                group,
                text="give me a concrete answer",
                mode="request_reply",
            )
            actor = find_actor(group, "peer1") or {}
            actor_created_at = str(actor.get("created_at") or "")
            manager = AutomationManager()
            future = datetime.now(timezone.utc) + timedelta(hours=1)

            with patch(
                "cccc.daemon.automation.engine.actor_runtime_running",
                return_value=True,
            ), patch(
                "cccc.daemon.automation.engine._queue_notify_to_pty",
                return_value=True,
            ):
                manager._check_reminders(group, _cfg(group), future)
                self.assertEqual(self._notice_events(group, "reply_notice"), [])

                append_delivery_state(
                    group,
                    actor_id="peer1",
                    actor_created_at=actor_created_at,
                    source_event_id=source["id"],
                    state="accepted",
                    transport="test",
                )
                manager._check_reminders(group, _cfg(group), future)
                manager._check_reminders(group, _cfg(group), future + timedelta(hours=1))

            notices = self._notice_events(group, "reply_notice")
            self.assertEqual(len(notices), 1)
            self.assertEqual(notices[0]["data"]["context"]["source_event_ids"], [source["id"]])

            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key="",
                by="peer1",
                data=ChatMessageData(
                    text="answer",
                    to=["user"],
                    reply_to=source["id"],
                    message_mode="send",
                ).model_dump(),
            )
            with patch(
                "cccc.daemon.automation.engine.actor_runtime_running",
                return_value=True,
            ):
                manager._check_reminders(group, _cfg(group), future + timedelta(days=1))
            self.assertEqual(len(self._notice_events(group, "reply_notice")), 1)
        finally:
            cleanup()

    def test_reminder_candidate_loader_reuses_unchanged_source_cache(self) -> None:
        from cccc.daemon.automation import AutomationManager

        group, cleanup = self._with_group()
        try:
            self._append_message(group, text="hello", mode="mail")
            manager = AutomationManager()
            engine_module = import_module("cccc.daemon.automation.engine")
            with patch(
                "cccc.daemon.automation.engine.iter_source_lines",
                wraps=engine_module.iter_source_lines,
            ) as iter_source_lines:
                all_first, chat_first = manager._load_reminder_candidate_events(group)
                all_second, chat_second = manager._load_reminder_candidate_events(group)

            self.assertEqual(len(all_first), 1)
            self.assertEqual(len(chat_first), 1)
            self.assertEqual(len(all_second), 1)
            self.assertEqual(len(chat_second), 1)
            self.assertEqual(iter_source_lines.call_count, 1)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
