from __future__ import annotations

import io
import json
import os
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch


class TestCodexAppFlow(unittest.TestCase):
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

    def _ledger_events(self, group) -> list[dict]:
        events: list[dict] = []
        with group.ledger_path.open("r", encoding="utf-8") as fh:
            for line in fh:
                line = line.strip()
                if not line:
                    continue
                obj = json.loads(line)
                if isinstance(obj, dict):
                    events.append(obj)
        return events

    def test_claude_app_session_persists_start_state_outside_lock(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession

        _, cleanup = self._with_home()
        try:
            session = ClaudeAppSession(
                group_id="g_lock_regression",
                actor_id="claude-peer",
                cwd=Path(os.environ["CCCC_HOME"]),
                env={},
            )

            class FakeProc:
                pid = 12345
                stdin = io.StringIO()
                stdout = io.StringIO()
                stderr = io.StringIO()

                def poll(self):
                    return None

            class FakeThread:
                def __init__(self, *args, **kwargs):
                    self.args = args
                    self.kwargs = kwargs

                def start(self):
                    return None

            persist_lock_was_free: list[bool] = []

            def fake_persist_state() -> None:
                acquired = session._lock.acquire(blocking=False)
                persist_lock_was_free.append(bool(acquired))
                if acquired:
                    session._lock.release()

            with (
                patch("cccc.daemon.claude_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.claude_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.claude_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.claude_app_sessions.time.sleep", return_value=None),
                patch.object(session, "_persist_state", side_effect=fake_persist_state),
                patch.object(session, "_queue_bootstrap_control_turn", return_value=None),
            ):
                session.start()

            self.assertEqual(persist_lock_was_free, [True])
        finally:
            cleanup()

    def test_actor_list_uses_codex_supervisor_state_for_headless_working(self) -> None:
        from cccc.daemon.actors.actor_ops import handle_actor_list

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-actor-list", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            with (
                patch("cccc.daemon.actors.actor_ops.codex_app_supervisor.actor_running", return_value=True),
                patch(
                    "cccc.daemon.actors.actor_ops.codex_app_supervisor.get_state",
                    return_value={
                        "group_id": group_id,
                        "actor_id": "peer1",
                        "status": "working",
                        "current_task_id": "turn-123",
                        "updated_at": "2026-04-02T10:00:00Z",
                    },
                ),
            ):
                resp = handle_actor_list({"group_id": group_id, "include_unread": False}, effective_runner_kind=lambda runner: str(runner or "pty"))

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            actors = (resp.result or {}).get("actors") if isinstance(resp.result, dict) else []
            self.assertIsInstance(actors, list)
            assert isinstance(actors, list)
            self.assertEqual(len(actors), 1)
            actor = actors[0]
            self.assertEqual(actor.get("runner"), "headless")
            self.assertEqual(actor.get("runner_effective"), "headless")
            self.assertTrue(bool(actor.get("running")))
            self.assertEqual(actor.get("effective_working_state"), "working")
            self.assertEqual(actor.get("effective_working_reason"), "headless_working")
            self.assertEqual(actor.get("effective_active_task_id"), "turn-123")
        finally:
            cleanup()

    def test_send_routes_running_headless_codex_actor_to_app_supervisor(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_send
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-send", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message") as submit_user_message,
                patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
            ):
                resp = handle_send(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "text": "hello codex",
                        "to": ["peer1"],
                        "client_id": "c1",
                    },
                    coerce_bool=lambda value: bool(value),
                    normalize_attachments=lambda _group, _attachments: [],
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _reasons: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            submit_user_message.assert_called_once()
            submitted_text = str(submit_user_message.call_args.kwargs.get("text") or "")
            self.assertIn("[cccc] user → peer1:", submitted_text)
            self.assertIn("hello codex", submitted_text)
            queue_chat_message.assert_not_called()
            request_flush_pending_messages.assert_not_called()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            self.assertFalse(any(str(item.get("kind") or "") == "system.notify" for item in self._ledger_events(group)))
        finally:
            cleanup()

    def test_send_headless_codex_auto_mark_waits_for_runtime_acceptance(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_send
        from cccc.daemon.messaging.delivery import MCP_REMINDER_LINE
        from cccc.daemon.messaging.delivery import auto_mark_headless_delivery_started
        from cccc.kernel.group import load_group
        from cccc.kernel.inbox import get_cursor, unread_count

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-send-auto-mark", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["delivery"] = {"auto_mark_on_delivery": True}
            group.save()
            before_cursor_event_id, before_cursor_ts = get_cursor(group, "peer1")

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message", return_value=True) as submit_user_message,
                patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
            ):
                resp = handle_send(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "text": "hello codex",
                        "to": ["peer1"],
                        "client_id": "c1",
                    },
                    coerce_bool=lambda value: bool(value),
                    normalize_attachments=lambda _group, _attachments: [],
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _reasons: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            submit_user_message.assert_called_once()
            submitted_text = str(submit_user_message.call_args.kwargs.get("text") or "")
            self.assertIn("[cccc] user → peer1", submitted_text)
            self.assertIn("hello codex", submitted_text)
            self.assertIn(MCP_REMINDER_LINE, submitted_text)
            queue_chat_message.assert_not_called()
            request_flush_pending_messages.assert_not_called()

            event = (resp.result or {}).get("event") if isinstance(resp.result, dict) else {}
            self.assertIsInstance(event, dict)
            assert isinstance(event, dict)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            cursor_event_id, cursor_ts = get_cursor(group, "peer1")
            self.assertEqual(cursor_event_id, before_cursor_event_id)
            self.assertEqual(cursor_ts, before_cursor_ts)
            self.assertEqual(unread_count(group, actor_id="peer1"), 1)

            ledger_events = self._ledger_events(group)
            self.assertFalse(any(str(item.get("kind") or "") == "system.notify" for item in ledger_events))
            self.assertNotEqual(str(ledger_events[-1].get("kind") or ""), "chat.read")

            marked = auto_mark_headless_delivery_started(
                group_id=group_id,
                actor_id="peer1",
                event_id=str(event.get("id") or ""),
                ts=str(event.get("ts") or ""),
            )
            self.assertTrue(marked)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            cursor_event_id, cursor_ts = get_cursor(group, "peer1")
            self.assertEqual(cursor_event_id, str(event.get("id") or ""))
            self.assertEqual(cursor_ts, str(event.get("ts") or ""))
            self.assertEqual(unread_count(group, actor_id="peer1"), 0)

            ledger_events = self._ledger_events(group)
            self.assertEqual(str(ledger_events[-1].get("kind") or ""), "chat.read")
        finally:
            cleanup()

    def test_reply_headless_codex_does_not_leak_original_external_source_into_sender_header(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_reply
        from cccc.kernel.group import load_group
        from cccc.kernel.ledger import append_event
        from cccc.contracts.v1 import ChatMessageData

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-reply-source-header", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            original_event = append_event(
                group.ledger_path,
                kind="chat.message",
                group_id=group.group_id,
                scope_key=str(group.doc.get("active_scope_key") or ""),
                by="user",
                data=ChatMessageData(
                    text="外部用户原话",
                    to=["peer1"],
                    source_platform="dingtalk",
                    source_user_name="Alice",
                    source_user_id="1729",
                ).model_dump(),
            )
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message", return_value=True) as submit_user_message,
                patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
            ):
                resp = handle_reply(
                    {
                        "group_id": group_id,
                        "by": "peer2",
                        "text": "收到，我来处理。",
                        "reply_to": reply_to,
                        "to": ["peer1"],
                    },
                    coerce_bool=lambda value: bool(value),
                    normalize_attachments=lambda _group, _attachments: [],
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _reasons: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            submit_user_message.assert_called_once()
            submitted_text = str(submit_user_message.call_args.kwargs.get("text") or "")
            self.assertIn("[cccc] peer2 → peer1", submitted_text)
            self.assertIn('> "外部用户原话"', submitted_text)
            self.assertIn("收到，我来处理。", submitted_text)
            self.assertNotIn("Alice", submitted_text)
            self.assertNotIn("dingtalk", submitted_text)
            self.assertNotIn("1729", submitted_text)
            queue_chat_message.assert_not_called()
            request_flush_pending_messages.assert_not_called()
        finally:
            cleanup()

    def test_send_routes_headless_codex_image_attachments_to_app_supervisor(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_send

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-send-image", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            attachments = [{"kind": "image", "path": "state/blobs/test.png", "title": "test.png", "mime_type": "image/png"}]

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message") as submit_user_message,
                patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
                patch("cccc.daemon.messaging.chat_ops.get_headless_targets_for_message", return_value=[]),
            ):
                resp = handle_send(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "text": "look at this",
                        "to": ["peer1"],
                    },
                    coerce_bool=lambda value: bool(value),
                    normalize_attachments=lambda _group, _attachments: attachments,
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _reasons: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            submit_user_message.assert_called_once()
            kwargs = submit_user_message.call_args.kwargs
            self.assertEqual(kwargs.get("attachments"), attachments)
            queue_chat_message.assert_not_called()
            request_flush_pending_messages.assert_not_called()
        finally:
            cleanup()

    def test_send_routes_running_pty_codex_actor_to_pty_delivery(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_send

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-send-pty", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message") as submit_user_message,
                patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
                patch("cccc.daemon.messaging.chat_ops.get_headless_targets_for_message", return_value=[]),
            ):
                resp = handle_send(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "text": "hello codex",
                        "to": ["peer1"],
                        "client_id": "c1",
                    },
                    coerce_bool=lambda value: bool(value),
                    normalize_attachments=lambda _group, _attachments: [],
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _reasons: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            submit_user_message.assert_not_called()
            queue_chat_message.assert_called_once()
            request_flush_pending_messages.assert_called_once()
        finally:
            cleanup()

    def test_reply_routes_running_pty_codex_actor_to_pty_delivery(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_reply

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-reply-pty", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            send_resp, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "text": "hello codex",
                    "to": ["peer1"],
                },
            )
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            original_event = (send_resp.result or {}).get("event") if isinstance(send_resp.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            reply_to = str((original_event or {}).get("id") or "").strip()
            self.assertTrue(reply_to)

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message") as submit_user_message,
                patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
                patch("cccc.daemon.messaging.chat_ops.get_headless_targets_for_message", return_value=[]),
            ):
                resp = handle_reply(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "text": "reply codex",
                        "reply_to": reply_to,
                        "to": ["peer1"],
                    },
                    coerce_bool=lambda value: bool(value),
                    normalize_attachments=lambda _group, _attachments: [],
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _reasons: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            submit_user_message.assert_not_called()
            queue_chat_message.assert_called_once()
            request_flush_pending_messages.assert_called_once()
        finally:
            cleanup()

    def test_reply_headless_codex_auto_mark_waits_for_runtime_acceptance(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_reply
        from cccc.daemon.messaging.delivery import MCP_REMINDER_LINE
        from cccc.daemon.messaging.delivery import auto_mark_headless_delivery_started
        from cccc.kernel.group import load_group
        from cccc.kernel.inbox import get_cursor, latest_unread_event, set_cursor, unread_count

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-reply-auto-mark", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            send_resp, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "text": "hello codex",
                    "to": ["peer1"],
                },
            )
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            original_event = (send_resp.result or {}).get("event") if isinstance(send_resp.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            assert isinstance(original_event, dict)
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            group.doc["delivery"] = {"auto_mark_on_delivery": True}
            group.save()

            stale_notify_count = sum(1 for item in self._ledger_events(group) if str(item.get("kind") or "") == "system.notify")
            last_unread = latest_unread_event(group, actor_id="peer1")
            self.assertIsNotNone(last_unread)
            assert last_unread is not None
            set_cursor(
                group,
                "peer1",
                event_id=str(last_unread.get("id") or ""),
                ts=str(last_unread.get("ts") or ""),
            )

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message", return_value=True) as submit_user_message,
                patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
            ):
                resp = handle_reply(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "text": "reply codex",
                        "reply_to": reply_to,
                        "to": ["peer1"],
                    },
                    coerce_bool=lambda value: bool(value),
                    normalize_attachments=lambda _group, _attachments: [],
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _reasons: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            submit_user_message.assert_called_once()
            self.assertIn(MCP_REMINDER_LINE, str(submit_user_message.call_args.kwargs.get("text") or ""))
            queue_chat_message.assert_not_called()
            request_flush_pending_messages.assert_not_called()

            event = (resp.result or {}).get("event") if isinstance(resp.result, dict) else {}
            self.assertIsInstance(event, dict)
            assert isinstance(event, dict)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            cursor_event_id, cursor_ts = get_cursor(group, "peer1")
            self.assertEqual(cursor_event_id, str(last_unread.get("id") or ""))
            self.assertEqual(cursor_ts, str(last_unread.get("ts") or ""))
            self.assertEqual(unread_count(group, actor_id="peer1"), 1)

            ledger_events = self._ledger_events(group)
            notify_count = sum(1 for item in ledger_events if str(item.get("kind") or "") == "system.notify")
            self.assertEqual(notify_count, stale_notify_count)

            marked = auto_mark_headless_delivery_started(
                group_id=group_id,
                actor_id="peer1",
                event_id=str(event.get("id") or ""),
                ts=str(event.get("ts") or ""),
            )
            self.assertTrue(marked)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            cursor_event_id, cursor_ts = get_cursor(group, "peer1")
            self.assertEqual(cursor_event_id, str(event.get("id") or ""))
            self.assertEqual(cursor_ts, str(event.get("ts") or ""))
            self.assertEqual(unread_count(group, actor_id="peer1"), 0)

            ledger_events = self._ledger_events(group)
            self.assertEqual(str(ledger_events[-1].get("kind") or ""), "chat.read")
        finally:
            cleanup()

    def test_reply_routes_running_headless_codex_actor_without_extra_info_notify(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_reply
        from cccc.daemon.messaging.delivery import MCP_REMINDER_LINE
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-reply-headless-direct", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            send_resp, _ = self._call(
                "send",
                {
                    "group_id": group_id,
                    "by": "user",
                    "text": "hello codex",
                    "to": ["peer1"],
                },
            )
            self.assertTrue(send_resp.ok, getattr(send_resp, "error", None))
            original_event = (send_resp.result or {}).get("event") if isinstance(send_resp.result, dict) else {}
            self.assertIsInstance(original_event, dict)
            assert isinstance(original_event, dict)
            reply_to = str(original_event.get("id") or "").strip()
            self.assertTrue(reply_to)

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            stale_notify_count = sum(1 for item in self._ledger_events(group) if str(item.get("kind") or "") == "system.notify")

            with (
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.actor_running", return_value=True),
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message", return_value=True) as submit_user_message,
                patch("cccc.daemon.messaging.chat_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
            ):
                resp = handle_reply(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "text": "reply codex",
                        "reply_to": reply_to,
                        "to": ["peer1"],
                    },
                    coerce_bool=lambda value: bool(value),
                    normalize_attachments=lambda _group, _attachments: [],
                    effective_runner_kind=lambda runner: str(runner or "pty"),
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _group_id, _reasons: None,
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            submit_user_message.assert_called_once()
            self.assertIn(MCP_REMINDER_LINE, str(submit_user_message.call_args.kwargs.get("text") or ""))
            queue_chat_message.assert_not_called()
            request_flush_pending_messages.assert_not_called()

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            notify_count = sum(1 for item in self._ledger_events(group) if str(item.get("kind") or "") == "system.notify")
            self.assertEqual(notify_count, stale_notify_count)
        finally:
            cleanup()

    def test_codex_turn_loop_auto_marks_only_after_runtime_accepts_turn(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._session_state.thread_id = "thread-1"
            session._turn_queue.put_nowait(_PendingTurn(text="hello", event_id="evt-1", ts="2026-04-08T00:00:00Z"))
            session._turn_queue.put_nowait(None)

            with (
                patch.object(session, "is_running", side_effect=[True, True, True]),
                patch.object(session, "_request", return_value={"turn": {"id": "turn-1"}}),
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit"),
                patch.object(session._turn_done, "wait", return_value=True),
                patch("cccc.daemon.codex_app_sessions.auto_mark_headless_delivery_started", return_value=True) as auto_mark,
            ):
                session._turn_loop()

            auto_mark.assert_called_once_with(
                group_id="g_test",
                actor_id="peer1",
                event_id="evt-1",
                ts="2026-04-08T00:00:00Z",
            )
        finally:
            cleanup()

    def test_codex_turn_start_timeout_stops_session_without_idle_overwrite(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )

            class _Proc:
                pid = os.getpid()

                @staticmethod
                def poll():
                    return None

                @staticmethod
                def terminate():
                    return None

                @staticmethod
                def wait(timeout: float | None = None):
                    _ = timeout
                    return 0

            session._proc = _Proc()
            session._running = True
            session._session_state.thread_id = "thread-1"
            session._session_state.status = "idle"
            session._turn_queue.put_nowait(_PendingTurn(text="hello", event_id="evt-1", ts="2026-04-08T00:00:00Z"))

            with (
                patch.object(session, "_request", side_effect=RuntimeError("codex request timed out: turn/start")),
                patch.object(session, "_emit") as emit,
            ):
                session._turn_loop()

            state = session.state()
            self.assertEqual(str(state.get("status") or ""), "stopped")
            self.assertFalse(session.is_running())
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.turn.failed", event_types)
            self.assertIn("headless.session.stopped", event_types)
        finally:
            cleanup()

    def test_claude_turn_loop_auto_marks_only_after_runtime_accepts_turn(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = ClaudeAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._turn_queue.put_nowait(_PendingTurn(text="hello", event_id="evt-1", ts="2026-04-08T00:00:00Z"))
            session._turn_queue.put_nowait(None)

            with (
                patch.object(session, "is_running", side_effect=[True, True, True]),
                patch.object(session, "_write_stdin", return_value=True),
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit"),
                patch.object(session._turn_done, "wait", return_value=True),
                patch("cccc.daemon.claude_app_sessions.auto_mark_headless_delivery_started", return_value=True) as auto_mark,
            ):
                session._turn_loop()

            auto_mark.assert_called_once_with(
                group_id="g_test",
                actor_id="peer1",
                event_id="evt-1",
                ts="2026-04-08T00:00:00Z",
            )
        finally:
            cleanup()

    def test_claude_voice_secretary_control_turn_requeues_when_input_not_consumed(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = ClaudeAppSession(
                group_id="g_test",
                actor_id="voice-secretary",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-voice"
            session._active_event_id = "event-voice"
            session._active_control_kind = "system_notify"
            session._active_payload = _PendingTurn(
                text="read secretary input",
                event_id="event-voice",
                control_kind="system_notify",
                validation_snapshot={"kind": "voice_secretary_input", "before_latest_seq": 8, "before_secretary_read_cursor": 5},
            )

            with (
                patch("cccc.daemon.claude_app_sessions._voice_secretary_control_consumed_input", return_value=False),
                patch(
                    "cccc.daemon.claude_app_sessions._voice_secretary_control_consumption_diagnostics",
                    return_value={"missing": ["secretary_report:req-1"]},
                ),
                patch(
                    "cccc.daemon.claude_app_sessions._voice_secretary_prepare_repair_retry",
                    return_value=(
                        "\n".join(
                            [
                                "read secretary input",
                                "",
                                "[CCCC] SYSTEM REPAIR: read_new_input already ran before this retry.",
                                "[CCCC] FETCHED INPUT:",
                                "Target: secretary",
                            ]
                        ),
                        {"missing": ["secretary_report:req-1"]},
                    ),
                ) as prepare_retry,
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_result_event({"type": "result", "subtype": "success"})

            done_set.assert_called_once()
            queued = session._turn_queue.get_nowait()
            self.assertIsInstance(queued, _PendingTurn)
            assert isinstance(queued, _PendingTurn)
            self.assertEqual(queued.retry_count, 1)
            self.assertEqual(queued.control_kind, "system_notify")
            self.assertIn("SYSTEM REPAIR", queued.text)
            self.assertIn("FETCHED INPUT", queued.text)
            prepare_retry.assert_called_once()
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.requeued", event_types)
            self.assertNotIn("headless.control.completed", event_types)
            self.assertNotIn("headless.control.failed", event_types)
            requeued_payload = next(call.args[1] for call in emit.call_args_list if call.args and call.args[0] == "headless.control.requeued")
            self.assertEqual(requeued_payload.get("status"), "requeued")
        finally:
            cleanup()

    def test_claude_voice_secretary_control_turn_fails_after_retry_when_input_not_consumed(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = ClaudeAppSession(
                group_id="g_test",
                actor_id="voice-secretary",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-voice"
            session._active_event_id = "event-voice"
            session._active_control_kind = "system_notify"
            session._active_payload = _PendingTurn(
                text="read secretary input",
                event_id="event-voice",
                control_kind="system_notify",
                retry_count=1,
                validation_snapshot={"kind": "voice_secretary_input", "before_latest_seq": 8, "before_secretary_read_cursor": 5},
            )

            with (
                patch("cccc.daemon.claude_app_sessions._voice_secretary_control_consumed_input", return_value=False),
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_result_event({"type": "result", "subtype": "success"})

            done_set.assert_called_once()
            self.assertTrue(session._turn_queue.empty())
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.failed", event_types)
            self.assertNotIn("headless.control.completed", event_types)
            self.assertNotIn("headless.control.requeued", event_types)
            failed_payload = next(call.args[1] for call in emit.call_args_list if call.args and call.args[0] == "headless.control.failed")
            self.assertEqual(failed_payload.get("status"), "failed")
        finally:
            cleanup()

    def test_codex_control_turn_uses_control_events_and_skips_auto_mark(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._session_state.thread_id = "thread-1"
            session._turn_queue.put_nowait(_PendingTurn(text="bootstrap", event_id="", control_kind="bootstrap"))
            session._turn_queue.put_nowait(None)

            with (
                patch.object(session, "is_running", side_effect=[True, True, True]),
                patch.object(session, "_request", return_value={"turn": {"id": "turn-bootstrap"}}),
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "wait", return_value=True),
                patch("cccc.daemon.codex_app_sessions.auto_mark_headless_delivery_started") as auto_mark,
            ):
                session._turn_loop()

            auto_mark.assert_not_called()
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertEqual(event_types.count("headless.control.started"), 1)
            self.assertNotIn("headless.turn.started", event_types)
        finally:
            cleanup()

    def test_codex_control_turn_emits_stalled_when_runtime_goes_quiet(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="voice-secretary",
                cwd=Path(home),
                env={},
            )

            class _Proc:
                @staticmethod
                def poll():
                    return None

            session._running = True
            session._proc = _Proc()
            session._session_state.thread_id = "thread-1"
            session._turn_queue.put_nowait(_PendingTurn(text="notify", event_id="evt-1", control_kind="system_notify"))
            session._turn_queue.put_nowait(None)

            with (
                patch.object(session, "_request", return_value={"turn": {"id": "turn-1"}}),
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "wait", side_effect=[False, True]),
                patch("cccc.daemon.codex_app_sessions._TURN_STALL_SECONDS", 0.0),
                patch("cccc.daemon.codex_app_sessions._TURN_WAIT_POLL_SECONDS", 0.0),
                patch("cccc.daemon.codex_app_sessions.auto_mark_headless_delivery_started") as auto_mark,
            ):
                session._turn_loop()

            auto_mark.assert_not_called()
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.started", event_types)
            self.assertIn("headless.control.stalled", event_types)
            stalled_payload = next(call.args[1] for call in emit.call_args_list if call.args and call.args[0] == "headless.control.stalled")
            self.assertEqual(stalled_payload.get("reason"), "runtime_no_events_after_turn_started")
            self.assertEqual(str(session._session_state.status or ""), "waiting")
        finally:
            cleanup()

    def test_codex_control_turn_completion_emits_headless_preview_output(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-bootstrap"
            session._active_control_kind = "bootstrap"

            with (
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_notification(
                    "item/started",
                    {"turnId": "turn-bootstrap", "item": {"type": "agentMessage", "id": "msg-bootstrap", "phase": "final_answer"}},
                )
                session._handle_notification(
                    "item/agentMessage/delta",
                    {"turnId": "turn-bootstrap", "itemId": "msg-bootstrap", "delta": "Hello"},
                )
                session._handle_notification(
                    "item/completed",
                    {"turnId": "turn-bootstrap", "item": {"type": "agentMessage", "id": "msg-bootstrap", "phase": "final_answer", "text": "Hello"}},
                )
                session._handle_notification(
                    "turn/completed",
                    {"turn": {"id": "turn-bootstrap", "status": "completed"}},
                )

            done_set.assert_called_once()
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.completed", event_types)
            self.assertIn("headless.message.started", event_types)
            self.assertIn("headless.message.delta", event_types)
            self.assertIn("headless.message.completed", event_types)
        finally:
            cleanup()

    def test_codex_control_turn_completion_failure_emits_failed_error(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-bootstrap"
            session._active_event_id = "event-bootstrap"
            session._active_control_kind = "system_notify"

            with (
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_notification(
                    "turn/completed",
                    {
                        "turn": {
                            "id": "turn-bootstrap",
                            "status": "failed",
                            "error": {
                                "message": "exceeded retry limit, last status: 429 Too Many Requests",
                            },
                        },
                    },
                )

            done_set.assert_called_once()
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.failed", event_types)
            self.assertNotIn("headless.control.completed", event_types)
            failed_payload = next(call.args[1] for call in emit.call_args_list if call.args and call.args[0] == "headless.control.failed")
            self.assertEqual(str((failed_payload.get("error") or {}).get("message") or ""), "exceeded retry limit, last status: 429 Too Many Requests")
        finally:
            cleanup()

    def test_codex_turn_completion_failure_emits_turn_failed_error(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-1"
            session._active_event_id = "event-1"

            with (
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_notification(
                    "turn/completed",
                    {
                        "turn": {
                            "id": "turn-1",
                            "status": "failed",
                            "error": "exceeded retry limit, last status: 429 Too Many Requests",
                        },
                    },
                )

            done_set.assert_called_once()
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.turn.failed", event_types)
            self.assertNotIn("headless.turn.completed", event_types)
            failed_payload = next(call.args[1] for call in emit.call_args_list if call.args and call.args[0] == "headless.turn.failed")
            self.assertEqual(str((failed_payload.get("error") or {}).get("message") or ""), "exceeded retry limit, last status: 429 Too Many Requests")
        finally:
            cleanup()

    def test_voice_secretary_control_turn_requeues_when_input_not_consumed(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="voice-secretary",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-voice"
            session._active_event_id = "event-voice"
            session._active_control_kind = "system_notify"
            session._active_payload = _PendingTurn(
                text="read secretary input",
                event_id="event-voice",
                control_kind="system_notify",
                validation_snapshot={"kind": "voice_secretary_input", "before_latest_seq": 8, "before_secretary_read_cursor": 5},
            )

            with (
                patch("cccc.daemon.codex_app_sessions._voice_secretary_control_consumed_input", return_value=False),
                patch(
                    "cccc.daemon.codex_app_sessions._voice_secretary_control_consumption_diagnostics",
                    return_value={"missing": ["secretary_report:req-1"]},
                ),
                patch(
                    "cccc.daemon.codex_app_sessions._voice_secretary_prepare_repair_retry",
                    return_value=(
                        "\n".join(["read secretary input", "", "[CCCC] REPAIR HINT:", "- secretary_report:req-1"]),
                        {"missing": ["secretary_report:req-1"]},
                    ),
                ) as prepare_retry,
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_notification(
                    "turn/completed",
                    {"turn": {"id": "turn-voice", "status": "completed"}},
                )

            done_set.assert_called_once()
            queued = session._turn_queue.get_nowait()
            self.assertIsInstance(queued, _PendingTurn)
            assert isinstance(queued, _PendingTurn)
            self.assertEqual(queued.retry_count, 1)
            self.assertEqual(queued.control_kind, "system_notify")
            self.assertIn("REPAIR HINT", queued.text)
            self.assertIn("secretary_report:req-1", queued.text)
            prepare_retry.assert_called_once()
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.requeued", event_types)
            self.assertNotIn("headless.control.completed", event_types)
            self.assertNotIn("headless.control.failed", event_types)
            requeued_payload = next(call.args[1] for call in emit.call_args_list if call.args and call.args[0] == "headless.control.requeued")
            self.assertEqual(requeued_payload.get("status"), "requeued")
        finally:
            cleanup()

    def test_voice_secretary_prepare_repair_retry_only_restates_missing_outputs(self) -> None:
        from cccc.daemon.codex_app_sessions import (
            _voice_secretary_control_failure_reason,
            _voice_secretary_prepare_repair_retry,
        )

        text, diagnostics = _voice_secretary_prepare_repair_retry(
            text="read secretary input",
            diagnostics={"missing": ["secretary_report:req-1"]},
        )

        self.assertEqual(diagnostics, {"missing": ["secretary_report:req-1"]})
        self.assertIn("REPAIR HINT", text)
        self.assertIn("secretary_report:req-1", text)
        self.assertNotIn("FETCHED INPUT", text)
        self.assertEqual(_voice_secretary_control_failure_reason(diagnostics), "voice_secretary_output_not_completed")
        self.assertEqual(
            _voice_secretary_control_failure_reason({"missing": ["read_new_input"]}),
            "voice_secretary_input_not_consumed",
        )

    def test_claude_voice_secretary_prepare_repair_retry_only_restates_missing_outputs(self) -> None:
        from cccc.daemon.claude_app_sessions import (
            _voice_secretary_control_failure_reason,
            _voice_secretary_prepare_repair_retry,
        )

        text, diagnostics = _voice_secretary_prepare_repair_retry(
            text="read secretary input",
            diagnostics={"missing": ["secretary_report:req-1"]},
        )

        self.assertEqual(diagnostics, {"missing": ["secretary_report:req-1"]})
        self.assertIn("REPAIR HINT", text)
        self.assertIn("secretary_report:req-1", text)
        self.assertNotIn("FETCHED INPUT", text)
        self.assertEqual(_voice_secretary_control_failure_reason(diagnostics), "voice_secretary_output_not_completed")
        self.assertEqual(
            _voice_secretary_control_failure_reason({"missing": ["read_new_input"]}),
            "voice_secretary_input_not_consumed",
        )

    def test_voice_secretary_control_turn_does_not_retry_missing_read_new_input(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="voice-secretary",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-voice"
            session._active_event_id = "event-voice"
            session._active_control_kind = "system_notify"
            session._active_payload = _PendingTurn(
                text="read secretary input",
                event_id="event-voice",
                control_kind="system_notify",
                validation_snapshot={"kind": "voice_secretary_input", "before_latest_seq": 8, "before_secretary_read_cursor": 5},
            )

            with (
                patch("cccc.daemon.codex_app_sessions._voice_secretary_control_consumed_input", return_value=False),
                patch(
                    "cccc.daemon.codex_app_sessions._voice_secretary_control_consumption_diagnostics",
                    return_value={"missing": ["read_new_input"]},
                ),
                patch("cccc.daemon.codex_app_sessions._voice_secretary_prepare_repair_retry") as prepare_retry,
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_notification(
                    "turn/completed",
                    {"turn": {"id": "turn-voice", "status": "completed"}},
                )

            done_set.assert_called_once()
            prepare_retry.assert_not_called()
            self.assertTrue(session._turn_queue.empty())
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.failed", event_types)
            self.assertNotIn("headless.control.requeued", event_types)
        finally:
            cleanup()

    def test_claude_voice_secretary_control_turn_does_not_retry_missing_read_new_input(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = ClaudeAppSession(
                group_id="g_test",
                actor_id="voice-secretary",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-voice"
            session._active_event_id = "event-voice"
            session._active_control_kind = "system_notify"
            session._active_payload = _PendingTurn(
                text="read secretary input",
                event_id="event-voice",
                control_kind="system_notify",
                validation_snapshot={"kind": "voice_secretary_input", "before_latest_seq": 8, "before_secretary_read_cursor": 5},
            )

            with (
                patch("cccc.daemon.claude_app_sessions._voice_secretary_control_consumed_input", return_value=False),
                patch(
                    "cccc.daemon.claude_app_sessions._voice_secretary_control_consumption_diagnostics",
                    return_value={"missing": ["read_new_input"]},
                ),
                patch("cccc.daemon.claude_app_sessions._voice_secretary_prepare_repair_retry") as prepare_retry,
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_result_event({"type": "result", "subtype": "success"})

            done_set.assert_called_once()
            prepare_retry.assert_not_called()
            self.assertTrue(session._turn_queue.empty())
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.failed", event_types)
            self.assertNotIn("headless.control.requeued", event_types)
        finally:
            cleanup()

    def test_voice_secretary_control_turn_fails_after_retry_when_input_not_consumed(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="voice-secretary",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-voice"
            session._active_event_id = "event-voice"
            session._active_control_kind = "system_notify"
            session._active_payload = _PendingTurn(
                text="read secretary input",
                event_id="event-voice",
                control_kind="system_notify",
                retry_count=1,
                validation_snapshot={"kind": "voice_secretary_input", "before_latest_seq": 8, "before_secretary_read_cursor": 5},
            )

            with (
                patch("cccc.daemon.codex_app_sessions._voice_secretary_control_consumed_input", return_value=False),
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_notification(
                    "turn/completed",
                    {"turn": {"id": "turn-voice", "status": "completed"}},
                )

            done_set.assert_called_once()
            self.assertTrue(session._turn_queue.empty())
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.failed", event_types)
            self.assertNotIn("headless.control.completed", event_types)
            self.assertNotIn("headless.control.requeued", event_types)
            failed_payload = next(call.args[1] for call in emit.call_args_list if call.args and call.args[0] == "headless.control.failed")
            self.assertEqual(failed_payload.get("status"), "failed")
        finally:
            cleanup()

    def test_voice_secretary_control_turn_requires_read_new_input_for_prompt_draft(self) -> None:
        from cccc.daemon.codex_app_sessions import (
            _voice_secretary_control_consumed_input,
            _voice_secretary_control_consumption_diagnostics,
        )

        home, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "voice-secretary-inline-success", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)
            state_dir = Path(home) / "groups" / group_id / "state"
            state_dir.mkdir(parents=True, exist_ok=True)
            assistants_path = state_dir / "assistants.json"
            assistants_path.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": group_id,
                        "assistants": {},
                        "voice_sessions": {},
                        "voice_ask_requests": {},
                        "voice_prompt_requests": {},
                        "voice_prompt_drafts": {
                            "voice-prompt-1": {
                                "request_id": "voice-prompt-1",
                                "updated_at": "2026-04-20T10:00:01Z",
                                "draft_text": "refined prompt",
                                "status": "pending",
                            }
                        },
                    }
                ),
                encoding="utf-8",
            )
            voice_state_dir = Path(home) / "voice-secretary" / group_id
            voice_state_dir.mkdir(parents=True, exist_ok=True)
            (voice_state_dir / "input_state.json").write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": group_id,
                        "latest_seq": 8,
                        "secretary_read_cursor": 5,
                    }
                ),
                encoding="utf-8",
            )

            draft_without_read_consumed = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 5,
                    "composer_request_ids": ["voice-prompt-1"],
                    "input_target_kinds": ["composer"],
                    "before_prompt_drafts": {
                        "voice-prompt-1": {
                            "updated_at": "",
                            "draft_text": "",
                            "status": "",
                        }
                    },
                },
            )

            self.assertFalse(draft_without_read_consumed)
            prefetched_consumed = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 8,
                    "prefetched_read_new_input": True,
                    "composer_request_ids": ["voice-prompt-1"],
                    "input_target_kinds": ["composer"],
                    "before_prompt_drafts": {
                        "voice-prompt-1": {
                            "updated_at": "",
                            "draft_text": "",
                            "status": "",
                        }
                    },
                },
            )
            self.assertTrue(prefetched_consumed)
            diagnostics = _voice_secretary_control_consumption_diagnostics(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 8,
                    "prefetched_read_new_input": True,
                    "composer_request_ids": ["voice-prompt-1"],
                    "input_target_kinds": ["composer"],
                    "before_prompt_drafts": {
                        "voice-prompt-1": {
                            "updated_at": "",
                            "draft_text": "",
                            "status": "",
                        }
                    },
                },
            )
            self.assertNotIn("read_new_input", diagnostics.get("missing") or [])

            envelope_consumed = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 5,
                    "input_envelope_delivered": True,
                    "delivery_id": f"voice-input:{group_id}:6-8",
                    "composer_request_ids": ["voice-prompt-1"],
                    "input_target_kinds": ["composer"],
                    "before_prompt_drafts": {
                        "voice-prompt-1": {
                            "updated_at": "",
                            "draft_text": "",
                            "status": "",
                        }
                    },
                },
            )
            self.assertTrue(envelope_consumed)
            envelope_diagnostics = _voice_secretary_control_consumption_diagnostics(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 5,
                    "input_envelope_delivered": True,
                    "delivery_id": f"voice-input:{group_id}:6-8",
                    "composer_request_ids": ["voice-prompt-1"],
                    "input_target_kinds": ["composer"],
                    "before_prompt_drafts": {
                        "voice-prompt-1": {
                            "updated_at": "",
                            "draft_text": "",
                            "status": "",
                        }
                    },
                },
            )
            self.assertTrue(envelope_diagnostics.get("input_envelope_delivered"))
            self.assertNotIn("read_new_input", envelope_diagnostics.get("missing") or [])

            (voice_state_dir / "input_state.json").write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": group_id,
                        "latest_seq": 8,
                        "secretary_read_cursor": 8,
                    }
                ),
                encoding="utf-8",
            )
            consumed_after_read = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 5,
                    "composer_request_ids": ["voice-prompt-1"],
                    "input_target_kinds": ["composer"],
                    "before_prompt_drafts": {
                        "voice-prompt-1": {
                            "updated_at": "",
                            "draft_text": "",
                            "status": "",
                        }
                    },
                },
            )
            self.assertTrue(consumed_after_read)

            missing_draft_consumed = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 5,
                    "composer_request_ids": ["voice-prompt-missing"],
                    "input_target_kinds": ["composer"],
                    "before_prompt_drafts": {},
                },
            )
            self.assertFalse(missing_draft_consumed)

            (voice_state_dir / "input_state.json").write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": group_id,
                        "latest_seq": 8,
                        "secretary_read_cursor": 5,
                    }
                ),
                encoding="utf-8",
            )
            mixed_batch_consumed = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 5,
                    "composer_request_ids": ["voice-prompt-1"],
                    "input_target_kinds": ["document", "composer"],
                    "before_prompt_drafts": {
                        "voice-prompt-1": {
                            "updated_at": "",
                            "draft_text": "",
                            "status": "",
                        }
                    },
                },
            )
            self.assertFalse(mixed_batch_consumed)

            (voice_state_dir / "input_state.json").write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": group_id,
                        "latest_seq": 9,
                        "secretary_read_cursor": 9,
                    }
                ),
                encoding="utf-8",
            )
            missing_ask_report_consumed = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 9,
                    "before_secretary_read_cursor": 5,
                    "secretary_request_ids": ["voice-ask-1"],
                    "input_target_kinds": ["secretary"],
                    "before_ask_requests": {},
                },
            )
            self.assertFalse(missing_ask_report_consumed)

            assistants_path.write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "group_id": group_id,
                        "assistants": {},
                        "voice_sessions": {},
                        "voice_prompt_requests": {},
                        "voice_prompt_drafts": {},
                        "voice_ask_requests": {
                            "voice-ask-1": {
                                "request_id": "voice-ask-1",
                                "updated_at": "2026-04-20T10:00:03Z",
                                "reply_text": "已检查，没有明显遗漏。",
                                "status": "done",
                            },
                            "voice-ask-empty": {
                                "request_id": "voice-ask-empty",
                                "updated_at": "2026-04-20T10:00:04Z",
                                "reply_text": "",
                                "status": "done",
                            },
                            "voice-doc-1": {
                                "request_id": "voice-doc-1",
                                "updated_at": "2026-04-20T10:00:05Z",
                                "reply_text": "已更新目标文档。",
                                "status": "done",
                            },
                        },
                    }
                ),
                encoding="utf-8",
            )
            ask_report_consumed = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 9,
                    "before_secretary_read_cursor": 5,
                    "secretary_request_ids": ["voice-ask-1"],
                    "input_target_kinds": ["secretary"],
                    "before_ask_requests": {
                        "voice-ask-1": {
                            "updated_at": "2026-04-20T10:00:01Z",
                            "reply_text": "",
                            "status": "working",
                        }
                    },
                },
            )
            self.assertTrue(ask_report_consumed)

            ask_report_without_reply_consumed = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 9,
                    "before_secretary_read_cursor": 5,
                    "secretary_request_ids": ["voice-ask-empty"],
                    "input_target_kinds": ["secretary"],
                    "before_ask_requests": {},
                },
            )
            self.assertFalse(ask_report_without_reply_consumed)

            document_report_consumed = _voice_secretary_control_consumed_input(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 9,
                    "before_secretary_read_cursor": 5,
                    "report_request_ids": ["voice-doc-1"],
                    "input_target_kinds": ["document"],
                    "before_ask_requests": {
                        "voice-doc-1": {
                            "updated_at": "2026-04-20T10:00:01Z",
                            "reply_text": "",
                            "status": "working",
                        }
                    },
                },
            )
            self.assertTrue(document_report_consumed)
        finally:
            cleanup()

    def test_codex_voice_secretary_prepare_control_turn_prefetches_input(self) -> None:
        from cccc.contracts.v1 import DaemonResponse
        from cccc.daemon.codex_app_sessions import _voice_secretary_prepare_control_turn

        with (
            patch(
                "cccc.daemon.codex_app_sessions._voice_secretary_control_snapshot",
                return_value={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 5,
                },
            ),
            patch(
                "cccc.daemon.assistants.assistant_ops.handle_assistant_voice_document_input_read",
                return_value=DaemonResponse(
                    ok=True,
                    result={
                        "input_text": "Target: secretary\nRequest ID: req-1",
                        "input_batches": [{"target_kind": "secretary", "request_ids": ["req-1"]}],
                        "input_timing": {"latest_seq": 8, "secretary_read_cursor": 8},
                    },
                ),
            ) as read_input,
        ):
            payload_text, snapshot = _voice_secretary_prepare_control_turn(
                group_id="g_test",
                actor_id="voice-secretary",
                text="read secretary input",
                event_id="evt-1",
                control_kind="system_notify",
            )

        read_input.assert_called_once_with({"group_id": "g_test", "by": "voice-secretary"})
        self.assertIn("SYSTEM PREFETCH", payload_text)
        self.assertIn("FETCHED INPUT", payload_text)
        self.assertIn("Target: secretary", payload_text)
        self.assertTrue(snapshot.get("prefetched_read_new_input"))

    def test_codex_voice_secretary_prepare_control_turn_uses_inline_envelope_without_prefetch(self) -> None:
        from cccc.daemon.codex_app_sessions import _voice_secretary_prepare_control_turn

        with (
            patch(
                "cccc.daemon.codex_app_sessions._voice_secretary_control_snapshot",
                return_value={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 8,
                    "input_envelope_delivered": True,
                    "delivery_id": "voice-input:g_test:6-8",
                    "composer_request_ids": ["req-1"],
                    "input_target_kinds": ["composer"],
                    "before_prompt_drafts": {},
                },
            ),
            patch("cccc.daemon.assistants.assistant_ops.handle_assistant_voice_document_input_read") as read_input,
        ):
            payload_text, snapshot = _voice_secretary_prepare_control_turn(
                group_id="g_test",
                actor_id="voice-secretary",
                text="Voice Secretary input: 1 item.\n\nTarget: composer\nRequest id: req-1",
                event_id="evt-1",
                control_kind="system_notify",
            )

        read_input.assert_not_called()
        self.assertIn("Target: composer", payload_text)
        self.assertNotIn("Input envelope", payload_text)
        self.assertNotIn("SYSTEM PREFETCH", payload_text)
        self.assertTrue(snapshot.get("input_envelope_delivered"))

    def test_claude_voice_secretary_prepare_control_turn_prefetches_input(self) -> None:
        from cccc.contracts.v1 import DaemonResponse
        from cccc.daemon.claude_app_sessions import _voice_secretary_prepare_control_turn

        with (
            patch(
                "cccc.daemon.claude_app_sessions._voice_secretary_control_snapshot",
                return_value={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 8,
                    "before_secretary_read_cursor": 5,
                },
            ),
            patch(
                "cccc.daemon.assistants.assistant_ops.handle_assistant_voice_document_input_read",
                return_value=DaemonResponse(
                    ok=True,
                    result={
                        "input_text": "Target: secretary\nRequest ID: req-1",
                        "input_batches": [{"target_kind": "secretary", "request_ids": ["req-1"]}],
                        "input_timing": {"latest_seq": 8, "secretary_read_cursor": 8},
                    },
                ),
            ) as read_input,
        ):
            payload_text, snapshot = _voice_secretary_prepare_control_turn(
                group_id="g_test",
                actor_id="voice-secretary",
                text="read secretary input",
                event_id="evt-1",
                control_kind="system_notify",
            )

        read_input.assert_called_once_with({"group_id": "g_test", "by": "voice-secretary"})
        self.assertIn("SYSTEM PREFETCH", payload_text)
        self.assertIn("FETCHED INPUT", payload_text)
        self.assertIn("Target: secretary", payload_text)
        self.assertTrue(snapshot.get("prefetched_read_new_input"))

    def test_emit_system_notify_routes_running_headless_codex_actor_to_control_turn(self) -> None:
        from cccc.contracts.v1 import SystemNotifyData
        from cccc.daemon.messaging.delivery import emit_system_notify
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-notify-control", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()

            add_resp, _ = self._call(
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
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            with (
                patch("cccc.daemon.codex_app_sessions.SUPERVISOR.actor_running", return_value=True),
                patch("cccc.daemon.codex_app_sessions.SUPERVISOR.submit_control_message", return_value=True) as submit_control_message,
            ):
                event = emit_system_notify(
                    group,
                    by="system",
                    notify=SystemNotifyData(
                        kind="info",
                        priority="high",
                        title="Need review",
                        message="Please refresh your inbox.",
                        target_actor_id="peer1",
                        requires_ack=False,
                    ),
                )

            submit_control_message.assert_called_once()
            kwargs = submit_control_message.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), group_id)
            self.assertEqual(kwargs.get("actor_id"), "peer1")
            self.assertEqual(kwargs.get("control_kind"), "system_notify")
            self.assertEqual(kwargs.get("event_id"), str(event.get("id") or ""))
            self.assertIn("Need review", str(kwargs.get("text") or ""))
        finally:
            cleanup()

    def test_emit_system_notify_routes_running_headless_claude_actor_to_control_turn(self) -> None:
        from cccc.contracts.v1 import SystemNotifyData
        from cccc.daemon.messaging.delivery import emit_system_notify
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "claude-notify-control", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()

            add_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer1",
                    "title": "Peer 1",
                    "runtime": "claude",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None

            with (
                patch("cccc.daemon.claude_app_sessions.SUPERVISOR.actor_running", return_value=True),
                patch("cccc.daemon.claude_app_sessions.SUPERVISOR.submit_control_message", return_value=True) as submit_control_message,
            ):
                event = emit_system_notify(
                    group,
                    by="system",
                    notify=SystemNotifyData(
                        kind="info",
                        priority="high",
                        title="Need review",
                        message="Please refresh your inbox.",
                        target_actor_id="peer1",
                        requires_ack=False,
                    ),
                )

            submit_control_message.assert_called_once()
            kwargs = submit_control_message.call_args.kwargs
            self.assertEqual(kwargs.get("group_id"), group_id)
            self.assertEqual(kwargs.get("actor_id"), "peer1")
            self.assertEqual(kwargs.get("control_kind"), "system_notify")
            self.assertEqual(kwargs.get("event_id"), str(event.get("id") or ""))
            self.assertIn("Need review", str(kwargs.get("text") or ""))
        finally:
            cleanup()

    def test_codex_notifications_write_stream_events_without_auto_materializing_chat_message(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.kernel.headless_events import headless_events_path
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="codex-session", topic="")
            loaded_group = load_group(group.group_id)
            self.assertIsNotNone(loaded_group)
            assert loaded_group is not None

            session = CodexAppSession(
                group_id=group.group_id,
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._active_event_id = "evt-1"

            session._handle_notification("turn/started", {"turn": {"id": "turn-1"}})
            session._handle_notification(
                "item/started",
                {"turnId": "turn-1", "item": {"type": "agentMessage", "id": "commentary-1", "phase": "commentary"}},
            )
            session._handle_notification("item/agentMessage/delta", {"turnId": "turn-1", "itemId": "commentary-1", "delta": "Inspecting"})
            session._handle_notification(
                "item/completed",
                {"turnId": "turn-1", "item": {"type": "agentMessage", "id": "commentary-1", "phase": "commentary", "text": "Inspecting"}},
            )
            session._handle_notification("turn/plan/updated", {"turnId": "turn-1", "plan": [{"step": "Inspect src/app.ts", "status": "in_progress"}]})
            session._handle_notification("item/plan/delta", {"turnId": "turn-1", "itemId": "plan-1", "delta": "Refine reducer merge"})
            session._handle_notification("item/started", {"turnId": "turn-1", "item": {"type": "reasoning", "id": "rs-1"}})
            session._handle_notification("item/reasoning/summaryTextDelta", {"turnId": "turn-1", "itemId": "rs-1", "delta": "Inspecting state flow"})
            session._handle_notification("item/reasoning/textDelta", {"turnId": "turn-1", "itemId": "rs-1", "delta": "Need to keep command metadata"})
            session._handle_notification(
                "item/started",
                {"turnId": "turn-1", "item": {"type": "commandExecution", "id": "cmd-1", "command": "npm run typecheck", "commandActions": [], "cwd": "/tmp", "status": "in_progress"}},
            )
            session._handle_notification("item/commandExecution/outputDelta", {"turnId": "turn-1", "itemId": "cmd-1", "delta": "typecheck started"})
            session._handle_notification("item/commandExecution/terminalInteraction", {"turnId": "turn-1", "itemId": "cmd-1", "stdin": "y\n"})
            session._handle_notification(
                "item/started",
                {"turnId": "turn-1", "item": {"type": "agentMessage", "id": "msg-1", "phase": "final_answer"}},
            )
            session._handle_notification("item/agentMessage/delta", {"turnId": "turn-1", "itemId": "msg-1", "delta": "Hel"})
            session._handle_notification("item/agentMessage/delta", {"turnId": "turn-1", "itemId": "msg-1", "delta": "lo"})
            session._handle_notification(
                "item/completed",
                {"turnId": "turn-1", "item": {"type": "commandExecution", "id": "cmd-1", "command": "npm run typecheck", "commandActions": [], "cwd": "/tmp", "status": "completed"}},
            )
            session._handle_notification(
                "item/completed",
                {"turnId": "turn-1", "item": {"type": "agentMessage", "id": "msg-1", "phase": "final_answer", "text": "Hello"}},
            )
            session._handle_notification(
                "item/completed",
                {"turnId": "turn-1", "item": {"type": "agentMessage", "id": "msg-1", "phase": "final_answer", "text": "Hello"}},
            )
            session._handle_notification("turn/completed", {"turn": {"id": "turn-1", "status": "completed"}})

            events_path = headless_events_path(loaded_group.path)
            self.assertTrue(events_path.exists())
            headless_events = [
                json.loads(line)
                for line in events_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            event_types = [str(item.get("type") or "") for item in headless_events]
            self.assertIn("headless.turn.progress", event_types)
            self.assertIn("headless.activity.started", event_types)
            self.assertIn("headless.activity.updated", event_types)
            self.assertIn("headless.activity.completed", event_types)
            self.assertIn("headless.message.started", event_types)

            self.assertIn("headless.message.delta", event_types)
            self.assertIn("headless.message.completed", event_types)
            self.assertIn("headless.turn.completed", event_types)
            commentary_started = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.message.started"
                and str(((item.get("data") or {}).get("phase") or "")) == "commentary"
            )
            self.assertEqual(str(((commentary_started.get("data") or {}).get("stream_id") or "")), "commentary-1")
            commentary_completed = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.message.completed"
                and str(((item.get("data") or {}).get("phase") or "")) == "commentary"
            )
            self.assertEqual(str(((commentary_completed.get("data") or {}).get("text") or "")), "Inspecting")
            message_started = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.message.started"
                and str(((item.get("data") or {}).get("phase") or "")) == "final_answer"
            )
            started_data = message_started.get("data") if isinstance(message_started.get("data"), dict) else {}
            self.assertEqual(str(started_data.get("event_id") or ""), "evt-1")
            activity_summaries = [
                str((item.get("data") or {}).get("summary") or "")
                for item in headless_events
                if str(item.get("type") or "").startswith("headless.activity.")
            ]
            self.assertIn("Inspect src/app.ts", activity_summaries)
            self.assertIn("Refine reducer merge", activity_summaries)
            self.assertIn("Inspecting state flow", activity_summaries)
            self.assertIn("Need to keep command metadata", activity_summaries)
            self.assertIn("npm run typecheck", activity_summaries)
            self.assertIn("terminal input", activity_summaries)
            self.assertIn("reply ready", activity_summaries)
            command_started = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.activity.started"
                and str(((item.get("data") or {}).get("activity_id") or "")).startswith("command:")
            )
            command_data = command_started.get("data") if isinstance(command_started.get("data"), dict) else {}
            self.assertEqual(str(command_data.get("raw_item_type") or ""), "commandExecution")
            self.assertEqual(str(command_data.get("command") or ""), "npm run typecheck")
            self.assertEqual(str(command_data.get("cwd") or ""), "/tmp")

            command_updates = [
                item.get("data") if isinstance(item.get("data"), dict) else {}
                for item in headless_events
                if str(item.get("type") or "") == "headless.activity.updated"
                and str(((item.get("data") or {}).get("activity_id") or "")).startswith("command:")
            ]
            self.assertTrue(any(str(data.get("summary") or "") == "typecheck started" for data in command_updates))
            self.assertTrue(any(str(data.get("summary") or "") == "terminal input" for data in command_updates))
            self.assertTrue(all(str(data.get("command") or "") == "npm run typecheck" for data in command_updates))
            self.assertTrue(all(str(data.get("cwd") or "") == "/tmp" for data in command_updates))

            ledger_events = [
                json.loads(line)
                for line in loaded_group.ledger_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            stream_events = [event for event in ledger_events if str(event.get("kind") or "") == "chat.stream"]
            self.assertEqual(stream_events, [])
            chat_messages = [event for event in ledger_events if str(event.get("kind") or "") == "chat.message"]
            self.assertEqual(chat_messages, [])
        finally:
            cleanup()

    def test_codex_session_build_turn_input_items_includes_local_image_blob(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _PendingTurn
        from cccc.kernel.blobs import store_blob_bytes
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="codex-image-input", topic="")
            stored = store_blob_bytes(
                group,
                data=b"\x89PNG\r\n\x1a\nfake-png",
                filename="image.png",
                mime_type="image/png",
                kind="image",
            )
            session = CodexAppSession(
                group_id=group.group_id,
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            items = session._build_turn_input_items(
                _PendingTurn(
                    text="请看图",
                    event_id="evt-1",
                    attachments=[stored],
                )
            )

            self.assertEqual(items[0], {"type": "text", "text": "请看图"})
            self.assertEqual(items[1]["type"], "local_image")
            self.assertEqual(items[1]["path"], str(group.path / str(stored.get("path") or "")))
        finally:
            cleanup()

    def test_claude_session_compose_user_content_includes_local_image_blob_path(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession, _PendingTurn
        from cccc.kernel.blobs import store_blob_bytes
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="claude-image-input", topic="")
            stored = store_blob_bytes(
                group,
                data=b"\x89PNG\r\n\x1a\nfake-png",
                filename="image.png",
                mime_type="image/png",
                kind="image",
            )
            session = ClaudeAppSession(
                group_id=group.group_id,
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            content = session._compose_user_content(
                _PendingTurn(
                    text="请看图",
                    event_id="evt-1",
                    attachments=[stored],
                )
            )

            self.assertIn("请看图", content)
            self.assertIn("Claude stream-json 当前仅支持文本输入", content)
            self.assertIn(str(group.path / str(stored.get("path") or "")), content)
        finally:
            cleanup()

    def test_codex_notifications_keep_streaming_when_agent_message_phase_missing(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.kernel.headless_events import headless_events_path
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="codex-session-phase-fallback", topic="")
            loaded_group = load_group(group.group_id)
            self.assertIsNotNone(loaded_group)
            assert loaded_group is not None

            session = CodexAppSession(
                group_id=group.group_id,
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._active_event_id = "evt-fallback"

            session._handle_notification("turn/started", {"turn": {"id": "turn-fallback"}})
            session._handle_notification(
                "item/started",
                {"turnId": "turn-fallback", "item": {"type": "agentMessage", "id": "msg-fallback"}},
            )
            session._handle_notification(
                "item/agentMessage/delta",
                {"turnId": "turn-fallback", "itemId": "msg-fallback", "delta": "Hel"},
            )
            session._handle_notification(
                "item/agentMessage/delta",
                {"turnId": "turn-fallback", "itemId": "msg-fallback", "delta": "lo"},
            )
            session._handle_notification(
                "item/completed",
                {
                    "turnId": "turn-fallback",
                    "item": {"type": "agentMessage", "id": "msg-fallback", "text": "Hello"},
                },
            )
            session._handle_notification("turn/completed", {"turn": {"id": "turn-fallback", "status": "completed"}})

            events_path = headless_events_path(loaded_group.path)
            self.assertTrue(events_path.exists())
            headless_events = [
                json.loads(line)
                for line in events_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            started = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.message.started"
                and str(((item.get("data") or {}).get("stream_id") or "")) == "msg-fallback"
            )
            started_data = started.get("data") if isinstance(started.get("data"), dict) else {}
            self.assertEqual(str(started_data.get("phase") or ""), "")

            deltas = [
                item for item in headless_events
                if str(item.get("type") or "") == "headless.message.delta"
                and str(((item.get("data") or {}).get("stream_id") or "")) == "msg-fallback"
            ]
            self.assertEqual([str((item.get("data") or {}).get("delta") or "") for item in deltas], ["Hel", "lo"])

            completed = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.message.completed"
                and str(((item.get("data") or {}).get("stream_id") or "")) == "msg-fallback"
            )
            completed_data = completed.get("data") if isinstance(completed.get("data"), dict) else {}
            self.assertEqual(str(completed_data.get("text") or ""), "Hello")
            self.assertEqual(str(completed_data.get("phase") or ""), "")

            ledger_events = [
                json.loads(line)
                for line in loaded_group.ledger_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            stream_events = [event for event in ledger_events if str(event.get("kind") or "") == "chat.stream"]
            self.assertEqual(stream_events, [])
            chat_messages = [event for event in ledger_events if str(event.get("kind") or "") == "chat.message"]
            self.assertEqual(chat_messages, [])
        finally:
            cleanup()

    def test_claude_stream_completion_emits_headless_events_without_auto_chat_message(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession
        from cccc.kernel.headless_events import headless_events_path
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="claude-stream-complete", topic="")
            loaded_group = load_group(group.group_id)
            self.assertIsNotNone(loaded_group)
            assert loaded_group is not None

            session = ClaudeAppSession(
                group_id=group.group_id,
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-claude"
            session._active_event_id = "evt-claude"

            session._handle_stream_event({"event": {"type": "message_start", "message": {"id": "msg-claude"}}})
            session._handle_stream_event(
                {"event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "Hello"}}}
            )
            session._handle_stream_event({"event": {"type": "message_delta", "delta": {"stop_reason": "end_turn"}}})
            session._handle_stream_event({"event": {"type": "message_stop"}})

            events_path = headless_events_path(loaded_group.path)
            self.assertTrue(events_path.exists())
            headless_events = [
                json.loads(line)
                for line in events_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            event_types = [str(item.get("type") or "") for item in headless_events]
            self.assertIn("headless.message.completed", event_types)
            self.assertIn("headless.turn.completed", event_types)

            ledger_events = [
                json.loads(line)
                for line in loaded_group.ledger_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]
            stream_events = [event for event in ledger_events if str(event.get("kind") or "") == "chat.stream"]
            self.assertEqual(stream_events, [])
            chat_messages = [event for event in ledger_events if str(event.get("kind") or "") == "chat.message"]
            self.assertEqual(chat_messages, [])
        finally:
            cleanup()

    def test_claude_stream_control_turn_requeues_when_input_not_consumed(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = ClaudeAppSession(
                group_id="g_test",
                actor_id="voice-secretary",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-claude"
            session._active_event_id = "evt-claude"
            session._active_control_kind = "system_notify"
            session._active_payload = _PendingTurn(
                text="read secretary input",
                event_id="evt-claude",
                control_kind="system_notify",
                validation_snapshot={"kind": "voice_secretary_input", "before_latest_seq": 8, "before_secretary_read_cursor": 5},
            )

            with (
                patch("cccc.daemon.claude_app_sessions._voice_secretary_control_consumed_input", return_value=False),
                patch(
                    "cccc.daemon.claude_app_sessions._voice_secretary_control_consumption_diagnostics",
                    return_value={"missing": ["secretary_report:req-1"]},
                ),
                patch(
                    "cccc.daemon.claude_app_sessions._voice_secretary_prepare_repair_retry",
                    return_value=(
                        "\n".join(
                            [
                                "read secretary input",
                                "",
                                "[CCCC] SYSTEM REPAIR: read_new_input already ran before this retry.",
                                "[CCCC] FETCHED INPUT:",
                                "Target: secretary",
                            ]
                        ),
                        {"missing": ["secretary_report:req-1"]},
                    ),
                ) as prepare_retry,
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_stream_event({"event": {"type": "message_start", "message": {"id": "msg-claude"}}})
                session._handle_stream_event({"event": {"type": "message_delta", "delta": {"stop_reason": "end_turn"}}})
                session._handle_stream_event({"event": {"type": "message_stop"}})

            done_set.assert_called_once()
            self.assertEqual(session._turn_queue.qsize(), 1)
            queued = session._turn_queue.get_nowait()
            assert isinstance(queued, _PendingTurn)
            self.assertEqual(queued.retry_count, 1)
            self.assertEqual(queued.control_kind, "system_notify")
            self.assertIn("SYSTEM REPAIR", queued.text)
            self.assertIn("FETCHED INPUT", queued.text)
            prepare_retry.assert_called_once()
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.control.requeued", event_types)
            self.assertNotIn("headless.control.completed", event_types)
            self.assertNotIn("headless.message.completed", event_types)
            requeued_payload = next(call.args[1] for call in emit.call_args_list if call.args and call.args[0] == "headless.control.requeued")
            self.assertEqual(requeued_payload.get("status"), "requeued")
        finally:
            cleanup()

    def test_claude_stream_control_turn_emits_headless_preview_output(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession

        home, cleanup = self._with_home()
        try:
            session = ClaudeAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-claude"
            session._active_event_id = "evt-claude"
            session._active_control_kind = "system_notify"

            with (
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_stream_event({"event": {"type": "message_start", "message": {"id": "msg-claude"}}})
                session._handle_stream_event(
                    {"event": {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "Hello"}}}
                )
                session._handle_stream_event({"event": {"type": "message_delta", "delta": {"stop_reason": "end_turn"}}})
                session._handle_stream_event({"event": {"type": "message_stop"}})

            done_set.assert_called_once()
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.message.started", event_types)
            self.assertIn("headless.message.delta", event_types)
            self.assertIn("headless.message.completed", event_types)
            self.assertIn("headless.control.completed", event_types)
        finally:
            cleanup()

    def test_claude_session_emits_rich_tool_hook_task_and_summary_activity(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession
        from cccc.kernel.headless_events import headless_events_path
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="claude-rich-activity", topic="")
            loaded_group = load_group(group.group_id)
            self.assertIsNotNone(loaded_group)
            assert loaded_group is not None

            session = ClaudeAppSession(
                group_id=group.group_id,
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-claude"
            session._active_event_id = "evt-claude"

            session._handle_assistant_event(
                {
                    "type": "assistant",
                    "partial": True,
                    "message": {
                        "id": "msg-tool",
                        "content": [
                            {
                                "type": "tool_use",
                                "id": "tool-1",
                                "name": "Bash",
                                "input": {
                                    "command": "npm run typecheck",
                                    "cwd": "/tmp",
                                },
                            }
                        ],
                    },
                }
            )
            session._handle_event(
                {
                    "type": "tool_progress",
                    "tool_use_id": "tool-1",
                    "tool_name": "Bash",
                    "elapsed_time_seconds": 4,
                    "uuid": "tool-progress-1",
                    "session_id": "sess-1",
                }
            )
            session._handle_event(
                {
                    "type": "tool_result",
                    "tool_use_id": "tool-1",
                    "tool_name": "Bash",
                    "content": "typecheck clean",
                }
            )
            session._handle_assistant_event(
                {
                    "type": "assistant",
                    "partial": True,
                    "message": {
                        "id": "msg-search",
                        "content": [
                            {
                                "type": "tool_use",
                                "id": "tool-2",
                                "name": "Grep",
                                "input": {
                                    "pattern": "headless.activity",
                                    "path": "/tmp/src",
                                    "glob": "**/*.py",
                                },
                            }
                        ],
                    },
                }
            )
            session._handle_system_event(
                {
                    "type": "system",
                    "subtype": "hook_started",
                    "hook_id": "hook-1",
                    "hook_name": "PostToolUse",
                    "hook_event": "post_tool_use",
                    "uuid": "hook-start-1",
                    "session_id": "sess-1",
                }
            )
            session._handle_system_event(
                {
                    "type": "system",
                    "subtype": "hook_progress",
                    "hook_id": "hook-1",
                    "hook_name": "PostToolUse",
                    "hook_event": "post_tool_use",
                    "output": "collecting trace",
                    "stdout": "",
                    "stderr": "",
                    "uuid": "hook-progress-1",
                    "session_id": "sess-1",
                }
            )
            session._handle_system_event(
                {
                    "type": "system",
                    "subtype": "hook_response",
                    "hook_id": "hook-1",
                    "hook_name": "PostToolUse",
                    "hook_event": "post_tool_use",
                    "output": "hook complete",
                    "stdout": "",
                    "stderr": "",
                    "outcome": "success",
                    "uuid": "hook-response-1",
                    "session_id": "sess-1",
                }
            )
            session._handle_system_event(
                {
                    "type": "system",
                    "subtype": "task_started",
                    "task_id": "task-1",
                    "description": "Audit reducers",
                    "prompt": "Inspect group reducers",
                    "uuid": "task-start-1",
                    "session_id": "sess-1",
                }
            )
            session._handle_system_event(
                {
                    "type": "system",
                    "subtype": "task_progress",
                    "task_id": "task-1",
                    "description": "Audit reducers",
                    "summary": "Mapped activity merge path",
                    "uuid": "task-progress-1",
                    "session_id": "sess-1",
                }
            )
            session._handle_system_event(
                {
                    "type": "system",
                    "subtype": "task_notification",
                    "task_id": "task-1",
                    "status": "completed",
                    "summary": "Reducer audit finished",
                    "output_file": "/tmp/task-1.txt",
                    "uuid": "task-done-1",
                    "session_id": "sess-1",
                }
            )
            session._handle_event(
                {
                    "type": "tool_use_summary",
                    "summary": "Read 2 files, wrote 1 file",
                    "preceding_tool_use_ids": ["tool-1"],
                    "uuid": "tool-summary-1",
                    "session_id": "sess-1",
                }
            )

            events_path = headless_events_path(loaded_group.path)
            self.assertTrue(events_path.exists())
            headless_events = [
                json.loads(line)
                for line in events_path.read_text(encoding="utf-8").splitlines()
                if line.strip()
            ]

            command_started = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.activity.started"
                and str(((item.get("data") or {}).get("activity_id") or "")) == "tool:tool-1"
            )
            command_started_data = command_started.get("data") if isinstance(command_started.get("data"), dict) else {}
            self.assertEqual(str(command_started_data.get("summary") or ""), "npm run typecheck")
            self.assertEqual(str(command_started_data.get("kind") or ""), "command")
            self.assertEqual(str(command_started_data.get("command") or ""), "npm run typecheck")
            self.assertEqual(str(command_started_data.get("cwd") or ""), "/tmp")

            command_progress = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.activity.updated"
                and str(((item.get("data") or {}).get("activity_id") or "")) == "tool:tool-1"
            )
            command_progress_data = command_progress.get("data") if isinstance(command_progress.get("data"), dict) else {}
            self.assertEqual(str(command_progress_data.get("detail") or ""), "running for 4s")

            command_completed = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.activity.completed"
                and str(((item.get("data") or {}).get("activity_id") or "")) == "tool:tool-1"
            )
            command_completed_data = command_completed.get("data") if isinstance(command_completed.get("data"), dict) else {}
            self.assertEqual(str(command_completed_data.get("summary") or ""), "npm run typecheck")
            self.assertEqual(str(command_completed_data.get("detail") or ""), "typecheck clean")

            search_started = next(
                item for item in headless_events
                if str(item.get("type") or "") == "headless.activity.started"
                and str(((item.get("data") or {}).get("activity_id") or "")) == "tool:tool-2"
            )
            search_started_data = search_started.get("data") if isinstance(search_started.get("data"), dict) else {}
            self.assertEqual(str(search_started_data.get("summary") or ""), "headless.activity")
            self.assertEqual(str(search_started_data.get("kind") or ""), "search")
            self.assertEqual(str(search_started_data.get("query") or ""), "headless.activity")
            self.assertEqual(str(search_started_data.get("detail") or ""), "in /tmp/src, glob **/*.py")
            self.assertFalse(search_started_data.get("file_paths"))

            activity_summaries = [
                str((item.get("data") or {}).get("summary") or "")
                for item in headless_events
                if str(item.get("type") or "").startswith("headless.activity.")
            ]
            self.assertIn("PostToolUse", activity_summaries)
            self.assertIn("Audit reducers", activity_summaries)
            self.assertIn("Reducer audit finished", activity_summaries)
            self.assertIn("Read 2 files, wrote 1 file", activity_summaries)
        finally:
            cleanup()

    def test_actor_activity_thread_emits_running_headless_codex_actor(self) -> None:
        from cccc.daemon.serve_ops import start_actor_activity_thread
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            created = create_group(reg, title="codex-activity", topic="")
            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", title="Peer 1", runtime="codex", runner="headless")  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            published: list[dict] = []

            class _Broadcaster:
                def publish(self, event: dict) -> None:
                    published.append(event)

            stop_event = threading.Event()
            thread = start_actor_activity_thread(
                stop_event=stop_event,
                home=Path(home),
                pty_supervisor=object(),
                headless_supervisor=object(),
                codex_supervisor=type(
                    "_CodexSupervisor",
                    (),
                    {
                        "get_state": staticmethod(lambda group_id, actor_id: {
                            "group_id": group_id,
                            "actor_id": actor_id,
                            "status": "working",
                            "current_task_id": "turn-1",
                            "updated_at": "2026-04-02T10:00:00Z",
                        }),
                        "actor_running": staticmethod(lambda _group_id, _actor_id: True),
                    },
                )(),
                event_broadcaster=_Broadcaster(),
                load_group=load_group,
                interval_seconds=1.0,
            )
            time.sleep(0.15)
            stop_event.set()
            thread.join(timeout=1.0)

            actor_events = [event for event in published if str(event.get("kind") or "") == "actor.activity"]
            self.assertTrue(actor_events)
            actors = ((actor_events[-1].get("data") or {}).get("actors") or [])
            self.assertEqual(len(actors), 1)
            self.assertEqual(str(actors[0].get("id") or ""), "peer1")
            self.assertTrue(bool(actors[0].get("running")))
            self.assertEqual(str(actors[0].get("effective_working_state") or ""), "working")
        finally:
            cleanup()

    def test_actor_activity_thread_writes_ledger_on_state_change(self) -> None:
        """actor.activity should be written to ledger on working-state transitions."""
        from cccc.daemon.serve_ops import start_actor_activity_thread
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            created = create_group(reg, title="codex-ledger-activity", topic="")
            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", title="Peer 1", runtime="codex", runner="headless")  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            # Re-load to get fresh ledger_path
            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            ledger_path = group.ledger_path  # type: ignore[union-attr]

            class _Broadcaster:
                def publish(self, event: dict) -> None:
                    pass

            status_holder = {"status": "working", "running": True}

            class _CodexSupervisor:
                @staticmethod
                def get_state(group_id: str, actor_id: str) -> dict:
                    return {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "status": status_holder["status"],
                        "current_task_id": "turn-1",
                        "updated_at": "2026-04-02T10:00:00Z",
                    }

                @staticmethod
                def actor_running(_group_id: str, _actor_id: str) -> bool:
                    return bool(status_holder.get("running", True))

            stop_event = threading.Event()
            thread = start_actor_activity_thread(
                stop_event=stop_event,
                home=Path(home),
                pty_supervisor=object(),
                headless_supervisor=object(),
                codex_supervisor=_CodexSupervisor(),
                event_broadcaster=_Broadcaster(),
                load_group=load_group,
                interval_seconds=1.0,
            )
            try:
                # First tick runs immediately: new actor → state_changed → writes to ledger
                time.sleep(0.25)
                # Verify ledger has actor.activity
                import json
                lines = ledger_path.read_text(encoding="utf-8").strip().split("\n")
                activity_lines = [json.loads(line) for line in lines if '"actor.activity"' in line]
                self.assertTrue(activity_lines, "First tick should write actor.activity to ledger")
                self.assertEqual(activity_lines[-1]["data"]["actors"][0]["effective_working_state"], "working")

                initial_count = len(activity_lines)
                # Wait another tick (>1s interval) — no state change → no new ledger write
                time.sleep(1.3)
                lines2 = ledger_path.read_text(encoding="utf-8").strip().split("\n")
                activity_lines2 = [json.loads(line) for line in lines2 if '"actor.activity"' in line]
                self.assertEqual(len(activity_lines2), initial_count, "No state change should not add ledger entries")

                # Change state: working → idle → should write to ledger on next tick
                status_holder["status"] = "idle"
                time.sleep(1.3)
                lines3 = ledger_path.read_text(encoding="utf-8").strip().split("\n")
                activity_lines3 = [json.loads(line) for line in lines3 if '"actor.activity"' in line]
                self.assertGreater(len(activity_lines3), initial_count, "State change should add ledger entry")
                self.assertEqual(activity_lines3[-1]["data"]["actors"][0]["effective_working_state"], "idle")

                # Simulate actor stopping (actor_running returns False)
                idle_count = len(activity_lines3)
                status_holder["running"] = False
                time.sleep(1.3)
                lines4 = ledger_path.read_text(encoding="utf-8").strip().split("\n")
                activity_lines4 = [json.loads(line) for line in lines4 if '"actor.activity"' in line]
                self.assertGreater(len(activity_lines4), idle_count, "Actor stop should add ledger entry")
                last_event = activity_lines4[-1]
                stopped_actors = [a for a in last_event["data"]["actors"] if a["id"] == "peer1"]
                self.assertEqual(len(stopped_actors), 1, "Stopped actor should appear in event")
                self.assertEqual(stopped_actors[0]["effective_working_state"], "stopped")
                self.assertFalse(stopped_actors[0]["running"])
            finally:
                stop_event.set()
                thread.join(timeout=1.0)
        finally:
            cleanup()

    def test_actor_activity_thread_preserves_runner_on_stopped_entry(self) -> None:
        from cccc.daemon.serve_ops import start_actor_activity_thread
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            created = create_group(reg, title="pty-ledger-activity", topic="")
            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", title="Peer 1", runtime="codex", runner="pty")  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            ledger_path = group.ledger_path  # type: ignore[union-attr]
            status_holder = {"running": True}

            class _PtySupervisor:
                @staticmethod
                def actor_running(_group_id: str, _actor_id: str) -> bool:
                    return bool(status_holder.get("running", True))

                @staticmethod
                def idle_seconds(*, group_id: str, actor_id: str) -> float:
                    return 0.0

                @staticmethod
                def terminal_override(*, group_id: str, actor_id: str):
                    return None

            class _Broadcaster:
                def publish(self, event: dict) -> None:
                    pass

            stop_event = threading.Event()
            thread = start_actor_activity_thread(
                stop_event=stop_event,
                home=Path(home),
                pty_supervisor=_PtySupervisor(),
                headless_supervisor=object(),
                codex_supervisor=object(),
                event_broadcaster=_Broadcaster(),
                load_group=load_group,
                interval_seconds=1.0,
            )
            try:
                time.sleep(0.25)
                status_holder["running"] = False
                time.sleep(1.3)

                lines = ledger_path.read_text(encoding="utf-8").strip().split("\n")
                activity_lines = [json.loads(line) for line in lines if '"actor.activity"' in line]
                self.assertTrue(activity_lines, "Actor stop should write actor.activity to ledger")
                stopped_actors = [a for a in activity_lines[-1]["data"]["actors"] if a["id"] == "peer1"]
                self.assertEqual(len(stopped_actors), 1)
                self.assertEqual(stopped_actors[0]["effective_working_state"], "stopped")
                self.assertEqual(stopped_actors[0]["runner_effective"], "pty")
            finally:
                stop_event.set()
                thread.join(timeout=1.0)
        finally:
            cleanup()

    def test_codex_session_persists_headless_state_file(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runner_state_ops import headless_state_path

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )

            class _Proc:
                pid = os.getpid()

                @staticmethod
                def poll():
                    return None

            session._proc = _Proc()
            session._running = True
            session._session_state.status = "working"
            session._session_state.current_task_id = "turn-1"
            session._persist_state()

            state_path = headless_state_path("g_test", "peer1")
            self.assertTrue(state_path.exists())
            payload = json.loads(state_path.read_text(encoding="utf-8"))
            self.assertEqual(str(payload.get("runtime") or ""), "codex")
            self.assertEqual(int(payload.get("pid") or 0), os.getpid())
            self.assertEqual(str(payload.get("status") or ""), "working")
            self.assertEqual(str(payload.get("current_task_id") or ""), "turn-1")
        finally:
            cleanup()

    def test_codex_session_manager_falls_back_when_cli_is_missing(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSessionManager
        from cccc.daemon.runner_state_ops import headless_state_path

        home, cleanup = self._with_home()
        try:
            manager = CodexAppSessionManager()

            with patch(
                "cccc.daemon.codex_app_sessions.CodexAppSession.start",
                side_effect=FileNotFoundError(2, "No such file or directory", "codex"),
            ):
                session = manager.start_actor(
                    group_id="g_test",
                    actor_id="peer1",
                    cwd=Path(home),
                    env={},
                )

            self.assertTrue(session.is_running())
            self.assertTrue(manager.actor_running("g_test", "peer1"))

            state = manager.get_state(group_id="g_test", actor_id="peer1")
            self.assertIsInstance(state, dict)
            assert isinstance(state, dict)
            self.assertEqual(str(state.get("status") or ""), "idle")

            state_path = headless_state_path("g_test", "peer1")
            self.assertTrue(state_path.exists())
            payload = json.loads(state_path.read_text(encoding="utf-8"))
            self.assertEqual(str(payload.get("runtime") or ""), "codex")
            self.assertEqual(int(payload.get("pid") or 0), os.getpid())
            self.assertTrue(bool(payload.get("fallback")))

            manager.stop_actor(group_id="g_test", actor_id="peer1")
            self.assertFalse(state_path.exists())
        finally:
            cleanup()

    def test_codex_session_manager_falls_back_before_mcp_install_when_cli_is_absent(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSessionManager
        from cccc.daemon.runner_state_ops import headless_state_path

        home, cleanup = self._with_home()
        try:
            manager = CodexAppSessionManager()

            with patch("cccc.daemon.codex_app_sessions.shutil.which", return_value=None), patch(
                "cccc.daemon.codex_app_sessions.ensure_mcp_installed",
                side_effect=AssertionError("MCP install should not run without codex CLI"),
            ):
                session = manager.start_actor(
                    group_id="g_test",
                    actor_id="peer1",
                    cwd=Path(home),
                    env={},
                )

            self.assertTrue(session.is_running())
            state_path = headless_state_path("g_test", "peer1")
            self.assertTrue(state_path.exists())
            payload = json.loads(state_path.read_text(encoding="utf-8"))
            self.assertTrue(bool(payload.get("fallback")))
            self.assertIn("codex", str(payload.get("reason") or "").lower())
        finally:
            manager.stop_actor(group_id="g_test", actor_id="peer1")
            cleanup()

if __name__ == "__main__":
    unittest.main()
