import os
import tempfile
import unittest
from unittest.mock import patch


class TestSystemNotifyOps(unittest.TestCase):
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

    def test_system_notify_and_notify_ack(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "sys-notify", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

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

            notify, _ = self._call(
                "system_notify",
                {
                    "group_id": group_id,
                    "by": "system",
                    "kind": "unknown_kind",
                    "priority": "unknown_priority",
                    "title": "notify",
                    "message": "hello",
                    "target_actor_id": "peer1",
                    "requires_ack": True,
                },
            )
            self.assertTrue(notify.ok, getattr(notify, "error", None))
            notify_event = (notify.result or {}).get("event") if isinstance(notify.result, dict) else {}
            self.assertIsInstance(notify_event, dict)
            assert isinstance(notify_event, dict)
            self.assertEqual(str(notify_event.get("kind") or ""), "system.notify")
            notify_data = notify_event.get("data") if isinstance(notify_event.get("data"), dict) else {}
            self.assertIsInstance(notify_data, dict)
            assert isinstance(notify_data, dict)
            self.assertEqual(str(notify_data.get("kind") or ""), "info")
            self.assertEqual(str(notify_data.get("priority") or ""), "normal")

            notify_event_id = str(notify_event.get("id") or "").strip()
            self.assertTrue(notify_event_id)

            ack, _ = self._call(
                "notify_ack",
                {"group_id": group_id, "actor_id": "peer1", "notify_event_id": notify_event_id, "by": "peer1"},
            )
            self.assertTrue(ack.ok, getattr(ack, "error", None))
            ack_event = (ack.result or {}).get("event") if isinstance(ack.result, dict) else {}
            self.assertIsInstance(ack_event, dict)
            assert isinstance(ack_event, dict)
            self.assertEqual(str(ack_event.get("kind") or ""), "system.notify_ack")
        finally:
            cleanup()

    def test_normal_priority_notify_queues_and_flushes_for_running_pty_actor(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "sys-notify-delivery", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

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

            with patch("cccc.daemon.messaging.delivery.pty_runner.SUPERVISOR.actor_running", return_value=True), patch(
                "cccc.daemon.messaging.delivery.queue_system_notify"
            ) as queue_mock, patch(
                "cccc.daemon.messaging.delivery.flush_pending_messages", return_value=True
            ) as flush_mock:
                notify, _ = self._call(
                    "system_notify",
                    {
                        "group_id": group_id,
                        "by": "system",
                        "kind": "info",
                        "priority": "normal",
                        "title": "notify",
                        "message": "hello",
                        "target_actor_id": "peer1",
                        "requires_ack": False,
                    },
                )

            self.assertTrue(notify.ok, getattr(notify, "error", None))
            queue_mock.assert_called_once()
            queue_kwargs = queue_mock.call_args.kwargs
            self.assertEqual(queue_kwargs.get("actor_id"), "peer1")
            self.assertEqual(queue_kwargs.get("notify_kind"), "info")
            self.assertEqual(queue_kwargs.get("title"), "notify")
            self.assertEqual(queue_kwargs.get("message"), "hello")
            flush_mock.assert_called_once()
            self.assertEqual(flush_mock.call_args.kwargs.get("actor_id"), "peer1")
        finally:
            cleanup()

    def test_voice_secretary_input_notify_delivery_supports_legacy_pointer(self) -> None:
        from cccc.contracts.v1 import SystemNotifyData
        from cccc.daemon.messaging.delivery import render_system_notify_delivery_text

        text = render_system_notify_delivery_text(
            notify=SystemNotifyData(
                kind="info",
                priority="normal",
                title="Voice Secretary input available",
                message='New Secretary input is waiting. Call MCP tool cccc_voice_secretary_document(action="read_new_input").',
                target_actor_id="voice-secretary",
                requires_ack=False,
                context={
                    "kind": "voice_secretary_input",
                    "reason": "new_input",
                },
            )
        )

        self.assertIn("read_new_input", text)
        self.assertIn("Legacy pointer notification", text)
        self.assertIn("Pointer only", text)
        self.assertIn("reason=new_input", text)
        self.assertNotIn("Do not bootstrap", text)
        self.assertNotIn("cccc_voice_secretary_composer", text)
        self.assertNotIn("Do not finish this turn with only a plan", text)
        self.assertNotIn("alpha beta transcript", text)
        self.assertIn("cccc_voice_secretary_document", text)
        self.assertNotIn("source_chars", text)

    def test_voice_secretary_input_notify_delivery_uses_inline_envelope_when_available(self) -> None:
        from cccc.contracts.v1 import SystemNotifyData
        from cccc.daemon.messaging.delivery import render_system_notify_delivery_text

        text = render_system_notify_delivery_text(
            notify=SystemNotifyData(
                kind="info",
                priority="normal",
                title="Voice Secretary input available",
                message="Secretary input is ready in this notification's input_envelope.",
                target_actor_id="voice-secretary",
                requires_ack=False,
                context={
                    "kind": "voice_secretary_input",
                    "reason": "new_input",
                    "input_envelope": {
                        "schema": 1,
                        "delivery_id": "voice-input:g_test:1-1",
                        "delivery_mode": "daemon_input_envelope",
                        "seq_start": 1,
                        "seq_end": 1,
                        "input_text": "Target: composer\nRequest id: voice-prompt-inline\n\n把按钮文案改得更直接",
                    },
                },
            )
        )

        self.assertNotIn("daemon-delivered input_envelope", text)
        self.assertNotIn("Input envelope:", text)
        self.assertNotIn("delivery_id=", text)
        self.assertNotIn("seq_range=", text)
        self.assertIn("voice-prompt-inline", text)
        self.assertIn("把按钮文案改得更直接", text)
        self.assertNotIn("Pointer only", text)
        self.assertNotIn("First action", text)

    def test_voice_secretary_action_request_notify_delivery_is_actionable(self) -> None:
        from cccc.contracts.v1 import SystemNotifyData
        from cccc.daemon.messaging.delivery import render_system_notify_delivery_text

        text = render_system_notify_delivery_text(
            notify=SystemNotifyData(
                kind="info",
                priority="normal",
                title="Voice Secretary action request",
                message="Please review the weather plan.",
                target_actor_id="lead",
                requires_ack=True,
                context={
                    "kind": "voice_secretary_action_request",
                    "request_id": "voice-request-1",
                    "document_path": "docs/voice-secretary/weather-plan.md",
                    "summary": "Spoken task detected.",
                    "request_text": "Please review the weather plan and assign an owner.",
                },
            )
        )

        self.assertIn("voice_secretary_action_request", text)
        self.assertIn("voice-request-1", text)
        self.assertIn("docs/voice-secretary/weather-plan.md", text)
        self.assertNotIn("voice-doc-1", text)
        self.assertNotIn("voice-job-1", text)
        self.assertIn("Please review the weather plan", text)
        self.assertIn("Action: handle the request", text)

    def test_system_notify_accepts_auto_idle_kind(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "sys-notify-auto-idle", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "foreman1",
                    "title": "Foreman 1",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add.ok, getattr(add, "error", None))

            notify, _ = self._call(
                "system_notify",
                {
                    "group_id": group_id,
                    "by": "system",
                    "kind": "auto_idle",
                    "priority": "normal",
                    "title": "idle",
                    "message": "group auto idled",
                    "target_actor_id": "foreman1",
                    "requires_ack": False,
                },
            )
            self.assertTrue(notify.ok, getattr(notify, "error", None))
            notify_event = (notify.result or {}).get("event") if isinstance(notify.result, dict) else {}
            self.assertIsInstance(notify_event, dict)
            assert isinstance(notify_event, dict)
            notify_data = notify_event.get("data") if isinstance(notify_event.get("data"), dict) else {}
            self.assertIsInstance(notify_data, dict)
            assert isinstance(notify_data, dict)
            self.assertEqual(str(notify_data.get("kind") or ""), "auto_idle")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
