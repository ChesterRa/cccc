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

    def test_malformed_cursor_store_fails_closed_without_overwrite(self) -> None:
        home, cleanup = self._with_home()
        try:
            create, _ = self._call(
                "group_create", {"title": "cursor-corruption", "topic": "", "by": "user"}
            )
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "pty",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))
            sent, _ = self._call(
                "send",
                {"group_id": group_id, "by": "user", "to": ["peer1"], "text": "hello"},
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))
            event_id = str((((sent.result or {}).get("event") or {}).get("id")) or "")
            self.assertTrue(event_id)
            cursor_path = Path(home) / "groups" / group_id / "state" / "read_cursors.json"
            cursor_path.parent.mkdir(parents=True, exist_ok=True)
            malformed = b"{malformed"
            cursor_path.write_bytes(malformed)

            response, _ = self._call(
                "inbox_mark_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": event_id,
                    "by": "peer1",
                },
            )

            self.assertFalse(response.ok)
            self.assertEqual(cursor_path.read_bytes(), malformed)
        finally:
            cleanup()

    def test_inbox_mark_read_ledger_failure_keeps_the_message_unread(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call(
                "group_create", {"title": "cursor-rollback", "topic": "", "by": "user"}
            )
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "codex",
                    "runner": "pty",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))
            sent, _ = self._call(
                "send",
                {"group_id": group_id, "by": "user", "to": ["peer1"], "text": "keep unread"},
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))
            event_id = str((((sent.result or {}).get("event") or {}).get("id")) or "")

            with patch(
                "cccc.daemon.messaging.inbox_read_ops.append_event",
                side_effect=OSError("injected ledger failure"),
            ):
                response, _ = self._call(
                    "inbox_mark_read",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "event_id": event_id,
                        "by": "peer1",
                    },
                )

            self.assertFalse(response.ok)
            inbox, _ = self._call(
                "inbox_list",
                {"group_id": group_id, "actor_id": "peer1", "by": "peer1"},
            )
            self.assertTrue(inbox.ok, getattr(inbox, "error", None))
            self.assertEqual(
                [item.get("id") for item in (inbox.result or {}).get("messages", [])],
                [event_id],
            )
        finally:
            cleanup()

    def test_inbox_mark_read_emits_chat_ack_for_attention(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call(
                "group_create", {"title": "inbox-read", "topic": "", "by": "user"}
            )
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call(
                "attach", {"group_id": group_id, "path": ".", "by": "user"}
            )
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            sent, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "attention ping",
                    "priority": "attention",
                },
            )
            self.assertTrue(sent.ok, getattr(sent, "error", None))
            sent_event = (
                (sent.result or {}).get("event")
                if isinstance(sent.result, dict)
                else {}
            )
            self.assertIsInstance(sent_event, dict)
            assert isinstance(sent_event, dict)
            event_id = str(sent_event.get("id") or "").strip()
            self.assertTrue(event_id)

            inbox, _ = self._call(
                "inbox_list",
                {"group_id": group_id, "actor_id": "peer1", "by": "peer1", "limit": 10},
            )
            self.assertTrue(inbox.ok, getattr(inbox, "error", None))
            messages = (
                (inbox.result or {}).get("messages")
                if isinstance(inbox.result, dict)
                else []
            )
            self.assertIsInstance(messages, list)
            assert isinstance(messages, list)
            self.assertTrue(
                any(
                    str(item.get("id") or "") == event_id
                    for item in messages
                    if isinstance(item, dict)
                )
            )

            marked, _ = self._call(
                "inbox_mark_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": event_id,
                    "by": "peer1",
                },
            )
            self.assertTrue(marked.ok, getattr(marked, "error", None))
            ack_event = (
                (marked.result or {}).get("ack_event")
                if isinstance(marked.result, dict)
                else {}
            )
            self.assertIsInstance(ack_event, dict)
            assert isinstance(ack_event, dict)
            self.assertEqual(str(ack_event.get("kind") or ""), "chat.ack")
        finally:
            cleanup()

    def test_chat_ack_idempotent_and_mark_all_read(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call(
                "group_create", {"title": "inbox-ack", "topic": "", "by": "user"}
            )
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            attach, _ = self._call(
                "attach", {"group_id": group_id, "path": ".", "by": "user"}
            )
            self.assertTrue(attach.ok, getattr(attach, "error", None))

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            attention, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "attention task",
                    "priority": "attention",
                },
            )
            self.assertTrue(attention.ok, getattr(attention, "error", None))
            attention_event = (
                (attention.result or {}).get("event")
                if isinstance(attention.result, dict)
                else {}
            )
            self.assertIsInstance(attention_event, dict)
            assert isinstance(attention_event, dict)
            attention_event_id = str(attention_event.get("id") or "").strip()
            self.assertTrue(attention_event_id)

            impersonated, _ = self._call(
                "chat_ack",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": attention_event_id,
                    "by": "user",
                },
            )
            self.assertFalse(impersonated.ok)
            self.assertEqual(
                getattr(impersonated.error, "code", ""), "permission_denied"
            )

            ack1, _ = self._call(
                "chat_ack",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": attention_event_id,
                    "by": "peer1",
                },
            )
            self.assertTrue(ack1.ok, getattr(ack1, "error", None))
            self.assertFalse(bool((ack1.result or {}).get("already")))

            ack2, _ = self._call(
                "chat_ack",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": attention_event_id,
                    "by": "peer1",
                },
            )
            self.assertTrue(ack2.ok, getattr(ack2, "error", None))
            self.assertTrue(bool((ack2.result or {}).get("already")))

            normal, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "normal ping",
                },
            )
            self.assertTrue(normal.ok, getattr(normal, "error", None))

            mark_all, _ = self._call(
                "inbox_mark_all_read",
                {"group_id": group_id, "actor_id": "peer1", "by": "peer1"},
            )
            self.assertTrue(mark_all.ok, getattr(mark_all, "error", None))
            mark_event = (
                (mark_all.result or {}).get("event")
                if isinstance(mark_all.result, dict)
                else {}
            )
            self.assertIsInstance(mark_event, dict)
            assert isinstance(mark_event, dict)
            self.assertEqual(str(mark_event.get("kind") or ""), "chat.read")

            inbox, _ = self._call(
                "inbox_list",
                {"group_id": group_id, "actor_id": "peer1", "by": "peer1", "limit": 10},
            )
            self.assertTrue(inbox.ok, getattr(inbox, "error", None))
            messages = (
                (inbox.result or {}).get("messages")
                if isinstance(inbox.result, dict)
                else []
            )
            self.assertIsInstance(messages, list)
            assert isinstance(messages, list)
            self.assertEqual(messages, [])
        finally:
            cleanup()

    def test_kind_filter_is_validated_and_applied_before_limit_or_mark_all(
        self,
    ) -> None:
        _, cleanup = self._with_home()
        try:

            def create_peer_group(title: str) -> str:
                created, _ = self._call(
                    "group_create", {"title": title, "topic": "", "by": "user"}
                )
                self.assertTrue(created.ok, getattr(created, "error", None))
                group_id = str((created.result or {}).get("group_id") or "").strip()
                stopped, _ = self._call(
                    "group_stop", {"group_id": group_id, "by": "user"}
                )
                self.assertTrue(stopped.ok, getattr(stopped, "error", None))
                added, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "runtime": "custom",
                        "runner": "pty",
                        "command": ["sh", "-c", "exit 0"],
                        "by": "user",
                    },
                )
                self.assertTrue(added.ok, getattr(added, "error", None))
                return group_id

            first_group = create_peer_group("filter before limit")
            notified, _ = self._call(
                "system_notify",
                {
                    "group_id": first_group,
                    "by": "system",
                    "title": "notify first",
                    "message": "notify first",
                    "target_actor_id": "peer1",
                },
            )
            chatted, _ = self._call(
                "send",
                {
                    "group_id": first_group,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "chat second",
                },
            )
            self.assertTrue(notified.ok, getattr(notified, "error", None))
            self.assertTrue(chatted.ok, getattr(chatted, "error", None))
            chat_id = str(((chatted.result or {}).get("event") or {}).get("id") or "")
            chat_page, _ = self._call(
                "inbox_list",
                {
                    "group_id": first_group,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "kind_filter": "chat",
                    "limit": 1,
                },
            )
            self.assertTrue(chat_page.ok, getattr(chat_page, "error", None))
            self.assertEqual(
                [
                    str(item.get("id") or "")
                    for item in (chat_page.result or {}).get("messages", [])
                ],
                [chat_id],
            )
            invalid_list, _ = self._call(
                "inbox_list",
                {
                    "group_id": first_group,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "kind_filter": "bogus",
                },
            )
            self.assertFalse(invalid_list.ok)
            self.assertEqual(
                getattr(invalid_list.error, "code", ""), "invalid_kind_filter"
            )

            second_group = create_peer_group("filtered mark all")
            first_chat, _ = self._call(
                "send",
                {
                    "group_id": second_group,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "chat first",
                },
            )
            later_notify, _ = self._call(
                "system_notify",
                {
                    "group_id": second_group,
                    "by": "system",
                    "title": "notify second",
                    "message": "notify second",
                    "target_actor_id": "peer1",
                },
            )
            chat_id = str(
                ((first_chat.result or {}).get("event") or {}).get("id") or ""
            )
            notify_id = str(
                ((later_notify.result or {}).get("event") or {}).get("id") or ""
            )
            invalid_mark, _ = self._call(
                "inbox_mark_all_read",
                {
                    "group_id": second_group,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "kind_filter": "bogus",
                },
            )
            self.assertFalse(invalid_mark.ok)
            self.assertEqual(
                getattr(invalid_mark.error, "code", ""), "invalid_kind_filter"
            )
            marked, _ = self._call(
                "inbox_mark_all_read",
                {
                    "group_id": second_group,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "kind_filter": "chat",
                },
            )
            self.assertTrue(marked.ok, getattr(marked, "error", None))
            self.assertEqual(
                ((marked.result or {}).get("cursor") or {}).get("event_id"), chat_id
            )
            remaining, _ = self._call(
                "inbox_list",
                {
                    "group_id": second_group,
                    "actor_id": "peer1",
                    "by": "peer1",
                    "kind_filter": "all",
                    "limit": 10,
                },
            )
            self.assertEqual(
                [
                    str(item.get("id") or "")
                    for item in (remaining.result or {}).get("messages", [])
                ],
                [notify_id],
            )
        finally:
            cleanup()

    def test_internal_actor_does_not_match_peer_or_broadcast_chat_targets(self) -> None:
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.inbox import is_message_for_actor
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            registry = load_registry()
            group_id = create_group(
                registry, title="internal-routing", topic=""
            ).group_id
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            add_actor(
                group, actor_id="lead", title="Lead", runtime="codex", runner="headless"
            )  # type: ignore[arg-type]
            add_actor(
                group,
                actor_id="peer1",
                title="Peer 1",
                runtime="codex",
                runner="headless",
            )  # type: ignore[arg-type]
            actors = (
                group.doc.get("actors")
                if isinstance(group.doc.get("actors"), list)
                else []
            )
            actors.append(
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

            peers_event = {
                "kind": "chat.message",
                "by": "lead",
                "data": {"to": ["@peers"], "text": "peer ping"},
            }
            all_event = {
                "kind": "chat.message",
                "by": "lead",
                "data": {"to": ["@all"], "text": "all ping"},
            }
            broadcast_event = {
                "kind": "chat.message",
                "by": "lead",
                "data": {"text": "broadcast ping"},
            }
            direct_event = {
                "kind": "chat.message",
                "by": "lead",
                "data": {"to": ["internal-helper"], "text": "direct ping"},
            }

            self.assertTrue(
                is_message_for_actor(group, actor_id="peer1", event=peers_event)
            )
            self.assertFalse(
                is_message_for_actor(
                    group, actor_id="internal-helper", event=peers_event
                )
            )
            self.assertFalse(
                is_message_for_actor(group, actor_id="internal-helper", event=all_event)
            )
            self.assertFalse(
                is_message_for_actor(
                    group, actor_id="internal-helper", event=broadcast_event
                )
            )
            self.assertTrue(
                is_message_for_actor(
                    group, actor_id="internal-helper", event=direct_event
                )
            )
        finally:
            cleanup()

    def test_read_cursor_follows_ledger_order_when_timestamps_collide_or_regress(
        self,
    ) -> None:
        from cccc.contracts.v1.event import Event as ContractEvent
        from cccc.kernel.actors import add_actor, list_actors
        from cccc.kernel.group import create_group
        from cccc.kernel.inbox import (
            batch_unread_counts,
            get_cursor,
            get_indexed_unread_counts,
            get_obligation_status_batch,
            get_read_status,
            get_read_status_batch,
            latest_unread_event,
            set_cursor,
            unread_count,
            unread_messages,
        )
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry

        _, cleanup = self._with_home()
        try:
            group = create_group(load_registry(), title="cursor-ledger-order", topic="")
            add_actor(
                group,
                actor_id="peer1",
                title="Peer 1",
                runtime="codex",
                runner="headless",
            )  # type: ignore[arg-type]
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
                            "priority": "attention"
                            if text == "clock moved backwards"
                            else "normal",
                        },
                    )
                    for text in ("first", "same timestamp", "clock moved backwards")
                ]

            set_cursor(group, "peer1", event_id=events[0]["id"], ts=events[0]["ts"])
            self.assertEqual(
                [event["id"] for event in unread_messages(group, actor_id="peer1")],
                [events[1]["id"], events[2]["id"]],
            )
            self.assertEqual(unread_count(group, actor_id="peer1"), 2)
            self.assertEqual(
                batch_unread_counts(group, actor_ids=["peer1"]), {"peer1": 2}
            )
            self.assertEqual(
                (latest_unread_event(group, actor_id="peer1") or {}).get("id"),
                events[2]["id"],
            )
            self.assertEqual(
                get_read_status(group, events[1]["id"]).get("peer1"), False
            )
            self.assertEqual(
                get_read_status_batch(group, events[1:])[events[2]["id"]].get("peer1"),
                False,
            )
            obligation = get_obligation_status_batch(group, [events[2]])[
                events[2]["id"]
            ]["peer1"]
            self.assertEqual(obligation.get("read"), False)
            self.assertEqual(obligation.get("acked"), False)

            with patch(
                "cccc.kernel.inbox.iter_events",
                side_effect=AssertionError("cursor advance must use the ledger index"),
            ):
                set_cursor(group, "peer1", event_id=events[2]["id"], ts=events[2]["ts"])
            self.assertEqual(get_cursor(group, "peer1")[0], events[2]["id"])
            self.assertEqual(unread_messages(group, actor_id="peer1"), [])
            self.assertEqual(
                get_indexed_unread_counts(group, actors=list_actors(group)).get(
                    "peer1"
                ),
                0,
            )

            with patch(
                "cccc.kernel.ledger.Event",
                side_effect=lambda **kwargs: ContractEvent(
                    ts="2098-12-31T23:59:59Z", **kwargs
                ),
            ):
                later_in_ledger = append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data={
                        "text": "later append after another clock regression",
                        "to": ["peer1"],
                    },
                )
            self.assertEqual(
                get_indexed_unread_counts(group, actors=list_actors(group)).get(
                    "peer1"
                ),
                1,
            )
            self.assertEqual(
                [event["id"] for event in unread_messages(group, actor_id="peer1")],
                [later_in_ledger["id"]],
            )
            self.assertEqual(
                get_read_status(group, later_in_ledger["id"]).get("peer1"), False
            )
            self.assertEqual(
                get_read_status_batch(group, [later_in_ledger])[
                    later_in_ledger["id"]
                ].get("peer1"),
                False,
            )
        finally:
            cleanup()

    def test_new_message_status_cache_is_lazy_but_existing_rows_stay_coherent(
        self,
    ) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.ledger import append_event
        from cccc.kernel.ledger_status_cache import (
            get_cached_message_status_batch,
            warm_message_status_cache_from_event,
        )

        _, cleanup = self._with_home()
        try:
            created, _ = self._call(
                "group_create",
                {"title": "lazy-status-cache", "topic": "", "by": "user"},
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            stopped, _ = self._call("group_stop", {"group_id": group_id, "by": "user"})
            self.assertTrue(stopped.ok, getattr(stopped, "error", None))
            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "custom",
                    "runner": "pty",
                    "command": ["sh", "-c", "exit 0"],
                    "by": "user",
                },
            )
            self.assertTrue(added.ok, getattr(added, "error", None))
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            message = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group_id,
                scope_key="",
                by="user",
                data={
                    "text": "cache me on first read",
                    "to": ["peer1"],
                    "priority": "attention",
                    "reply_required": True,
                },
            )
            message_id = str(message.get("id") or "")
            self.assertEqual(get_cached_message_status_batch(group, [message_id]), {})

            warm_message_status_cache_from_event(group, message_id)
            warmed = get_cached_message_status_batch(group, [message_id])
            self.assertEqual(warmed[message_id]["read_status"].get("peer1"), False)
            self.assertEqual(warmed[message_id]["ack_status"].get("peer1"), False)
            self.assertEqual(
                warmed[message_id]["obligation_status"]["peer1"].get("replied"),
                False,
            )

            append_event(
                group.ledger_path,
                kind="chat.read",
                group_id=group_id,
                scope_key="",
                by="user",
                data={"actor_id": "peer1", "event_id": message_id},
            )
            after_read = get_cached_message_status_batch(group, [message_id])
            self.assertEqual(after_read[message_id]["read_status"].get("peer1"), True)
            self.assertEqual(after_read[message_id]["ack_status"].get("peer1"), False)
            self.assertEqual(
                after_read[message_id]["obligation_status"]["peer1"].get("acked"),
                False,
            )

            reply = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group_id,
                scope_key="",
                by="peer1",
                data={
                    "text": "reply",
                    "to": ["user"],
                    "reply_to": message_id,
                },
            )
            reply_id = str(reply.get("id") or "")
            after_reply = get_cached_message_status_batch(group, [message_id, reply_id])
            self.assertEqual(
                after_reply[message_id]["obligation_status"]["peer1"].get("replied"),
                True,
            )
            self.assertEqual(after_reply[message_id]["ack_status"].get("peer1"), False)
            self.assertEqual(
                after_reply[message_id]["obligation_status"]["peer1"].get("acked"),
                False,
            )
            self.assertNotIn(reply_id, after_reply)

            append_event(
                group.ledger_path,
                kind="chat.ack",
                group_id=group_id,
                scope_key="",
                by="peer1",
                data={"actor_id": "peer1", "event_id": message_id},
            )
            after_ack = get_cached_message_status_batch(group, [message_id])
            self.assertEqual(after_ack[message_id]["ack_status"].get("peer1"), True)
            self.assertEqual(
                after_ack[message_id]["obligation_status"]["peer1"].get("acked"),
                True,
            )

            removed, _ = self._call(
                "actor_remove",
                {"group_id": group_id, "actor_id": "peer1", "by": "user"},
            )
            self.assertTrue(removed.ok, getattr(removed, "error", None))
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            self.assertEqual(get_cached_message_status_batch(group, [message_id]), {})
        finally:
            cleanup()

    def test_actor_generation_follows_ledger_order_not_timestamps(self) -> None:
        from cccc.contracts.v1.event import Event as ContractEvent
        from cccc.kernel.group import load_group
        from cccc.kernel.inbox import (
            get_ack_status_batch,
            get_obligation_status_batch,
            get_read_status_batch,
        )
        from cccc.kernel.ledger import append_event
        from cccc.kernel.ledger_status_cache import (
            get_cached_message_status_batch,
            warm_message_status_cache_from_event,
        )

        _, cleanup = self._with_home()
        try:
            created, _ = self._call(
                "group_create",
                {"title": "actor-generation-order", "topic": "", "by": "user"},
            )
            self.assertTrue(created.ok, getattr(created, "error", None))
            group_id = str((created.result or {}).get("group_id") or "").strip()
            stopped, _ = self._call("group_stop", {"group_id": group_id, "by": "user"})
            self.assertTrue(stopped.ok, getattr(stopped, "error", None))
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            with patch(
                "cccc.kernel.ledger.Event",
                side_effect=lambda **kwargs: ContractEvent(
                    ts="2999-01-01T00:00:00Z", **kwargs
                ),
            ):
                before_actor = append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group_id,
                    scope_key="",
                    by="user",
                    data={
                        "text": "before actor",
                        "to": ["peer1"],
                        "priority": "attention",
                        "reply_required": True,
                    },
                )
            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "runtime": "custom",
                    "runner": "pty",
                    "command": ["sh", "-c", "exit 0"],
                    "by": "user",
                },
            )
            self.assertTrue(added.ok, getattr(added, "error", None))
            actor_add_id = str(
                ((added.result or {}).get("event") or {}).get("id") or ""
            )
            self.assertTrue(actor_add_id)
            with patch(
                "cccc.kernel.ledger.Event",
                side_effect=lambda **kwargs: ContractEvent(
                    ts="2000-01-01T00:00:00Z", **kwargs
                ),
            ):
                after_actor = append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group_id,
                    scope_key="",
                    by="user",
                    data={
                        "text": "after actor",
                        "to": ["peer1"],
                        "priority": "attention",
                        "reply_required": True,
                    },
                )
            other_actor = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group_id,
                scope_key="",
                by="user",
                data={"text": "for another actor", "to": ["peer2"]},
            )

            for event_id, expected_code in (
                (before_actor["id"], "event_not_for_actor"),
                (actor_add_id, "invalid_event_kind"),
                (other_actor["id"], "event_not_for_actor"),
            ):
                rejected_read, _ = self._call(
                    "inbox_mark_read",
                    {
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "event_id": event_id,
                        "by": "peer1",
                    },
                )
                self.assertFalse(rejected_read.ok)
                self.assertEqual(
                    getattr(rejected_read.error, "code", ""), expected_code
                )

            inbox, _ = self._call(
                "inbox_list",
                {"group_id": group_id, "actor_id": "peer1", "by": "peer1", "limit": 10},
            )
            self.assertTrue(inbox.ok, getattr(inbox, "error", None))
            self.assertEqual(
                [item.get("id") for item in (inbox.result or {}).get("messages", [])],
                [after_actor["id"]],
            )

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            events = [before_actor, after_actor]
            read = get_read_status_batch(group, events)
            ack = get_ack_status_batch(group, events)
            obligation = get_obligation_status_batch(group, events)
            self.assertNotIn("peer1", read[before_actor["id"]])
            self.assertNotIn("peer1", ack[before_actor["id"]])
            self.assertNotIn("peer1", obligation[before_actor["id"]])
            self.assertEqual(read[after_actor["id"]].get("peer1"), False)
            self.assertEqual(ack[after_actor["id"]].get("peer1"), False)
            self.assertEqual(obligation[after_actor["id"]]["peer1"].get("acked"), False)

            rejected, _ = self._call(
                "chat_ack",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": before_actor["id"],
                    "by": "peer1",
                },
            )
            self.assertFalse(rejected.ok)
            self.assertEqual(getattr(rejected.error, "code", ""), "event_not_for_actor")
            accepted, _ = self._call(
                "chat_ack",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": after_actor["id"],
                    "by": "peer1",
                },
            )
            self.assertTrue(accepted.ok, getattr(accepted, "error", None))
            marked, _ = self._call(
                "inbox_mark_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": after_actor["id"],
                    "by": "peer1",
                },
            )
            self.assertTrue(marked.ok, getattr(marked, "error", None))
            cursor = (marked.result or {}).get("cursor") or {}
            self.assertEqual(cursor.get("event_id"), after_actor["id"])
            self.assertEqual(cursor.get("ts"), after_actor["ts"])
            self.assertTrue(str(cursor.get("updated_at") or ""))

            warm_message_status_cache_from_event(group, after_actor["id"])
            self.assertIn(
                after_actor["id"],
                get_cached_message_status_batch(group, [after_actor["id"]]),
            )
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
                    "runtime": "custom",
                    "runner": "pty",
                    "command": ["sh", "-c", "exit 0"],
                    "by": "user",
                },
            )
            self.assertTrue(readded.ok, getattr(readded, "error", None))
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            self.assertEqual(
                get_cached_message_status_batch(group, [after_actor["id"]]), {}
            )
            self.assertNotIn(
                "peer1", get_read_status_batch(group, [after_actor])[after_actor["id"]]
            )
            stale_ack, _ = self._call(
                "chat_ack",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": after_actor["id"],
                    "by": "peer1",
                },
            )
            self.assertFalse(stale_ack.ok)
            self.assertEqual(
                getattr(stale_ack.error, "code", ""), "event_not_for_actor"
            )
            stale_read, _ = self._call(
                "inbox_mark_read",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "event_id": after_actor["id"],
                    "by": "peer1",
                },
            )
            self.assertFalse(stale_read.ok)
            self.assertEqual(
                getattr(stale_read.error, "code", ""), "event_not_for_actor"
            )
        finally:
            cleanup()

    def test_deepseek_cursor_gap_check_scans_only_the_reverse_tail(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call(
                "group_create",
                {"title": "deepseek-cursor-gap", "topic": "", "by": "user"},
            )
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            added, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "deepseek",
                    "runtime": "deepseek",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(added.ok, getattr(added, "error", None))

            events = []
            for text in ("first", "second", "third"):
                sent, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "to": ["deepseek"],
                        "text": text,
                    },
                )
                self.assertTrue(sent.ok, getattr(sent, "error", None))
                events.append((sent.result or {})["event"])

            from cccc.kernel.group import load_group
            from cccc.kernel.inbox import set_cursor

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            with self.assertRaisesRegex(ValueError, "cannot skip"):
                set_cursor(
                    group,
                    "deepseek",
                    event_id=events[1]["id"],
                    ts=events[1]["ts"],
                )

            set_cursor(
                group,
                "deepseek",
                event_id=events[0]["id"],
                ts=events[0]["ts"],
            )
            with patch(
                "cccc.kernel.inbox.iter_source_lines",
                side_effect=AssertionError("must not reparse the ledger prefix"),
            ):
                cursor = set_cursor(
                    group,
                    "deepseek",
                    event_id=events[1]["id"],
                    ts=events[1]["ts"],
                )
            self.assertEqual(cursor["event_id"], events[1]["id"])
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
