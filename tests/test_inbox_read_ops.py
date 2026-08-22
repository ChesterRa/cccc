import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestInboxReadOps(unittest.TestCase):
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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _create_group_with_actor(
        self, *, actor_id: str = "peer1", runtime: str = "codex"
    ) -> str:
        created, _ = self._call(
            "group_create",
            {"title": "inbox-read", "topic": "", "by": "user"},
        )
        self.assertTrue(created.ok, getattr(created, "error", None))
        group_id = str((created.result or {}).get("group_id") or "").strip()
        added, _ = self._call(
            "actor_add",
            {
                "group_id": group_id,
                "actor_id": actor_id,
                "runtime": runtime,
                "runner": "headless",
                "by": "user",
            },
        )
        self.assertTrue(added.ok, getattr(added, "error", None))
        return group_id

    def _send_mail(self, group_id: str, text: str, *, actor_id: str = "peer1"):
        sent, _ = self._call(
            "send",
            {
                "group_id": group_id,
                "by": "user",
                "to": [actor_id],
                "text": text,
                "message_mode": "mail",
            },
        )
        self.assertTrue(sent.ok, getattr(sent, "error", None))
        return (sent.result or {})["event"]

    def test_malformed_cursor_store_fails_closed_without_overwrite(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            self._send_mail(group_id, "hello")
            cursor_path = (
                Path(home) / "groups" / group_id / "state" / "read_cursors.json"
            )
            cursor_path.parent.mkdir(parents=True, exist_ok=True)
            malformed = b"{malformed"
            cursor_path.write_bytes(malformed)

            response, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 1,
                    "by": "peer1",
                },
            )

            self.assertFalse(response.ok)
            self.assertEqual(cursor_path.read_bytes(), malformed)
        finally:
            cleanup()

    def test_user_has_message_history_but_no_mail_inbox(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            sent, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "peer1",
                    "to": ["user"],
                    "text": "visible to the user",
                    "message_mode": "send",
                },
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))

            for op in ("inbox_peek", "inbox_read"):
                with self.subTest(op=op):
                    response, _ = self._call(
                        op,
                        {
                            "group_id": group_id,
                            "actor_id": "user",
                            "by": "user",
                        },
                    )
                    self.assertFalse(response.ok)
                    self.assertEqual(
                        getattr(response.error, "code", ""),
                        "invalid_inbox_recipient",
                    )

            history, _ = self._call(
                "message_history",
                {
                    "group_id": group_id,
                    "actor_id": "user",
                    "by": "user",
                },
            )
            self.assertTrue(history.ok, getattr(history, "error", None))
            self.assertEqual(
                [item.get("id") for item in (history.result or {}).get("messages", [])],
                [(sent.result or {}).get("event", {}).get("id")],
            )
        finally:
            cleanup()

    def test_ledger_failure_keeps_the_batch_unread(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            event = self._send_mail(group_id, "keep unread")

            with patch(
                "cccc.daemon.messaging.inbox_read_ops.append_event",
                side_effect=OSError("injected ledger failure"),
            ):
                response, _ = self._call(
                    "inbox_read",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "limit": 1,
                        "by": "peer1",
                    },
                )

            self.assertFalse(response.ok)
            peek, _ = self._call(
                "inbox_peek",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertTrue(peek.ok, getattr(peek, "error", None))
            self.assertEqual(
                [item.get("id") for item in (peek.result or {}).get("messages", [])],
                [event["id"]],
            )
            self.assertFalse(
                (Path(home) / "groups" / group_id / "state" / "read_cursors.pending.json").exists()
            )
        finally:
            cleanup()

    def test_cursor_write_failure_recovers_from_mail_read_without_replaying_bodies(self) -> None:
        from cccc.kernel.inbox import iter_events
        from cccc.kernel.group import load_group

        home, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            event = self._send_mail(group_id, "commit once")
            with patch(
                "cccc.kernel.inbox._save_cursors",
                side_effect=OSError("injected cursor failure"),
            ):
                failed, _ = self._call(
                    "inbox_read",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "limit": 1,
                        "by": "peer1",
                    },
                )

            self.assertFalse(failed.ok)
            pending_path = (
                Path(home) / "groups" / group_id / "state" / "read_cursors.pending.json"
            )
            self.assertTrue(pending_path.exists())
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            reads = [
                item
                for item in iter_events(group.ledger_path)
                if item.get("kind") == "mail.read"
            ]
            self.assertEqual(len(reads), 1)
            self.assertEqual(reads[0]["data"]["event_id"], event["id"])

            recovered, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 1,
                    "by": "peer1",
                },
            )
            self.assertTrue(recovered.ok, getattr(recovered, "error", None))
            self.assertEqual((recovered.result or {})["messages"], [])
            self.assertEqual((recovered.result or {})["cursor"]["event_id"], event["id"])
            self.assertFalse(pending_path.exists())
            reads = [
                item
                for item in iter_events(group.ledger_path)
                if item.get("kind") == "mail.read"
            ]
            self.assertEqual(len(reads), 1)
        finally:
            cleanup()

    def test_read_consumes_ordered_mail_batches_and_writes_mail_read(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            first = self._send_mail(group_id, "first")
            second = self._send_mail(group_id, "second")

            read_first, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 1,
                    "by": "peer1",
                },
            )
            self.assertTrue(read_first.ok, getattr(read_first, "error", None))
            self.assertEqual(
                [item["id"] for item in (read_first.result or {})["messages"]],
                [first["id"]],
            )
            self.assertEqual((read_first.result or {})["event"]["kind"], "mail.read")
            self.assertTrue((read_first.result or {})["cursor"]["updated_at"])
            self.assertEqual(
                (read_first.result or {})["event"]["data"]["event_id"], first["id"]
            )

            peek, _ = self._call(
                "inbox_peek",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [item["id"] for item in (peek.result or {})["messages"]],
                [second["id"]],
            )

            read_second, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [item["id"] for item in (read_second.result or {})["messages"]],
                [second["id"]],
            )
            empty, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual((empty.result or {})["messages"], [])
            self.assertIsNone((empty.result or {})["event"])
            self.assertEqual(
                (empty.result or {})["cursor"],
                (read_second.result or {})["cursor"],
            )
        finally:
            cleanup()

    def test_natural_pending_summary_tracks_unread_mail_after_reply_and_push(self) -> None:
        from cccc.contracts.v1 import ChatMessageData
        from cccc.daemon.messaging.runtime_delivery import append_delivery_state
        from cccc.kernel.actors import find_actor
        from cccc.kernel.group import load_group
        from cccc.kernel.inbox import mail_pending_summary
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            mail = self._send_mail(group_id, "read this later")
            group = load_group(group_id)
            assert group is not None
            actor = find_actor(group, "peer1")
            assert actor is not None

            append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group_id,
                scope_key="",
                by="peer1",
                data=ChatMessageData(
                    text="I handled the urgent part",
                    to=["user"],
                    reply_to=str(mail["id"]),
                    message_mode="send",
                ).model_dump(),
            )
            append_delivery_state(
                group,
                actor_id="peer1",
                actor_created_at=str(actor.get("created_at") or ""),
                source_event_id=str(mail["id"]),
                state="accepted",
                transport="test",
            )

            self.assertEqual(
                mail_pending_summary(group, actor_id="peer1")["count"], 1
            )
            consumed, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertTrue(consumed.ok, getattr(consumed, "error", None))
            self.assertEqual(
                [event["id"] for event in (consumed.result or {})["messages"]],
                [mail["id"]],
            )
            self.assertEqual(mail_pending_summary(group, actor_id="peer1"), {})
        finally:
            cleanup()

    def test_pre_mail_cursor_document_is_ignored(self) -> None:
        home, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            first = self._send_mail(group_id, "first")
            second = self._send_mail(group_id, "second")
            cursor_path = (
                Path(home) / "groups" / group_id / "state" / "read_cursors.json"
            )
            cursor_path.parent.mkdir(parents=True, exist_ok=True)
            cursor_path.write_text(
                '{"peer1":{"event_id":"%s","ts":"%s"}}'
                % (second["id"], second["ts"]),
                encoding="utf-8",
            )

            peek, _ = self._call(
                "inbox_peek",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertTrue(peek.ok, getattr(peek, "error", None))
            self.assertEqual(
                [event["id"] for event in (peek.result or {})["messages"]],
                [first["id"], second["id"]],
            )
            self.assertEqual((peek.result or {})["cursor"], {"event_id": "", "ts": ""})
        finally:
            cleanup()

    def test_read_requires_the_actor_or_user_authority(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            self._send_mail(group_id, "private")
            rejected, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 1,
                    "by": "peer2",
                },
            )
            self.assertFalse(rejected.ok)
            self.assertEqual(getattr(rejected.error, "code", ""), "permission_denied")
        finally:
            cleanup()

    def test_internal_actor_requires_an_explicit_recipient(self) -> None:
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.inbox import is_message_for_actor
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            group = create_group(load_registry(), title="internal-routing", topic="")
            add_actor(
                group,
                actor_id="peer1",
                title="Peer 1",
                runtime="codex",
                runner="headless",
            )
            group.doc["actors"].append(
                {
                    "id": "internal-helper",
                    "title": "Internal Helper",
                    "internal_kind": "legacy",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": True,
                }
            )
            group.save()

            for recipients in (["@peers"], ["@all"], []):
                event = {
                    "kind": "chat.message",
                    "by": "user",
                    "data": {
                        "to": recipients,
                        "text": "broad",
                        "message_mode": "mail",
                    },
                }
                self.assertFalse(
                    is_message_for_actor(
                        group, actor_id="internal-helper", event=event
                    )
                )
            direct = {
                "kind": "chat.message",
                "by": "user",
                "data": {
                    "to": ["internal-helper"],
                    "text": "direct",
                    "message_mode": "mail",
                },
            }
            self.assertTrue(
                is_message_for_actor(group, actor_id="internal-helper", event=direct)
            )
        finally:
            cleanup()

    def test_cursor_and_status_follow_append_order_when_timestamps_regress(self) -> None:
        from cccc.contracts.v1.event import Event as ContractEvent
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.inbox import (
            get_obligation_status_batch,
            get_read_status_batch,
            unread_messages,
        )
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            group = create_group(load_registry(), title="ledger-order", topic="")
            add_actor(
                group,
                actor_id="peer1",
                title="Peer 1",
                runtime="codex",
                runner="headless",
            )
            timestamps = iter(
                [
                    "2099-01-01T00:00:01Z",
                    "2099-01-01T00:00:01Z",
                    "2099-01-01T00:00:00Z",
                ]
            )

            def fixed_event(**kwargs):
                return ContractEvent(ts=next(timestamps), **kwargs)

            with patch("cccc.kernel.ledger.Event", side_effect=fixed_event):
                events = [
                    append_event(
                        group.ledger_path,
                        kind="chat.message",
                        group_id=group.group_id,
                        scope_key="",
                        by="user",
                        data={
                            "text": text,
                            "to": ["peer1"],
                            "message_mode": "mail",
                        },
                    )
                    for text in ("first", "same timestamp", "clock regressed")
                ]

            consumed, _ = self._call(
                "inbox_read",
                {
                    "group_id": group.group_id,
                    "actor_id": "peer1",
                    "limit": 1,
                    "by": "peer1",
                },
            )
            self.assertTrue(consumed.ok, getattr(consumed, "error", None))
            self.assertEqual(
                [event["id"] for event in (consumed.result or {})["messages"]],
                [events[0]["id"]],
            )
            self.assertEqual(
                [event["id"] for event in unread_messages(group, actor_id="peer1")],
                [events[1]["id"], events[2]["id"]],
            )
            self.assertFalse(
                get_read_status_batch(group, events[1:])[events[2]["id"]]["peer1"]
            )
            self.assertNotIn(
                "read",
                get_obligation_status_batch(group, [events[2]])[events[2]["id"]][
                    "peer1"
                ],
            )
        finally:
            cleanup()

    def test_actor_generation_excludes_messages_before_latest_add(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.ledger import append_event

        _, cleanup = self._with_home()
        try:
            created, _ = self._call(
                "group_create",
                {"title": "actor-generation", "topic": "", "by": "user"},
            )
            group_id = str((created.result or {})["group_id"])
            group = load_group(group_id)
            assert group is not None
            before = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group_id,
                scope_key="",
                by="user",
                data={
                    "text": "before actor",
                    "to": ["peer1"],
                    "message_mode": "mail",
                },
            )
            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(added.ok, getattr(added, "error", None))
            group = load_group(group_id)
            assert group is not None
            after = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group_id,
                scope_key="",
                by="user",
                data={
                    "text": "after actor",
                    "to": ["peer1"],
                    "message_mode": "mail",
                },
            )

            peek, _ = self._call(
                "inbox_peek",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [event["id"] for event in (peek.result or {})["messages"]],
                [after["id"]],
            )
            self.assertNotEqual(before["id"], after["id"])

            removed, _ = self._call(
                "actor_remove",
                {"group_id": group_id, "actor_id": "peer1", "by": "user"},
            )
            self.assertTrue(removed.ok, getattr(removed, "error", None))
            readded, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(readded.ok, getattr(readded, "error", None))
            peek_after_recreate, _ = self._call(
                "inbox_peek",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual((peek_after_recreate.result or {})["messages"], [])
        finally:
            cleanup()

    def test_mail_cursor_can_consume_through_a_later_mail(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor(
                actor_id="deepseek", runtime="deepseek"
            )
            events = [
                self._send_mail(group_id, text, actor_id="deepseek")
                for text in ("first", "second", "third")
            ]
            consumed, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "deepseek",
                    "limit": 2,
                    "by": "deepseek",
                },
            )
            self.assertTrue(consumed.ok, getattr(consumed, "error", None))
            cursor = (consumed.result or {})["cursor"]
            self.assertEqual(cursor["event_id"], events[1]["id"])
            unread, _ = self._call(
                "inbox_peek",
                {
                    "group_id": group_id,
                    "actor_id": "deepseek",
                    "limit": 10,
                    "by": "deepseek",
                },
            )
            self.assertEqual(
                [event["id"] for event in (unread.result or {})["messages"]],
                [events[2]["id"]],
            )
        finally:
            cleanup()

    def test_inbox_is_mail_only_and_history_is_non_consuming(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            sent_events = []
            for mode, text in (
                ("send", "direct update"),
                ("request_reply", "please answer"),
                ("mail", "read later"),
            ):
                sent, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "to": ["peer1"],
                        "text": text,
                        "message_mode": mode,
                    },
                )
                self.assertTrue(sent.ok, getattr(sent, "error", None))
                sent_events.append((sent.result or {})["event"])

            peek, _ = self._call(
                "inbox_peek",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [event["id"] for event in (peek.result or {})["messages"]],
                [sent_events[2]["id"]],
            )
            self.assertEqual(set((peek.result or {})["cursor"]), {"event_id", "ts"})

            history, _ = self._call(
                "message_history",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertTrue(history.ok, getattr(history, "error", None))
            self.assertEqual(
                [event["id"] for event in (history.result or {})["messages"]],
                [event["id"] for event in reversed(sent_events)],
            )

            first_page, _ = self._call(
                "message_history",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 2,
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [event["id"] for event in (first_page.result or {})["messages"]],
                [sent_events[2]["id"], sent_events[1]["id"]],
            )
            self.assertTrue((first_page.result or {})["has_more"])

            older, _ = self._call(
                "message_history",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "before_event_id": sent_events[2]["id"],
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [event["id"] for event in (older.result or {})["messages"]],
                [sent_events[1]["id"], sent_events[0]["id"]],
            )
            self.assertFalse((older.result or {})["has_more"])

            searched, _ = self._call(
                "message_history",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "query": "PLEASE ANSWER",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [event["id"] for event in (searched.result or {})["messages"]],
                [sent_events[1]["id"]],
            )

            history_after, _ = self._call(
                "message_history",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "mode": "send",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [event["id"] for event in (history_after.result or {})["messages"]],
                [sent_events[0]["id"]],
            )
            peek_again, _ = self._call(
                "inbox_peek",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "limit": 10,
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [event["id"] for event in (peek_again.result or {})["messages"]],
                [sent_events[2]["id"]],
            )
        finally:
            cleanup()

    def test_message_history_is_bounded_to_the_current_actor_generation(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            old, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "old generation",
                    "message_mode": "send",
                },
            )
            self.assertTrue(old.ok, getattr(old, "error", None))
            removed, _ = self._call(
                "actor_remove",
                {"group_id": group_id, "actor_id": "peer1", "by": "user"},
            )
            self.assertTrue(removed.ok, getattr(removed, "error", None))
            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(added.ok, getattr(added, "error", None))
            current, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "current generation",
                    "message_mode": "send",
                },
            )
            history, _ = self._call(
                "message_history",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "peer1",
                },
            )
            self.assertEqual(
                [event["id"] for event in (history.result or {})["messages"]],
                [(current.result or {})["event"]["id"]],
            )
        finally:
            cleanup()

    def test_inbox_and_history_reject_non_integer_limits(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor()
            for op, maximum in (("inbox_peek", 200), ("inbox_read", 200), ("message_history", 100)):
                for invalid in (True, "2", 1.5, 0, maximum + 1):
                    response, _ = self._call(
                        op,
                        {
                            "group_id": group_id,
                            "actor_id": "peer1",
                            "by": "peer1",
                            "limit": invalid,
                        },
                    )
                    self.assertFalse(response.ok, (op, invalid, response.result))
                    self.assertEqual(response.error.code, "invalid_limit")
        finally:
            cleanup()

    def test_deepseek_inbox_read_consumes_the_returned_prefix(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group_with_actor(
                actor_id="deepseek", runtime="deepseek"
            )
            events = [
                self._send_mail(group_id, text, actor_id="deepseek")
                for text in ("first", "second", "third")
            ]

            response, _ = self._call(
                "inbox_read",
                {
                    "group_id": group_id,
                    "actor_id": "deepseek",
                    "limit": 3,
                    "by": "deepseek",
                },
            )
            self.assertTrue(response.ok, getattr(response, "error", None))
            self.assertEqual(
                [event["id"] for event in (response.result or {})["messages"]],
                [event["id"] for event in events],
            )
            self.assertEqual(
                (response.result or {})["cursor"]["event_id"], events[-1]["id"]
            )
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
