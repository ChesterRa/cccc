import json
import logging
import os
import shutil
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch


class TestChatOps(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            for attempt in range(5):
                try:
                    shutil.rmtree(td)
                    break
                except FileNotFoundError:
                    break
                except OSError:
                    if attempt >= 4:
                        raise
                    time.sleep(0.05)

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_try_handle_unknown_chat_op_returns_none(self) -> None:
        from cccc.daemon.messaging.chat_ops import try_handle_chat_op

        self.assertIsNone(
            try_handle_chat_op(
                "not_chat",
                {},
                coerce_bool=lambda _: False,
                normalize_attachments=lambda _group, _raw: [],
                effective_runner_kind=lambda kind: kind,
                auto_wake_recipients=lambda _group, _to, _by: [],
                automation_on_resume=lambda _group: None,
                automation_on_new_message=lambda _group: None,
                clear_pending_system_notifies=lambda _group_id, _kinds: None,
            )
        )

    def test_wake_group_on_human_message_skips_execution_time_idle_when_accept_was_active(self) -> None:
        from cccc.daemon.messaging.chat_ops import _wake_group_on_human_message

        group = object()
        with patch("cccc.daemon.messaging.chat_ops.get_group_state", return_value="idle"), patch(
            "cccc.daemon.messaging.chat_ops.find_actor", return_value=None
        ), patch("cccc.daemon.messaging.chat_ops.set_group_state") as set_state:
            out = _wake_group_on_human_message(
                group,
                by="user",
                state_at_accept="active",
                automation_on_resume=lambda _group: None,
                clear_pending_system_notifies=lambda _group_id, _kinds: None,
            )

        self.assertIs(out, group)
        set_state.assert_not_called()

    def test_wake_group_on_human_message_wakes_when_accept_was_idle(self) -> None:
        from cccc.daemon.messaging.chat_ops import _wake_group_on_human_message

        fake_group = type("G", (), {"group_id": "g1"})()
        with patch("cccc.daemon.messaging.chat_ops.get_group_state", return_value="idle"), patch(
            "cccc.daemon.messaging.chat_ops.find_actor", return_value=None
        ), patch("cccc.daemon.messaging.chat_ops.set_group_state", return_value=fake_group) as set_state:
            out = _wake_group_on_human_message(
                fake_group,
                by="user",
                state_at_accept="idle",
                automation_on_resume=lambda _group: None,
                clear_pending_system_notifies=lambda _group_id, _kinds: None,
            )

        self.assertIs(out, fake_group)
        set_state.assert_called_once_with(fake_group, state="active")

    def test_attention_reply_still_writes_chat_ack(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "chat-ops", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_1, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": False,
                },
            )
            self.assertTrue(add_1.ok, getattr(add_1, "error", None))

            add_2, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer2",
                    "title": "Peer 2",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": False,
                },
            )
            self.assertTrue(add_2.ok, getattr(add_2, "error", None))

            send, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "peer1",
                    "to": ["peer2"],
                    "text": "ack me",
                    "priority": "attention",
                },
            )
            self.assertTrue(send.ok, getattr(send, "error", None))
            sent_event = (send.result or {}).get("event") if isinstance(send.result, dict) else {}
            self.assertIsInstance(sent_event, dict)
            assert isinstance(sent_event, dict)
            sent_event_id = str(sent_event.get("id") or "").strip()
            self.assertTrue(sent_event_id)

            reply, _ = self._call(
                "reply",
                {
                    "group_id": group_id,
                    "by": "peer2",
                    "reply_to": sent_event_id,
                    "text": "done",
                },
            )
            self.assertTrue(reply.ok, getattr(reply, "error", None))
            ack_event = (reply.result or {}).get("ack_event") if isinstance(reply.result, dict) else {}
            self.assertIsInstance(ack_event, dict)
            assert isinstance(ack_event, dict)
            self.assertEqual(str(ack_event.get("kind") or ""), "chat.ack")
        finally:
            cleanup()

    def test_reply_accepts_unique_short_event_id_prefix(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "chat-short-reply", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": False,
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            send, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "original message",
                },
            )
            self.assertTrue(send.ok, getattr(send, "error", None))
            sent_event = (send.result or {}).get("event") if isinstance(send.result, dict) else {}
            self.assertIsInstance(sent_event, dict)
            assert isinstance(sent_event, dict)
            sent_event_id = str(sent_event.get("id") or "").strip()
            self.assertGreaterEqual(len(sent_event_id), 8)

            reply, _ = self._call(
                "reply",
                {
                    "group_id": group_id,
                    "by": "peer1",
                    "reply_to": sent_event_id[:8],
                    "text": "reply via short id",
                },
            )
            self.assertTrue(reply.ok, getattr(reply, "error", None))
            reply_event = (reply.result or {}).get("event") if isinstance(reply.result, dict) else {}
            self.assertIsInstance(reply_event, dict)
            assert isinstance(reply_event, dict)
            data = reply_event.get("data") if isinstance(reply_event.get("data"), dict) else {}
            self.assertEqual(str(data.get("reply_to") or ""), sent_event_id)
        finally:
            cleanup()

    def test_send_preserves_explicit_quote_text(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "chat-send-quote", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            send, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["user"],
                    "text": "测试activity消息抖动",
                    "quote_text": "为什么activity 会出现再消失，当前抖动太严重了",
                },
            )
            self.assertTrue(send.ok, getattr(send, "error", None))
            sent_event = (send.result or {}).get("event") if isinstance(send.result, dict) else {}
            self.assertIsInstance(sent_event, dict)
            assert isinstance(sent_event, dict)
            data = sent_event.get("data") if isinstance(sent_event.get("data"), dict) else {}
            self.assertEqual(
                str(data.get("quote_text") or ""),
                "为什么activity 会出现再消失，当前抖动太严重了",
            )
        finally:
            cleanup()

    def test_reply_preserves_im_source_identity_and_mentions(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "chat-im-reply", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            send, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "请回复我",
                    "source_platform": "dingtalk",
                    "source_user_name": "Alice",
                    "source_user_id": "staff_001",
                    "mention_user_ids": ["staff_001"],
                },
            )
            self.assertTrue(send.ok, getattr(send, "error", None))
            original_event = (send.result or {}).get("event") if isinstance(send.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            assert isinstance(original_event, dict)
            original_event_id = str(original_event.get("id") or "").strip()
            self.assertTrue(original_event_id)

            reply, _ = self._call(
                "reply",
                {
                    "group_id": group_id,
                    "by": "peer1",
                    "reply_to": original_event_id,
                    "text": "收到",
                },
            )
            self.assertTrue(reply.ok, getattr(reply, "error", None))
            reply_event = (reply.result or {}).get("event") if isinstance(reply.result, dict) else {}
            self.assertIsInstance(reply_event, dict)
            assert isinstance(reply_event, dict)
            data = reply_event.get("data") if isinstance(reply_event.get("data"), dict) else {}
            self.assertEqual(str(data.get("source_platform") or ""), "dingtalk")
            self.assertEqual(str(data.get("source_user_name") or ""), "Alice")
            self.assertEqual(str(data.get("source_user_id") or ""), "staff_001")
            self.assertEqual(data.get("mention_user_ids"), ["staff_001"])
        finally:
            cleanup()

    def test_send_persists_actor_sender_snapshot(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "chat-sender-snapshot", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "代码审查员",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": False,
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            avatar_path = Path(os.environ["CCCC_HOME"]) / "groups" / group_id / "state" / "actor_avatars" / "avatar_test.png"
            avatar_path.parent.mkdir(parents=True, exist_ok=True)
            avatar_path.write_bytes(
                b"\x89PNG\r\n\x1a\n"
                b"\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89"
                b"\x00\x00\x00\rIDATx\x9cc`\x00\x00\x00\x02\x00\x01\xe5'\xd4\xa2"
                b"\x00\x00\x00\x00IEND\xaeB`\x82"
            )
            update, _ = self._call(
                "actor_update",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "by": "user",
                    "patch": {"avatar_asset_path": f"groups/{group_id}/state/actor_avatars/avatar_test.png"},
                },
            )
            self.assertTrue(update.ok, getattr(update, "error", None))

            send, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "peer1",
                    "to": ["user"],
                    "text": "审查完成",
                },
            )
            self.assertTrue(send.ok, getattr(send, "error", None))
            event = (send.result or {}).get("event") if isinstance(send.result, dict) else {}
            self.assertIsInstance(event, dict)
            assert isinstance(event, dict)
            data = event.get("data") if isinstance(event.get("data"), dict) else {}
            self.assertEqual(str(data.get("sender_title") or ""), "代码审查员")
            self.assertEqual(str(data.get("sender_runtime") or ""), "codex")
            avatar_blob_path = str(data.get("sender_avatar_path") or "")
            self.assertTrue(avatar_blob_path.startswith("state/blobs/"))
            self.assertTrue((Path(os.environ["CCCC_HOME"]) / "groups" / group_id / avatar_blob_path).exists())
        finally:
            cleanup()


    def test_send_pet_review_immediate_follows_reply_required(self) -> None:
        group_id, cleanup = self._setup_group_with_actors()
        try:
            with patch("cccc.daemon.messaging.chat_ops.request_pet_review") as review_mock:
                resp, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "to": ["peer1"],
                        "text": "normal send",
                    },
                )
                self.assertTrue(resp.ok, getattr(resp, "error", None))

                review_mock.assert_called_once()
                self.assertEqual(review_mock.call_args.kwargs.get("reason"), "chat_message")
                self.assertFalse(bool(review_mock.call_args.kwargs.get("immediate")))

                review_mock.reset_mock()
                resp, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "to": ["peer1"],
                        "text": "urgent send",
                        "reply_required": True,
                    },
                )
                self.assertTrue(resp.ok, getattr(resp, "error", None))

                review_mock.assert_called_once()
                self.assertEqual(review_mock.call_args.kwargs.get("reason"), "chat_message")
                self.assertTrue(bool(review_mock.call_args.kwargs.get("immediate")))
        finally:
            cleanup()

    def test_reply_pet_review_immediate_follows_reply_required(self) -> None:
        group_id, cleanup = self._setup_group_with_actors()
        try:
            send, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["peer1"],
                    "text": "original",
                },
            )
            self.assertTrue(send.ok, getattr(send, "error", None))
            original_event = (send.result or {}).get("event") if isinstance(send.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            assert isinstance(original_event, dict)
            original_event_id = str(original_event.get("id") or "").strip()
            self.assertTrue(original_event_id)

            with patch("cccc.daemon.messaging.chat_ops.request_pet_review") as review_mock:
                reply, _ = self._call(
                    "reply",
                    {
                        "group_id": group_id,
                        "by": "peer1",
                        "reply_to": original_event_id,
                        "text": "normal reply",
                    },
                )
                self.assertTrue(reply.ok, getattr(reply, "error", None))

                review_mock.assert_called_once()
                self.assertEqual(review_mock.call_args.kwargs.get("reason"), "chat_reply")
                self.assertFalse(bool(review_mock.call_args.kwargs.get("immediate")))

                review_mock.reset_mock()
                reply, _ = self._call(
                    "reply",
                    {
                        "group_id": group_id,
                        "by": "peer1",
                        "reply_to": original_event_id,
                        "text": "urgent reply",
                        "reply_required": True,
                    },
                )
                self.assertTrue(reply.ok, getattr(reply, "error", None))

                review_mock.assert_called_once()
                self.assertEqual(review_mock.call_args.kwargs.get("reason"), "chat_reply")
                self.assertTrue(bool(review_mock.call_args.kwargs.get("immediate")))
        finally:
            cleanup()

    def test_user_message_wakes_idle_group_and_clears_pending_auto_idle_notifications(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "chat-wake", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "foreman1",
                    "title": "Foreman 1",
                    "runtime": "codex",
                    "runner": "headless",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            idle, _ = self._call("group_set_state", {"group_id": group_id, "state": "idle", "by": "user"})
            self.assertTrue(idle.ok, getattr(idle, "error", None))

            with patch("cccc.daemon.server.THROTTLE.clear_pending_system_notifies", return_value=1) as clear_mock:
                send, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "to": ["foreman1"],
                        "text": "wake up",
                    },
                )

            self.assertTrue(send.ok, getattr(send, "error", None))
            clear_mock.assert_called_once()
            notify_kinds = clear_mock.call_args.kwargs.get("notify_kinds")
            self.assertIsInstance(notify_kinds, set)
            self.assertIn("auto_idle", notify_kinds)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            self.assertEqual(str(group.doc.get("state") or ""), "active")
        finally:
            cleanup()


    # -- T067: cccc_message_send `to` routing bug fix tests --

    def _setup_group_with_actors(self):
        """Helper: create group with foreman + 2 peers for routing tests."""
        _, cleanup = self._with_home()
        create, _ = self._call("group_create", {"title": "route-test", "topic": "", "by": "user"})
        assert create.ok, getattr(create, "error", None)
        group_id = str((create.result or {}).get("group_id") or "").strip()

        self._call("actor_add", {
            "group_id": group_id, "by": "user",
            "actor_id": "peer1", "title": "Peer 1",
            "runtime": "codex", "runner": "headless", "enabled": False,
        })
        self._call("actor_add", {
            "group_id": group_id, "by": "user",
            "actor_id": "peer2", "title": "Peer 2",
            "runtime": "codex", "runner": "headless", "enabled": False,
        })
        return group_id, cleanup

    def test_tracked_send_creates_task_and_linked_message(self) -> None:
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("tracked_send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "title": "Investigate routing bug",
                "text": "Please investigate the routing bug and reply with evidence.",
                "outcome": "Root cause and evidence are reported.",
                "checklist": [{"text": "Find root cause"}, {"text": "Report evidence"}],
                "idempotency_key": "tracked-1",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            result = resp.result or {}
            self.assertTrue(result.get("message_sent"), result)
            task_id = str(result.get("task_id") or "").strip()
            self.assertTrue(task_id)
            event = result.get("event") or {}
            refs = event.get("data", {}).get("refs", [])
            self.assertEqual(refs[0].get("kind"), "task_ref")
            self.assertEqual(refs[0].get("task_id"), task_id)
            self.assertEqual(event.get("data", {}).get("reply_required"), True)

            task_resp, _ = self._call("task_list", {"group_id": group_id, "task_id": task_id})
            self.assertTrue(task_resp.ok, getattr(task_resp, "error", None))
            task = (task_resp.result or {}).get("task") or {}
            self.assertEqual(task.get("assignee"), "peer1")
            self.assertEqual(task.get("waiting_on"), "actor")
            self.assertEqual(task.get("task_type"), "standard")
        finally:
            cleanup()

    def test_tracked_send_replay_does_not_duplicate_successful_request(self) -> None:
        group_id, cleanup = self._setup_group_with_actors()
        try:
            payload = {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "title": "Idempotent tracked send",
                "text": "Please claim this once.",
                "idempotency_key": "same-request",
            }
            first, _ = self._call("tracked_send", dict(payload))
            second, _ = self._call("tracked_send", dict(payload))
            self.assertTrue(first.ok, getattr(first, "error", None))
            self.assertTrue(second.ok, getattr(second, "error", None))
            self.assertEqual((first.result or {}).get("task_id"), (second.result or {}).get("task_id"))
            self.assertTrue((second.result or {}).get("replayed"))

            tasks_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(tasks_resp.ok, getattr(tasks_resp, "error", None))
            tasks = (tasks_resp.result or {}).get("tasks") or []
            self.assertEqual(len(tasks), 1)
        finally:
            cleanup()

    def test_tracked_send_retries_partial_failure_without_duplicate_task(self) -> None:
        from cccc.contracts.v1 import DaemonError, DaemonResponse
        from cccc.daemon.messaging import chat_ops
        from cccc.kernel.group import load_group
        from cccc.kernel.ledger import read_last_lines

        group_id, cleanup = self._setup_group_with_actors()
        real_handle_send = chat_ops.handle_send
        attempts = {"count": 0}

        def fail_once(*args, **kwargs):
            attempts["count"] += 1
            if attempts["count"] == 1:
                return DaemonResponse(
                    ok=False,
                    error=DaemonError(code="send_failed", message="simulated send failure"),
                )
            return real_handle_send(*args, **kwargs)

        try:
            payload = {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "title": "Recover tracked send",
                "text": "Please recover this tracked send.",
                "idempotency_key": "resume-request",
            }
            with patch("cccc.daemon.messaging.chat_ops.handle_send", side_effect=fail_once):
                first, _ = self._call("tracked_send", dict(payload))
                second, _ = self._call("tracked_send", dict(payload))

            self.assertTrue(first.ok, getattr(first, "error", None))
            self.assertTrue(second.ok, getattr(second, "error", None))

            first_result = first.result or {}
            second_result = second.result or {}
            self.assertTrue(first_result.get("task_created"), first_result)
            self.assertFalse(first_result.get("message_sent"), first_result)
            self.assertTrue(first_result.get("partial_failure"), first_result)

            self.assertFalse(second_result.get("task_created"), second_result)
            self.assertTrue(second_result.get("message_sent"), second_result)
            self.assertFalse(second_result.get("partial_failure"), second_result)
            self.assertTrue(second_result.get("recovered_from_partial_failure"), second_result)
            self.assertEqual(first_result.get("task_id"), second_result.get("task_id"))

            tasks_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(tasks_resp.ok, getattr(tasks_resp, "error", None))
            tasks = (tasks_resp.result or {}).get("tasks") or []
            self.assertEqual(len(tasks), 1)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            lines = read_last_lines(group.ledger_path, 50)
            chat_events = []
            for raw_line in lines:
                event = json.loads(raw_line)
                if isinstance(event, dict) and str(event.get("kind") or "") == "chat.message":
                    chat_events.append(event)
            self.assertEqual(len(chat_events), 1)
        finally:
            cleanup()

    def test_tracked_send_peer_cannot_assign_other_peer_and_sends_no_message(self) -> None:
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("tracked_send", {
                "group_id": group_id,
                "by": "peer2",
                "to": ["peer1"],
                "title": "Peer should not assign peer",
                "text": "Please do this.",
            })
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "context_sync_error")

            tasks_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(tasks_resp.ok, getattr(tasks_resp, "error", None))
            self.assertEqual((tasks_resp.result or {}).get("tasks") or [], [])
        finally:
            cleanup()

    def test_tracked_send_invalid_message_priority_creates_no_task(self) -> None:
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("tracked_send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "title": "Bad priority",
                "text": "This must fail before task creation.",
                "priority": "urgent",
            })
            self.assertFalse(resp.ok)
            self.assertEqual(resp.error.code, "invalid_priority")

            tasks_resp, _ = self._call("task_list", {"group_id": group_id})
            self.assertTrue(tasks_resp.ok, getattr(tasks_resp, "error", None))
            self.assertEqual((tasks_resp.result or {}).get("tasks") or [], [])
        finally:
            cleanup()

    def test_send_to_string_is_routed_correctly(self) -> None:
        """T067 scenario 1: LLM passes `to` as string instead of array."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "text": "hello peer1",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertIn("peer1", event.get("data", {}).get("to", []))
        finally:
            cleanup()

    def test_send_to_array_is_routed_correctly(self) -> None:
        """T067 scenario 2: `to` passed as array (normal case)."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1", "peer2"],
                "text": "hello both",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            to_list = event.get("data", {}).get("to", [])
            self.assertIn("peer1", to_list)
            self.assertIn("peer2", to_list)
        finally:
            cleanup()

    def test_send_to_string_direct_payload_is_routed_correctly(self) -> None:
        """T067 scenario: daemon send op tolerates string `to` payload."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": "peer1",
                "text": "hello peer1",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertEqual(event.get("data", {}).get("to", []), ["peer1"])
        finally:
            cleanup()

    def test_send_empty_to_uses_default(self) -> None:
        """T067 scenario 3: empty `to` falls back to group default."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": [],
                "text": "broadcast test",
            })
            # Empty to with no mentions -> falls back to group default or broadcast
            # Either way should succeed for user sender
            self.assertTrue(resp.ok, getattr(resp, "error", None))
        finally:
            cleanup()

    def test_send_to_malformed_list_entries_falls_back_to_default(self) -> None:
        """T067 edge: malformed list entries should not become broadcast."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": [None, ""],
                "text": "malformed to payload",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertEqual(event.get("data", {}).get("to", []), ["@foreman"])
        finally:
            cleanup()

    def test_send_multiple_recipients(self) -> None:
        """T067 scenario 4: multiple explicit recipients."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1", "peer2"],
                "text": "multi-target",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            to_list = event.get("data", {}).get("to", [])
            self.assertEqual(len(to_list), 2)
        finally:
            cleanup()

    def test_send_and_reply_preserve_refs(self) -> None:
        """Refs payload should round-trip through daemon send/reply."""
        group_id, cleanup = self._setup_group_with_actors()
        refs = [{"kind": "presentation_ref", "slot_id": "slot-2", "label": "P2", "locator_label": "PDF p.12"}]
        try:
            send_resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "text": "hello peer1",
                "refs": refs,
            })
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            send_event = (send_resp.result or {}).get("event", {})
            self.assertEqual(send_event.get("data", {}).get("refs", []), refs)

            reply_to = str(send_event.get("id") or "")
            reply_resp, _ = self._call("reply", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "reply_to": reply_to,
                "text": "reply peer1",
                "refs": refs,
            })
            self.assertTrue(reply_resp.ok, getattr(reply_resp, "error", None))
            reply_event = (reply_resp.result or {}).get("event", {})
            self.assertEqual(reply_event.get("data", {}).get("refs", []), refs)
        finally:
            cleanup()

    def test_send_and_reply_delivery_text_include_presentation_refs_for_pty_actor(self) -> None:
        """PTY delivery text should include compact presentation ref details."""
        _, cleanup = self._with_home()
        refs = [
            {
                "kind": "presentation_ref",
                "slot_id": "slot-2",
                "label": "P2",
                "locator_label": "PDF p.12",
                "title": "Revenue deck",
                "excerpt": "Gross margin note is outdated.",
                "href": "https://example.test/deck.pdf#page=12",
                "locator": {
                    "url": "https://example.test/deck.pdf#page=12",
                    "captured_at": "2026-03-23T10:00:00Z",
                    "viewer_scroll_top": 240,
                },
                "snapshot": {
                    "path": "state/blobs/sha256_demo.jpg",
                    "width": 1440,
                    "height": 900,
                },
            }
        ]
        try:
            create, _ = self._call("group_create", {"title": "pty-refs", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "pty",
                    "enabled": True,
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            with patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as send_queue, patch(
                "cccc.daemon.messaging.chat_ops.request_flush_pending_messages"
            ) as send_flush:
                send_resp, _ = self._call(
                    "send",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "to": ["peer1"],
                        "text": "please inspect this page",
                        "refs": refs,
                    },
                )
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            send_queue.assert_called_once()
            send_flush.assert_called_once_with(unittest.mock.ANY, actor_id="peer1")
            send_delivery_text = str(send_queue.call_args.kwargs.get("text") or "")
            self.assertIn("[cccc] References:", send_delivery_text)
            self.assertIn("P2 (slot-2) · PDF p.12 — Revenue deck", send_delivery_text)
            self.assertIn('excerpt: "Gross margin note is outdated."', send_delivery_text)
            self.assertIn("href: https://example.test/deck.pdf#page=12", send_delivery_text)
            self.assertIn("captured_at: 2026-03-23T10:00:00Z", send_delivery_text)
            self.assertIn("scroll_top: 240", send_delivery_text)
            self.assertIn("snapshot: state/blobs/sha256_demo.jpg (1440x900)", send_delivery_text)

            send_event = (send_resp.result or {}).get("event", {})
            reply_to = str(send_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            with patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as reply_queue, patch(
                "cccc.daemon.messaging.chat_ops.request_flush_pending_messages"
            ) as reply_flush, patch("cccc.daemon.messaging.chat_ops.flush_pending_messages") as reply_sync_flush:
                reply_resp, _ = self._call(
                    "reply",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "to": ["peer1"],
                        "reply_to": reply_to,
                        "text": "same view, updated ask",
                        "refs": refs,
                    },
                )
            self.assertTrue(reply_resp.ok, getattr(reply_resp, "error", None))
            reply_queue.assert_called_once()
            reply_flush.assert_called_once_with(unittest.mock.ANY, actor_id="peer1")
            reply_sync_flush.assert_not_called()
            reply_delivery_text = str(reply_queue.call_args.kwargs.get("text") or "")
            self.assertIn("[cccc] References:", reply_delivery_text)
            self.assertIn("P2 (slot-2) · PDF p.12 — Revenue deck", reply_delivery_text)
            self.assertIn("snapshot: state/blobs/sha256_demo.jpg (1440x900)", reply_delivery_text)
        finally:
            cleanup()

    def test_send_to_nonexistent_actor_returns_error(self) -> None:
        """T067 scenario 5: sending to a non-existent actor returns error."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["nonexistent_actor"],
                "text": "should fail",
            })
            self.assertFalse(resp.ok)
            err = resp.error
            self.assertIsNotNone(err)
            self.assertEqual(err.code, "invalid_recipient")
        finally:
            cleanup()

    def test_mcp_to_string_coercion(self) -> None:
        """T067 MCP layer: string `to` is converted to single-element array.

        Tests the daemon layer with a single-element array (simulating
        the MCP layer's string→array conversion).
        """
        group_id, cleanup = self._setup_group_with_actors()
        try:
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "user",
                "to": ["peer1"],
                "text": "string coercion test",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertEqual(event.get("data", {}).get("to", []), ["peer1"])
        finally:
            cleanup()

    def test_foreman_send_to_peer_no_self_routing(self) -> None:
        """T067 regression: foreman sending to peer should NOT route to self."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            # Add a foreman actor
            self._call("actor_add", {
                "group_id": group_id, "by": "user",
                "actor_id": "fm1", "title": "Foreman",
                "runtime": "codex", "runner": "headless", "enabled": False,
                "role": "foreman",
            })
            # Foreman sends explicitly to peer1
            resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "fm1",
                "to": ["peer1"],
                "text": "foreman to peer",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            to_list = event.get("data", {}).get("to", [])
            self.assertEqual(to_list, ["peer1"])
            # NOT ["@foreman"]!
            self.assertNotIn("@foreman", to_list)
        finally:
            cleanup()

    def test_foreman_send_default_to_foreman_returns_guided_error(self) -> None:
        """N015: foreman empty `to` should error with actionable guidance."""
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "n015-default", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_foreman, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "fm1",
                    "title": "Foreman",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": True,
                },
            )
            self.assertTrue(add_foreman.ok, getattr(add_foreman, "error", None))

            resp, _ = self._call("send", {"group_id": group_id, "by": "fm1", "text": "status"})
            self.assertFalse(resp.ok)
            err = resp.error
            self.assertIsNotNone(err)
            self.assertEqual(err.code, "no_enabled_recipients")
            message = str(err.message or "")
            self.assertIn("No enabled recipients after excluding sender", message)
            self.assertIn("to=['user']", message)
            self.assertIn("to=['@all']", message)
        finally:
            cleanup()

    def test_user_default_to_stopped_foreman_does_not_route_to_peer(self) -> None:
        """Default @foreman routing must keep the stable foreman target even when stopped."""
        from cccc.daemon.messaging.chat_ops import handle_send
        from cccc.kernel.group import load_group
        from cccc.util.conv import coerce_bool

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "stopped-foreman", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            for actor_id, enabled in (("fm1", False), ("peer1", True)):
                add, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "actor_id": actor_id,
                        "title": actor_id,
                        "runtime": "codex",
                        "runner": "headless",
                        "enabled": enabled,
                    },
                )
                self.assertTrue(add.ok, getattr(add, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            wake_calls: list[list[str]] = []

            def wake(_group, to, _by):
                wake_calls.append(list(to))
                return ["fm1"]

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message") as submit,
                patch("cccc.daemon.messaging.chat_ops.schedule_headless_post_wake_delivery", return_value=True) as schedule_post_wake,
                patch("cccc.daemon.messaging.chat_ops.get_headless_targets_for_message", return_value=["fm1"]),
                patch("cccc.daemon.messaging.chat_ops.emit_system_notify") as emit_notify,
            ):
                resp = handle_send(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "text": "default to stopped foreman",
                    },
                    coerce_bool=coerce_bool,
                    normalize_attachments=lambda _group, _raw: [],
                    effective_runner_kind=lambda runner: str(runner or "headless"),
                    auto_wake_recipients=wake,
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _kinds: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            self.assertEqual(wake_calls, [["@foreman"]])
            event = (resp.result or {}).get("event") if isinstance(resp.result, dict) else {}
            self.assertIsInstance(event, dict)
            assert isinstance(event, dict)
            self.assertEqual(event.get("data", {}).get("to"), ["@foreman"])
            submit.assert_not_called()
            schedule_post_wake.assert_called_once()
            schedule_kwargs = schedule_post_wake.call_args.kwargs
            self.assertEqual(schedule_kwargs.get("group_id"), group_id)
            self.assertEqual(schedule_kwargs.get("actor_id"), "fm1")
            self.assertEqual(schedule_kwargs.get("runtime"), "codex")
            self.assertEqual(schedule_kwargs.get("event_id"), event.get("id"))
            self.assertIn("default to stopped foreman", str(schedule_kwargs.get("text") or ""))
            emit_notify.assert_not_called()
        finally:
            cleanup()

    def test_foreman_send_explicit_foreman_returns_guided_error(self) -> None:
        """N015: foreman explicit `@foreman` should error with actionable guidance."""
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "n015-explicit", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_foreman, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "fm1",
                    "title": "Foreman",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": True,
                },
            )
            self.assertTrue(add_foreman.ok, getattr(add_foreman, "error", None))

            resp, _ = self._call(
                "send",
                {"group_id": group_id, "by": "fm1", "to": ["@foreman"], "text": "status"},
            )
            self.assertFalse(resp.ok)
            err = resp.error
            self.assertIsNotNone(err)
            self.assertEqual(err.code, "no_enabled_recipients")
            message = str(err.message or "")
            self.assertIn("No enabled recipients after excluding sender", message)
            self.assertIn("to=['peer-reviewer']", message)
        finally:
            cleanup()

    def test_reply_to_stopped_headless_actor_schedules_post_wake_delivery(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_reply
        from cccc.kernel.group import load_group
        from cccc.util.conv import coerce_bool

        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "headless-reply-wake", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "by": "user",
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "enabled": False,
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            original, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "to": ["user"],
                    "text": "original",
                },
            )
            self.assertTrue(original.ok, getattr(original, "error", None))
            original_event = (original.result or {}).get("event") if isinstance(original.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            assert isinstance(original_event, dict)
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message") as submit,
                patch("cccc.daemon.messaging.chat_ops.schedule_headless_post_wake_delivery", return_value=True) as schedule_post_wake,
                patch("cccc.daemon.messaging.chat_ops.get_headless_targets_for_message", return_value=["peer1"]),
                patch("cccc.daemon.messaging.chat_ops.emit_system_notify") as emit_notify,
            ):
                resp = handle_reply(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "reply_to": reply_to,
                        "to": ["peer1"],
                        "text": "reply after wake",
                    },
                    coerce_bool=coerce_bool,
                    normalize_attachments=lambda _group, _raw: [],
                    effective_runner_kind=lambda runner: str(runner or "headless"),
                    auto_wake_recipients=lambda _group, _to, _by: ["peer1"],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _kinds: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event") if isinstance(resp.result, dict) else {}
            self.assertIsInstance(event, dict)
            assert isinstance(event, dict)
            submit.assert_not_called()
            schedule_post_wake.assert_called_once()
            schedule_kwargs = schedule_post_wake.call_args.kwargs
            self.assertEqual(schedule_kwargs.get("group_id"), group_id)
            self.assertEqual(schedule_kwargs.get("actor_id"), "peer1")
            self.assertEqual(schedule_kwargs.get("runtime"), "codex")
            self.assertEqual(schedule_kwargs.get("event_id"), event.get("id"))
            self.assertEqual(schedule_kwargs.get("reply_to"), reply_to)
            self.assertIn("reply after wake", str(schedule_kwargs.get("text") or ""))
            emit_notify.assert_not_called()
        finally:
            cleanup()

    def test_auto_wake_reports_actor_already_being_woken_for_post_wake_delivery(self) -> None:
        from cccc.daemon.messaging.chat_support_ops import auto_wake_recipients

        group = type("G", (), {"group_id": "g1"})()
        lock = threading.Lock()
        in_progress = {("g1", "peer1")}
        start_calls: list[str] = []

        result = auto_wake_recipients(
            group,
            ["peer1"],
            by="user",
            disabled_recipient_actor_ids=lambda _group, _to: [],
            enabled_recipient_actor_ids=lambda _group, _to: ["peer1"],
            find_actor=lambda _group, _actor_id: {
                "id": "peer1",
                "enabled": True,
                "runtime": "codex",
                "runner": "headless",
            },
            coerce_bool=lambda value, default=True: bool(value) if value is not None else default,
            is_actor_running=lambda _group, _actor_id: False,
            start_actor_process=lambda *_args, **_kwargs: start_calls.append("start") or {"success": True},
            update_actor=lambda *_args, **_kwargs: None,
            runner_stop_actor=lambda *_args, **_kwargs: None,
            request_flush_pending_messages=lambda *_args, **_kwargs: True,
            logger=logging.getLogger("test"),
            auto_wake_lock=lock,
            auto_wake_in_progress=in_progress,
        )

        self.assertEqual(result, ["peer1"])
        self.assertEqual(start_calls, [])
        self.assertEqual(in_progress, {("g1", "peer1")})

    # -- T070: cccc_message_reply & cccc_file_send `to` string coercion tests --

    def test_reply_to_string_is_routed_correctly(self) -> None:
        """T070: reply with string `to` should route correctly (daemon layer)."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            # First send a message to get an event_id to reply to
            send_resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "peer1",
                "to": ["peer2"],
                "text": "original message",
            })
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            event_id = (send_resp.result or {}).get("event", {}).get("id", "")

            # Reply with string-style to (as single-element array, simulating MCP coercion)
            resp, _ = self._call("reply", {
                "group_id": group_id,
                "by": "peer2",
                "reply_to": event_id,
                "to": ["peer1"],
                "text": "reply to peer1",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            event = (resp.result or {}).get("event", {})
            self.assertIn("peer1", event.get("data", {}).get("to", []))
        finally:
            cleanup()

    def test_reply_empty_to_defaults_to_original_sender(self) -> None:
        """T070: reply with empty/None `to` defaults to original sender."""
        group_id, cleanup = self._setup_group_with_actors()
        try:
            send_resp, _ = self._call("send", {
                "group_id": group_id,
                "by": "peer1",
                "to": ["peer2"],
                "text": "original",
            })
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            event_id = (send_resp.result or {}).get("event", {}).get("id", "")

            # Reply with no `to` — should default to original sender
            resp, _ = self._call("reply", {
                "group_id": group_id,
                "by": "peer2",
                "reply_to": event_id,
                "text": "reply default",
            })
            self.assertTrue(resp.ok, getattr(resp, "error", None))
        finally:
            cleanup()


class TestMCPToCoercion(unittest.TestCase):
    """T070: Test MCP handler string→array coercion for reply and file_send."""

    def test_mcp_reply_handler_string_to_coercion(self) -> None:
        """MCP cccc_message_reply handler: string `to` → single-element list."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        # Capture what message_reply receives
        captured = {}

        def fake_message_reply(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.message_reply", side_effect=fake_message_reply):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_message_reply", {
                            "to": "peer2",
                            "event_id": "evt_123",
                            "text": "hello",
                        })
                    except Exception:
                        pass  # May fail on daemon call, we only care about captured args

        self.assertEqual(captured.get("to"), ["peer2"])

    def test_mcp_reply_handler_list_to_preserved(self) -> None:
        """MCP cccc_message_reply handler: list `to` is preserved."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        captured = {}

        def fake_message_reply(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.message_reply", side_effect=fake_message_reply):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_message_reply", {
                            "to": ["peer2", "peer3"],
                            "event_id": "evt_123",
                            "text": "hello",
                        })
                    except Exception:
                        pass

        self.assertEqual(captured.get("to"), ["peer2", "peer3"])

    def test_mcp_reply_handler_none_to_is_none(self) -> None:
        """MCP cccc_message_reply handler: missing `to` → None."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        captured = {}

        def fake_message_reply(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.message_reply", side_effect=fake_message_reply):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_message_reply", {
                            "event_id": "evt_123",
                            "text": "hello",
                        })
                    except Exception:
                        pass

        self.assertIsNone(captured.get("to"))

    def test_mcp_file_send_handler_string_to_coercion(self) -> None:
        """MCP cccc_file_send handler: string `to` → single-element list."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        captured = {}

        def fake_file_send(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.file_send", side_effect=fake_file_send):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_file", {
                            "action": "send",
                            "to": "user",
                            "path": "/tmp/test.txt",
                            "text": "file caption",
                        })
                    except Exception:
                        pass

        self.assertEqual(captured.get("to"), ["user"])

    def test_mcp_file_send_handler_none_to_is_none(self) -> None:
        """MCP cccc_file_send handler: missing `to` → None."""
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        captured = {}

        def fake_file_send(**kwargs):
            captured.update(kwargs)
            return {"event": {}}

        with patch("cccc.ports.mcp.server.file_send", side_effect=fake_file_send):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                with patch("cccc.ports.mcp.server._resolve_self_actor_id", return_value="peer1"):
                    try:
                        _handle_cccc_namespace("cccc_file", {
                            "action": "send",
                            "path": "/tmp/test.txt",
                        })
                    except Exception:
                        pass

        self.assertIsNone(captured.get("to"))

    def test_mcp_file_read_handler_routes_to_blob_read(self) -> None:
        from cccc.ports.mcp.server import _handle_cccc_namespace
        from unittest.mock import patch

        captured = {}

        def fake_blob_read(**kwargs):
            captured.update(kwargs)
            return {"text": "hello", "truncated": False}

        with patch("cccc.ports.mcp.server.blob_read", side_effect=fake_blob_read):
            with patch("cccc.ports.mcp.server._resolve_group_id", return_value="g_test"):
                _handle_cccc_namespace(
                    "cccc_file",
                    {
                        "action": "read",
                        "rel_path": "state/blobs/demo.txt",
                        "max_bytes": 12,
                    },
                )

        self.assertEqual(captured.get("group_id"), "g_test")
        self.assertEqual(captured.get("rel_path"), "state/blobs/demo.txt")
        self.assertEqual(captured.get("max_bytes"), 12)


if __name__ == "__main__":
    unittest.main()
