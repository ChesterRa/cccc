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

    def _wait_until(self, predicate, *, timeout: float = 1.0, interval: float = 0.01):
        deadline = time.monotonic() + timeout
        last = None
        while time.monotonic() < deadline:
            last = predicate()
            if last:
                return last
            time.sleep(interval)
        return predicate()

    def _activity_events_from_path(self, ledger_path: Path) -> list[dict]:
        if not ledger_path.exists():
            return []
        text = ledger_path.read_text(encoding="utf-8").strip()
        if not text:
            return []
        return [json.loads(line) for line in text.split("\n") if '"actor.activity"' in line]

    def _activity_events_with_actor_state(self, ledger_path: Path, actor_id: str, state: str) -> list[dict]:
        return [
            ev
            for ev in self._activity_events_from_path(ledger_path)
            if any(
                isinstance(actor, dict)
                and actor.get("id") == actor_id
                and actor.get("effective_working_state") == state
                for actor in ((ev.get("data") or {}).get("actors") or [])
            )
        ]

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

    def test_claude_headless_start_uses_persistent_print_session(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session

        home, cleanup = self._with_home()
        try:
            session = ClaudeAppSession(
                group_id="g_claude_resume",
                actor_id="peer1",
                cwd=Path(home),
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

            launch_commands: list[list[str]] = []

            def fake_popen(cmd, *args, **kwargs):
                launch_commands.append(list(cmd))
                return FakeProc()

            with (
                patch("cccc.daemon.claude_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.claude_app_sessions.subprocess.Popen", side_effect=fake_popen),
                patch("cccc.daemon.claude_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.claude_app_sessions.time.sleep", return_value=None),
                patch("cccc.daemon.runtime_session_ops.uuid.uuid4", return_value="42e9ef0c-3b75-43a0-9056-eef13dd1061d"),
                patch.object(session, "_persist_state"),
                patch.object(session, "_queue_bootstrap_control_turn", return_value=None),
            ):
                session.start()

            self.assertEqual(len(launch_commands), 1)
            self.assertIn("--session-id", launch_commands[0])
            self.assertIn("42e9ef0c-3b75-43a0-9056-eef13dd1061d", launch_commands[0])
            self.assertNotIn("--no-session-persistence", launch_commands[0])
            stored = read_runtime_session("g_claude_resume", "peer1")
            self.assertEqual(stored.get("runner"), "headless")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
        finally:
            cleanup()

    def test_claude_headless_start_resumes_existing_session_id(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession
        from cccc.daemon.runtime_session_ops import record_headless_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            base_command = [
                "claude",
                "-p",
                "--input-format", "stream-json",
                "--output-format", "stream-json",
                "--include-partial-messages",
                "--include-hook-events",
                "--verbose",
                "--dangerously-skip-permissions",
            ]
            record_headless_runtime_session(
                group_id="g_claude_resume",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=base_command,
                provider_session_id="434d41d0-02e9-4760-9f13-155d04cde834",
                status="usable",
                captured_from="stream_json_init",
                resume_eligible=True,
            )
            session = ClaudeAppSession(
                group_id="g_claude_resume",
                actor_id="peer1",
                cwd=cwd,
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

            launch_commands: list[list[str]] = []

            def fake_popen(cmd, *args, **kwargs):
                launch_commands.append(list(cmd))
                return FakeProc()

            with (
                patch("cccc.daemon.claude_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.claude_app_sessions.subprocess.Popen", side_effect=fake_popen),
                patch("cccc.daemon.claude_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.claude_app_sessions.time.sleep", return_value=None),
                patch.object(session, "_persist_state"),
                patch.object(session, "_queue_bootstrap_control_turn", return_value=None),
            ):
                session.start()

            self.assertEqual(len(launch_commands), 1)
            self.assertEqual(launch_commands[0][:3], ["claude", "--resume", "434d41d0-02e9-4760-9f13-155d04cde834"])
            self.assertIn("-p", launch_commands[0])
        finally:
            cleanup()

    def test_claude_headless_shutdown_stdout_close_does_not_persist_actor_stop(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession, ClaudeAppSessionManager

        home, cleanup = self._with_home()
        try:
            manager = ClaudeAppSessionManager()
            session = ClaudeAppSession(
                group_id="g_claude_shutdown",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )

            class FakeProc:
                pid = os.getpid()
                stdin = io.StringIO()
                stdout = io.StringIO("")
                stderr = io.StringIO()

                def poll(self):
                    return None

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 0

            session._proc = FakeProc()
            session._running = True
            manager._sessions[("g_claude_shutdown", "peer1")] = session

            with patch("cccc.daemon.claude_app_sessions.persist_actor_process_exit_stopped") as persist_stopped:
                manager.begin_shutdown()
                session._stdout_loop()

            persist_stopped.assert_not_called()
            self.assertFalse(session.is_running())
        finally:
            cleanup()

    def test_claude_headless_print_mode_stdout_close_does_not_persist_actor_stop(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession

        home, cleanup = self._with_home()
        try:
            session = ClaudeAppSession(
                group_id="g_claude_print_mode",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )

            class FakeProc:
                pid = os.getpid()
                stdin = io.StringIO()
                stdout = io.StringIO("")
                stderr = io.StringIO()

                def poll(self):
                    return 0

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 0

            session._proc = FakeProc()
            session._running = True

            with patch("cccc.daemon.claude_app_sessions.persist_actor_process_exit_stopped") as persist_stopped:
                session._stdout_loop()

            persist_stopped.assert_not_called()
            self.assertFalse(session.is_running())
        finally:
            cleanup()

    def test_claude_headless_resume_exit_marks_failed_without_fresh_fallback(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_headless_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            base_command = [
                "claude",
                "-p",
                "--input-format", "stream-json",
                "--output-format", "stream-json",
                "--include-partial-messages",
                "--include-hook-events",
                "--verbose",
                "--dangerously-skip-permissions",
            ]
            record_headless_runtime_session(
                group_id="g_claude_resume",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=base_command,
                provider_session_id="stale-session",
                status="usable",
                captured_from="stream_json_init",
                resume_eligible=True,
            )
            session = ClaudeAppSession(
                group_id="g_claude_resume",
                actor_id="peer1",
                cwd=cwd,
                env={},
            )

            class ExitedProc:
                pid = 111
                stdin = io.StringIO()
                stdout = io.StringIO()
                stderr = io.StringIO()

                def poll(self):
                    return 1

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 1

            class FakeThread:
                def __init__(self, *args, **kwargs):
                    self.args = args
                    self.kwargs = kwargs

                def start(self):
                    return None

            launch_commands: list[list[str]] = []

            def fake_popen(cmd, *args, **kwargs):
                launch_commands.append(list(cmd))
                return ExitedProc()

            with (
                patch("cccc.daemon.claude_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.claude_app_sessions.subprocess.Popen", side_effect=fake_popen),
                patch("cccc.daemon.claude_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.claude_app_sessions.time.sleep", return_value=None),
                patch.object(session, "_persist_state"),
                patch.object(session, "_queue_bootstrap_control_turn", return_value=None),
            ):
                with self.assertRaises(RuntimeError):
                    session.start()

            self.assertEqual(launch_commands[0][:3], ["claude", "--resume", "stale-session"])
            self.assertEqual(len(launch_commands), 1)
            self.assertEqual(session._turn_queue.qsize(), 0)
            stored = read_runtime_session("g_claude_resume", "peer1")
            self.assertEqual(stored.get("provider_session_id"), "stale-session")
            self.assertEqual(stored.get("status"), "resume_failed")
            self.assertFalse(bool(stored.get("resume_eligible")))
        finally:
            cleanup()

    def test_claude_headless_resume_rejected_on_first_turn_marks_metadata_failed(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_headless_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            base_command = [
                "claude",
                "-p",
                "--input-format", "stream-json",
                "--output-format", "stream-json",
                "--include-partial-messages",
                "--include-hook-events",
                "--verbose",
                "--dangerously-skip-permissions",
            ]
            record_headless_runtime_session(
                group_id="g_claude_resume",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=base_command,
                provider_session_id="stale-session",
                status="usable",
                captured_from="stream_json_init",
                resume_eligible=True,
            )
            session = ClaudeAppSession(
                group_id="g_claude_resume",
                actor_id="peer1",
                cwd=cwd,
                env={},
            )
            session._resumed_provider_session_id = "stale-session"
            session._active_turn_id = "turn-bootstrap"
            session._active_event_id = "bootstrap-event"
            session._active_control_kind = "bootstrap"

            emitted: list[str] = []
            with (
                patch.object(session, "_persist_state", return_value=None),
                patch.object(session, "_emit", side_effect=lambda event_type, payload: emitted.append(event_type)),
                patch("cccc.daemon.claude_app_sessions.persist_actor_process_exit_stopped"),
            ):
                session._handle_result_event(
                    {
                        "type": "result",
                        "subtype": "error",
                        "error": "No conversation found for stale-session",
                    }
                )

            stored = read_runtime_session("g_claude_resume", "peer1")
            self.assertEqual(stored.get("status"), "resume_failed")
            self.assertFalse(bool(stored.get("resume_eligible")))
            self.assertIn("No conversation found", str(stored.get("last_resume_error") or ""))
            self.assertIn("headless.session.resume_failed", emitted)
            self.assertIn("headless.control.failed", emitted)
            self.assertIn("headless.session.stopped", emitted)
            self.assertFalse(session.is_running())
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
                patch("cccc.daemon.messaging.chat_delivery_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages") as request_flush_pending_messages,
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

    def test_install_slash_command_delivers_general_install_task_to_agent(self) -> None:
        from cccc.daemon.messaging.chat_ops import handle_send
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-install-slash", "topic": "", "by": "user"})
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
                patch("cccc.daemon.messaging.chat_ops.codex_app_supervisor.submit_user_message", return_value=True) as submit_user_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
            ):
                resp = handle_send(
                    {
                        "group_id": group_id,
                        "by": "user",
                        "text": "/install https://github.com/obra/superpowers",
                        "to": ["peer1"],
                        "client_id": "install-1",
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
            self.assertIn("[cccc] Slash command: /install", submitted_text)
            self.assertIn("[cccc] Capability: skill:cccc:install", submitted_text)
            self.assertIn("scope=group", submitted_text)
            self.assertIn("Route this request through skill:cccc:install", submitted_text)
            self.assertIn("The skill definition is the source of truth", submitted_text)
            self.assertNotIn("Expected procedure:", submitted_text)
            self.assertNotIn("use the skill-installer workflow", submitted_text)
            self.assertIn("https://github.com/obra/superpowers", submitted_text)
            self.assertIn("Parser target hint: github", submitted_text)
            self.assertIn("Treat the parser target hint as non-authoritative", submitted_text)
            queue_chat_message.assert_not_called()
            request_flush_pending_messages.assert_not_called()

            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            event = (resp.result or {}).get("event") if isinstance(resp.result, dict) else {}
            data = event.get("data") if isinstance(event, dict) and isinstance(event.get("data"), dict) else {}
            self.assertEqual(str(data.get("text") or ""), "/install https://github.com/obra/superpowers")
            refs = data.get("refs") if isinstance(data.get("refs"), list) else []
            slash_refs = [item for item in refs if isinstance(item, dict) and item.get("command") == "/install"]
            self.assertEqual(len(slash_refs), 1)
            self.assertEqual(str(slash_refs[0].get("capability_id") or ""), "skill:cccc:install")
            self.assertEqual(str(slash_refs[0].get("target_kind") or ""), "github")
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
                patch("cccc.daemon.messaging.chat_delivery_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages") as request_flush_pending_messages,
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
                patch("cccc.daemon.messaging.chat_delivery_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages") as request_flush_pending_messages,
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
                patch("cccc.daemon.messaging.chat_delivery_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
                patch("cccc.daemon.messaging.chat_delivery_ops.get_headless_targets_for_message", return_value=[]),
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
                patch("cccc.daemon.messaging.chat_delivery_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
                patch("cccc.daemon.messaging.chat_delivery_ops.get_headless_targets_for_message", return_value=[]),
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

    def test_auto_wake_treats_codex_pty_app_server_state_as_running(self) -> None:
        from cccc.daemon import server as daemon_server
        from cccc.kernel.actors import update_actor
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "codex-auto-wake-running", "topic": "", "by": "user"})
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
            group = load_group(group_id)
            self.assertIsNotNone(group)
            update_actor(group, "peer1", {"runtime_state_source": "app_server"})  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            with patch("cccc.daemon.server.codex_app_supervisor.actor_running", return_value=True), patch(
                "cccc.daemon.server.pty_runner.SUPERVISOR.actor_running",
                return_value=False,
            ):
                self.assertTrue(daemon_server._auto_wake_actor_running(group, "peer1"))  # type: ignore[arg-type]
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
                patch("cccc.daemon.messaging.chat_delivery_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages") as request_flush_pending_messages,
                patch("cccc.daemon.messaging.chat_ops.flush_pending_messages"),
                patch("cccc.daemon.messaging.chat_delivery_ops.get_headless_targets_for_message", return_value=[]),
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
                patch("cccc.daemon.messaging.chat_delivery_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages") as request_flush_pending_messages,
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
                patch("cccc.daemon.messaging.chat_delivery_ops.queue_chat_message") as queue_chat_message,
                patch("cccc.daemon.messaging.chat_delivery_ops.request_flush_pending_messages") as request_flush_pending_messages,
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

    def test_codex_bootstrap_control_text_passes_explicit_actor_id(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            created = create_group(reg, title="codex-bootstrap", topic="")
            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            assert group is not None
            add_actor(group, actor_id="Development", title="Development", runtime="codex", runner="headless")
            group.save()

            session = CodexAppSession(
                group_id=created.group_id,
                actor_id="Development",
                cwd=Path(home),
                env={},
            )

            text = session._build_bootstrap_control_text()

            self.assertIn("cccc_bootstrap", text)
            self.assertIn('actor_id="Development"', text)
            self.assertIn(f'group_id="{created.group_id}"', text)
            self.assertIn("Pass actor_id explicitly", text)
        finally:
            cleanup()

    def test_codex_bootstrap_control_failed_tool_call_finishes_turn(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _PendingTurn

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="Development",
                cwd=Path(home),
                env={},
            )
            session._active_turn_id = "turn-bootstrap"
            session._active_control_kind = "bootstrap"
            session._active_payload = _PendingTurn(text="bootstrap", event_id="", control_kind="bootstrap")
            session._session_state.status = "working"
            session._session_state.current_task_id = "turn-bootstrap"

            with (
                patch.object(session, "_persist_state"),
                patch.object(session, "_emit") as emit,
                patch.object(session._turn_done, "set") as done_set,
            ):
                session._handle_notification(
                    "item/completed",
                    {
                        "turnId": "turn-bootstrap",
                        "item": {
                            "type": "mcpToolCall",
                            "id": "call-bootstrap",
                            "server": "cccc",
                            "tool": "cccc_bootstrap",
                            "status": "failed",
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": "missing_actor_id (set CCCC_ACTOR_ID env or pass actor_id)",
                                    }
                                ]
                            },
                        },
                    },
                )

            done_set.assert_called_once()
            self.assertEqual(session._session_state.status, "idle")
            self.assertIsNone(session._session_state.current_task_id)
            event_types = [str(call.args[0]) for call in emit.call_args_list if call.args]
            self.assertIn("headless.item.completed", event_types)
            self.assertIn("headless.control.failed", event_types)
            failed_payload = next(call.args[1] for call in emit.call_args_list if call.args and call.args[0] == "headless.control.failed")
            self.assertEqual(failed_payload.get("status"), "failed")
            self.assertEqual(str((failed_payload.get("error") or {}).get("message") or ""), "missing_actor_id (set CCCC_ACTOR_ID env or pass actor_id)")
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

    def test_voice_secretary_control_completion_keeps_unknown_composer_draft_incomplete(self) -> None:
        from cccc.daemon.codex_app_sessions import _voice_secretary_control_completion_state

        home, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "voice-secretary-missing-draft", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            completed, diagnostics = _voice_secretary_control_completion_state(
                group_id=group_id,
                snapshot={
                    "kind": "voice_secretary_input",
                    "before_latest_seq": 1,
                    "before_secretary_read_cursor": 1,
                    "prefetched_read_new_input": True,
                    "composer_request_ids": ["voice-prompt-missing"],
                    "input_target_kinds": ["composer"],
                    "before_prompt_drafts": {},
                },
            )

            self.assertFalse(completed)
            self.assertEqual(diagnostics.get("missing"), ["composer_draft:voice-prompt-missing"])
        finally:
            cleanup()

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
                interval_seconds=0.01,
            )
            try:
                # First tick runs immediately: new actor → state_changed → writes to ledger
                activity_lines = self._wait_until(
                    lambda: self._activity_events_with_actor_state(ledger_path, "peer1", "working"),
                    timeout=0.5,
                )
                self.assertTrue(activity_lines, "First tick should write actor.activity to ledger")
                self.assertEqual(activity_lines[-1]["data"]["actors"][0]["effective_working_state"], "working")

                initial_count = len(self._activity_events_from_path(ledger_path))
                # Wait another tick — no state change → no new ledger write
                time.sleep(0.05)
                activity_lines2 = self._activity_events_from_path(ledger_path)
                self.assertEqual(len(activity_lines2), initial_count, "No state change should not add ledger entries")

                # Change state: working → idle → should write to ledger on next tick
                status_holder["status"] = "idle"
                activity_lines3 = self._wait_until(
                    lambda: self._activity_events_with_actor_state(ledger_path, "peer1", "idle"),
                    timeout=0.5,
                )
                self.assertGreater(len(self._activity_events_from_path(ledger_path)), initial_count, "State change should add ledger entry")
                self.assertEqual(activity_lines3[-1]["data"]["actors"][0]["effective_working_state"], "idle")

                # Simulate actor stopping (actor_running returns False)
                idle_count = len(self._activity_events_from_path(ledger_path))
                status_holder["running"] = False
                activity_lines4 = self._wait_until(
                    lambda: self._activity_events_with_actor_state(ledger_path, "peer1", "stopped"),
                    timeout=0.5,
                )
                self.assertGreater(len(self._activity_events_from_path(ledger_path)), idle_count, "Actor stop should add ledger entry")
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
            add_actor(
                group,
                actor_id="peer1",
                title="Peer 1",
                runtime="codex",
                runner="pty",
                runtime_state_source="terminal",
            )  # type: ignore[arg-type]
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
                interval_seconds=0.01,
            )
            try:
                self._wait_until(lambda: self._activity_events_from_path(ledger_path))
                status_holder["running"] = False

                activity_lines = self._wait_until(
                    lambda: [
                        ev
                        for ev in self._activity_events_from_path(ledger_path)
                        if any(
                            isinstance(actor, dict)
                            and actor.get("id") == "peer1"
                            and actor.get("effective_working_state") == "stopped"
                            for actor in ((ev.get("data") or {}).get("actors") or [])
                        )
                    ]
                )
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

    def test_actor_activity_thread_uses_codex_pty_prompt_override(self) -> None:
        from cccc.daemon.serve_ops import start_actor_activity_thread
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            created = create_group(reg, title="codex-pty-prompt-activity", topic="")
            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            add_actor(
                group,
                actor_id="peer1",
                title="Peer 1",
                runtime="codex",
                runner="pty",
                runtime_state_source="terminal",
            )  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            published: list[dict] = []

            class _PtySupervisor:
                @staticmethod
                def actor_running(_group_id: str, _actor_id: str) -> bool:
                    return True

                @staticmethod
                def idle_seconds(*, group_id: str, actor_id: str) -> float:
                    return 0.2

                @staticmethod
                def terminal_override(*, group_id: str, actor_id: str):
                    return None

                @staticmethod
                def tail_output(*, group_id: str, actor_id: str, max_bytes: int) -> bytes:
                    return (
                        "◦ Working (4s • esc to interrupt)\n"
                        "> Use /skills to list available skills\n"
                        "gpt-5.5 medium · ~/Desktop/waterbang/ai/cccc · 30M used\n"
                    ).encode("utf-8")

            class _Broadcaster:
                def publish(self, event: dict) -> None:
                    published.append(event)

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
                actor_events = [event for event in published if str(event.get("kind") or "") == "actor.activity"]
                self.assertTrue(actor_events)
                actors = ((actor_events[-1].get("data") or {}).get("actors") or [])
                self.assertEqual(len(actors), 1)
                self.assertEqual(str(actors[0].get("effective_working_state") or ""), "idle")
                self.assertEqual(str(actors[0].get("effective_working_reason") or ""), "pty_terminal_codex_prompt_visible")
            finally:
                stop_event.set()
                thread.join(timeout=1.0)
        finally:
            cleanup()

    def test_actor_activity_thread_uses_codex_app_server_state_for_pty_app_server_actor(self) -> None:
        from cccc.daemon.serve_ops import start_actor_activity_thread
        from cccc.kernel.actors import add_actor, update_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            created = create_group(reg, title="pty-app-server-activity", topic="")
            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", title="Peer 1", runtime="codex", runner="pty")  # type: ignore[arg-type]
            update_actor(group, "peer1", {"runtime_state_source": "app_server"})  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            published: list[dict] = []

            class _PtySupervisor:
                terminal_override_calls = 0

                @staticmethod
                def actor_running(_group_id: str, _actor_id: str) -> bool:
                    return True

                @staticmethod
                def idle_seconds(*, group_id: str, actor_id: str) -> float:
                    return 600.0

                @classmethod
                def terminal_override(cls, *, group_id: str, actor_id: str):
                    cls.terminal_override_calls += 1
                    return {
                        "effective_working_state": "idle",
                        "effective_working_reason": "unused_terminal_override",
                    }

            class _CodexSupervisor:
                @staticmethod
                def actor_running(group_id: str, actor_id: str) -> bool:
                    return True

                @staticmethod
                def get_state(group_id: str, actor_id: str) -> dict:
                    return {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "status": "working",
                        "current_task_id": "turn-1",
                        "updated_at": "2026-05-12T00:00:00Z",
                    }

            class _Broadcaster:
                def publish(self, event: dict) -> None:
                    published.append(event)

            stop_event = threading.Event()
            thread = start_actor_activity_thread(
                stop_event=stop_event,
                home=Path(home),
                pty_supervisor=_PtySupervisor(),
                headless_supervisor=object(),
                codex_supervisor=_CodexSupervisor(),
                event_broadcaster=_Broadcaster(),
                load_group=load_group,
                interval_seconds=1.0,
            )
            try:
                time.sleep(0.25)
                actor_events = [event for event in published if str(event.get("kind") or "") == "actor.activity"]
                self.assertTrue(actor_events)
                actors = ((actor_events[-1].get("data") or {}).get("actors") or [])
                self.assertEqual(len(actors), 1)
                self.assertEqual(str(actors[0].get("runner_effective") or ""), "pty")
                self.assertEqual(str(actors[0].get("effective_working_state") or ""), "working")
                self.assertEqual(str(actors[0].get("effective_working_reason") or ""), "headless_working")
                self.assertEqual(_PtySupervisor.terminal_override_calls, 0)
            finally:
                stop_event.set()
                thread.join(timeout=1.0)
        finally:
            cleanup()

    def test_actor_activity_thread_does_not_infer_working_from_tail_for_idle_pty_app_server_actor(self) -> None:
        from cccc.daemon.serve_ops import start_actor_activity_thread
        from cccc.kernel.actors import add_actor, update_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            created = create_group(reg, title="pty-app-server-no-tail-inference", topic="")
            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", title="Peer 1", runtime="codex", runner="pty")  # type: ignore[arg-type]
            update_actor(group, "peer1", {"runtime_state_source": "app_server"})  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            published: list[dict] = []

            class _PtySupervisor:
                tail_calls = 0
                terminal_override_calls = 0

                @staticmethod
                def actor_running(_group_id: str, _actor_id: str) -> bool:
                    return True

                @staticmethod
                def idle_seconds(*, group_id: str, actor_id: str) -> float:
                    return 0.2

                @classmethod
                def terminal_override(cls, *, group_id: str, actor_id: str):
                    cls.terminal_override_calls += 1
                    return None

                @classmethod
                def tail_output(cls, *, group_id: str, actor_id: str, max_bytes: int) -> bytes:
                    cls.tail_calls += 1
                    return b"tool call output\n"

            class _CodexSupervisor:
                @staticmethod
                def actor_running(group_id: str, actor_id: str) -> bool:
                    return True

                @staticmethod
                def get_state(group_id: str, actor_id: str) -> dict:
                    return {
                        "group_id": group_id,
                        "actor_id": actor_id,
                        "status": "idle",
                        "current_task_id": "turn-1",
                        "updated_at": "2026-05-12T00:00:00Z",
                    }

            class _Broadcaster:
                def publish(self, event: dict) -> None:
                    published.append(event)

            stop_event = threading.Event()
            thread = start_actor_activity_thread(
                stop_event=stop_event,
                home=Path(home),
                pty_supervisor=_PtySupervisor(),
                headless_supervisor=object(),
                codex_supervisor=_CodexSupervisor(),
                event_broadcaster=_Broadcaster(),
                load_group=load_group,
                interval_seconds=1.0,
            )
            try:
                time.sleep(0.25)
                actor_events = [event for event in published if str(event.get("kind") or "") == "actor.activity"]
                self.assertTrue(actor_events)
                actors = ((actor_events[-1].get("data") or {}).get("actors") or [])
                self.assertEqual(len(actors), 1)
                self.assertEqual(str(actors[0].get("runner_effective") or ""), "pty")
                self.assertEqual(str(actors[0].get("effective_working_state") or ""), "idle")
                self.assertEqual(str(actors[0].get("effective_working_reason") or ""), "headless_idle")
                self.assertEqual(_PtySupervisor.tail_calls, 0)
                self.assertEqual(_PtySupervisor.terminal_override_calls, 0)
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

    def test_codex_thread_status_notification_does_not_mark_active_as_working(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(group_id="g_test", actor_id="peer1", cwd=Path(home), env={})
            session._running = True
            session._session_state.thread_id = "thread-1"
            session._session_state.status = "idle"

            session._handle_notification(
                "thread/status/changed",
                {"threadId": "thread-1", "status": {"type": "active", "activeFlags": []}},
            )

            self.assertEqual(str(session._session_state.status or ""), "idle")
            self.assertIsNone(session._session_state.current_task_id)

            session._handle_notification(
                "thread/status/changed",
                {"threadId": "thread-1", "status": {"type": "idle"}},
            )

            self.assertEqual(str(session._session_state.status or ""), "idle")
            self.assertIsNone(session._session_state.current_task_id)
        finally:
            cleanup()

    def test_codex_thread_status_notification_marks_remote_tui_waiting(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(group_id="g_test", actor_id="peer1", cwd=Path(home), env={})
            session._running = True
            session._session_state.thread_id = "thread-1"
            session._session_state.status = "idle"

            session._handle_notification(
                "thread/status/changed",
                {
                    "threadId": "thread-1",
                    "status": {"type": "active", "activeFlags": ["waitingOnUserInput"]},
                },
            )

            self.assertEqual(str(session._session_state.status or ""), "waiting")
            self.assertEqual(str(session._session_state.current_task_id or ""), "thread-1")
        finally:
            cleanup()

    def test_codex_thread_status_notification_ignores_other_threads(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(group_id="g_test", actor_id="peer1", cwd=Path(home), env={})
            session._running = True
            session._session_state.thread_id = "thread-1"
            session._session_state.status = "idle"

            session._handle_notification(
                "thread/status/changed",
                {"threadId": "thread-2", "status": {"type": "active", "activeFlags": []}},
            )

            self.assertEqual(str(session._session_state.status or ""), "idle")
            self.assertIsNone(session._session_state.current_task_id)
        finally:
            cleanup()

    def test_codex_headless_start_resumes_recorded_app_server_thread(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_headless_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            record_headless_runtime_session(
                group_id="g_codex_resume",
                actor_id="peer1",
                runtime="codex",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "stdio://"],
                provider_thread_id="thr_existing",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_codex_resume",
                actor_id="peer1",
                cwd=cwd,
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

            requests: list[tuple[str, dict]] = []

            def fake_request(method, params, *, timeout):
                requests.append((method, dict(params or {})))
                if method == "initialize":
                    return {}
                if method == "thread/resume":
                    return {"thread": {"id": "thr_existing"}}
                raise AssertionError(f"unexpected request: {method}")

            with (
                patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=FakeThread),
                patch.object(session, "_request", side_effect=fake_request),
                patch.object(session, "_persist_state"),
                patch.object(session, "_queue_bootstrap_control_turn", return_value=None),
            ):
                session.start()

            self.assertEqual(requests[1][0], "thread/resume")
            self.assertEqual(requests[1][1].get("threadId"), "thr_existing")
            self.assertEqual(requests[1][1].get("approvalPolicy"), "never")
            self.assertEqual(requests[1][1].get("sandbox"), "danger-full-access")
            stored = read_runtime_session("g_codex_resume", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr_existing")
            self.assertEqual(stored.get("captured_from"), "app_server_thread_resume")
            self.assertTrue(bool(stored.get("resume_eligible")))
        finally:
            cleanup()

    def test_codex_headless_start_resumes_when_listen_url_changes(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_headless_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            record_headless_runtime_session(
                group_id="g_codex_resume",
                actor_id="peer1",
                runtime="codex",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "ws://127.0.0.1:49152"],
                provider_thread_id="thr_existing",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_codex_resume",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:49153",
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

            requests: list[tuple[str, dict]] = []

            def fake_request(method, params, *, timeout):
                requests.append((method, dict(params or {})))
                if method == "initialize":
                    return {}
                if method == "thread/resume":
                    return {"thread": {"id": "thr_existing"}}
                raise AssertionError(f"unexpected request: {method}")

            with (
                patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=FakeThread),
                patch.object(session, "_request", side_effect=fake_request),
                patch.object(session, "_persist_state"),
                patch.object(session, "_queue_bootstrap_control_turn", return_value=None),
            ):
                session.start()

            self.assertEqual(requests[1][0], "thread/resume")
            self.assertEqual(requests[1][1].get("threadId"), "thr_existing")
            self.assertEqual(requests[1][1].get("approvalPolicy"), "never")
            self.assertEqual(requests[1][1].get("sandbox"), "danger-full-access")
            stored = read_runtime_session("g_codex_resume", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr_existing")
            self.assertEqual(stored.get("captured_from"), "app_server_thread_resume")
            self.assertTrue(bool(stored.get("resume_eligible")))
        finally:
            cleanup()

    def test_codex_headless_missing_resume_starts_fresh_thread(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_headless_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            record_headless_runtime_session(
                group_id="g_codex_resume",
                actor_id="peer1",
                runtime="codex",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "stdio://"],
                provider_thread_id="thr_stale",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_codex_resume",
                actor_id="peer1",
                cwd=cwd,
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

            requests: list[str] = []

            def fake_request(method, params, *, timeout):
                requests.append(method)
                if method == "initialize":
                    return {}
                if method == "thread/resume":
                    raise RuntimeError("thread not found")
                if method == "thread/start":
                    return {"thread": {"id": "thr_fresh"}}
                raise AssertionError(f"unexpected request: {method}")

            with (
                patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=FakeThread),
                patch.object(session, "_request", side_effect=fake_request),
                patch.object(session, "_persist_state"),
                patch.object(session, "_queue_bootstrap_control_turn", return_value=None),
            ):
                session.start()

            self.assertEqual(requests, ["initialize", "thread/resume", "thread/start"])
            stored = read_runtime_session("g_codex_resume", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr_fresh")
            self.assertEqual(stored.get("captured_from"), "app_server_thread_start")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
        finally:
            cleanup()

    def test_codex_headless_resume_transport_error_starts_fresh_thread(self) -> None:
        from cccc.daemon.codex_app_thread_ops import start_codex_app_thread
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_headless_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            command = ["codex", "app-server", "--listen", "stdio://"]
            record_headless_runtime_session(
                group_id="g_codex_resume",
                actor_id="peer1",
                runtime="codex",
                cwd=cwd,
                command=command,
                provider_thread_id="thr_stale",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            requests: list[str] = []

            def fake_request(method, params, *, timeout):
                requests.append(method)
                if method == "thread/resume":
                    raise RuntimeError("websocket timed out")
                if method == "thread/start":
                    return {"thread": {"id": "thr_fresh"}}
                raise AssertionError(f"unexpected request: {method}")

            result = start_codex_app_thread(
                request=fake_request,
                group_id="g_codex_resume",
                actor_id="peer1",
                cwd=cwd,
                command=command,
            )

            self.assertEqual(result.thread_id, "thr_fresh")
            self.assertFalse(result.resumed)
            self.assertEqual(requests, ["thread/resume", "thread/start"])
            stored = read_runtime_session("g_codex_resume", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr_fresh")
            self.assertEqual(stored.get("captured_from"), "app_server_thread_start")
            self.assertEqual(stored.get("status"), "usable")
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

    def test_codex_pty_app_session_starts_ws_server_and_remote_tui(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSessionManager
        from cccc.daemon.runtime_session_ops import read_runtime_session

        home, cleanup = self._with_home()
        try:
            manager = CodexAppSessionManager()
            requests: list[tuple[str, dict]] = []
            started_pty: list[dict] = []

            class FakeProc:
                pid = 12345
                stdin = io.StringIO()
                stdout = io.StringIO()
                stderr = io.StringIO()

                def poll(self):
                    return None

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 0

            class FakeThread:
                def __init__(self, *args, **kwargs):
                    self.args = args
                    self.kwargs = kwargs

                def start(self):
                    return None

            class FakeWs:
                def send(self, data):
                    return None

                def recv(self):
                    time.sleep(1)
                    return ""

                def close(self):
                    return None

            def fake_request(self, method: str, params: dict, *, timeout: float):
                requests.append((method, params))
                if method == "initialize":
                    return {}
                raise AssertionError(f"unexpected request: {method}")

            def fake_start_actor(**kwargs):
                started_pty.append(dict(kwargs))

                class _PtySession:
                    pid = 22222

                    def is_running(self):
                        return True

                return _PtySession()

            def fake_resolve_subprocess_argv(argv):
                parts = list(argv or [])
                if parts and parts[0] == "codex":
                    return [r"C:\Tools\codex.cmd", *parts[1:]]
                return parts

            with patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()) as popen, patch(
                "cccc.daemon.codex_app_sessions.threading.Thread", FakeThread
            ), patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True), patch(
                "cccc.daemon.codex_app_sessions._connect_websocket", return_value=FakeWs()
            ) as connect_ws, patch(
                "cccc.daemon.codex_app_sessions._codex_cli_available", return_value=True
            ), patch(
                "cccc.daemon.codex_app_sessions.resolve_subprocess_argv",
                side_effect=fake_resolve_subprocess_argv,
            ), patch(
                "cccc.daemon.codex_app_sessions.CodexAppSession._request", fake_request
            ), patch(
                "cccc.daemon.codex_app_sessions.CodexAppSession._build_bootstrap_control_text", return_value="bootstrap"
            ), patch(
                "cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.start_actor", side_effect=fake_start_actor
            ), patch(
                "cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.actor_running", return_value=True
            ):
                session = manager.start_pty_app_actor(
                    group_id="g_test",
                    actor_id="peer1",
                    cwd=Path(home),
                    env={},
                    model="gpt-5.5",
                    remote_tui_base_command=[
                        "codex",
                        "-c",
                        "shell_environment_policy.inherit=all",
                        "--dangerously-bypass-approvals-and-sandbox",
                        "--search",
                    ],
                    max_backlog_bytes=1234,
                )

            self.assertTrue(session.is_running())
            with patch("cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.actor_running", return_value=True):
                self.assertTrue(manager.actor_running("g_test", "peer1"))
            self.assertTrue(session._turn_queue.empty())
            command = list(popen.call_args.args[0])
            self.assertEqual(command[:3], [r"C:\Tools\codex.cmd", "app-server", "--listen"])
            self.assertTrue(str(command[3]).startswith("ws://127.0.0.1:"))
            connect_ws.assert_called_once()
            self.assertEqual([item[0] for item in requests], ["initialize"])
            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored, {})
            self.assertEqual(len(started_pty), 1)
            self.assertEqual(
                started_pty[0]["command"],
                [
                    r"C:\Tools\codex.cmd",
                    "-c",
                    "shell_environment_policy.inherit=all",
                    "--dangerously-bypass-approvals-and-sandbox",
                    "--search",
                    "--remote",
                    command[3],
                ],
            )
            self.assertEqual(started_pty[0]["runtime"], "codex")
        finally:
            manager.stop_actor(group_id="g_test", actor_id="peer1")
            cleanup()

    def test_codex_app_start_waits_beyond_initial_refused_connections(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        session = None
        try:
            cwd = Path(home)
            requests: list[str] = []

            class FakeProc:
                pid = 12345
                stdin = io.StringIO()
                stdout = io.StringIO()
                stderr = io.StringIO()

                def poll(self):
                    return None

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 0

                def kill(self):
                    return None

            class FakeThread:
                def __init__(self, *args, **kwargs):
                    self.args = args
                    self.kwargs = kwargs

                def start(self):
                    return None

            class FakeWs:
                def send(self, data):
                    return None

                def recv(self):
                    return ""

                def close(self):
                    return None

            attempts = {"count": 0}

            def connect_after_cold_start(url: str, *, timeout: float):
                attempts["count"] += 1
                if attempts["count"] <= 55:
                    raise ConnectionRefusedError(111, "Connection refused")
                return FakeWs()

            def fake_request(method, params, *, timeout):
                requests.append(method)
                if method == "initialize":
                    return {}
                if method == "thread/start":
                    return {"thread": {"id": "thread-after-cold-start"}}
                raise AssertionError(f"unexpected request: {method}")

            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=False,
            )

            with (
                patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.codex_app_sessions._connect_websocket", side_effect=connect_after_cold_start),
                patch("cccc.daemon.codex_app_sessions.resolve_subprocess_argv", side_effect=lambda argv: list(argv or [])),
                patch("cccc.daemon.codex_app_sessions.time.sleep", return_value=None),
                patch.object(session, "_request", side_effect=fake_request),
            ):
                session.start()

            self.assertEqual(attempts["count"], 56)
            self.assertTrue(session.is_running())
            self.assertEqual(requests, ["initialize", "thread/start"])
        finally:
            if session is not None:
                session.stop()
            cleanup()

    def test_codex_pty_app_missing_resume_records_fresh_thread_from_remote_tui_status(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_codex_app_thread_runtime_session

        home, cleanup = self._with_home()
        session = None
        try:
            cwd = Path(home)
            record_codex_app_thread_runtime_session(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "ws://127.0.0.1:12345"],
                provider_thread_id="thr-stale",
                runner="pty",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex", "--search"],
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

            class FakeWs:
                def send(self, data):
                    return None

                def recv(self):
                    time.sleep(1)
                    return ""

                def close(self):
                    return None

            requests: list[str] = []
            emitted: list[tuple[str, dict]] = []

            def fake_request(method, params, *, timeout):
                requests.append(method)
                if method == "initialize":
                    return {}
                if method == "thread/resume":
                    raise RuntimeError("thread not found")
                raise AssertionError(f"unexpected request: {method}")

            with (
                patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.codex_app_sessions._connect_websocket", return_value=FakeWs()),
                patch("cccc.daemon.codex_app_sessions.resolve_subprocess_argv", side_effect=lambda argv: list(argv or [])),
                patch.object(session, "_request", side_effect=fake_request),
                patch.object(session, "_emit", side_effect=lambda event_type, data: emitted.append((event_type, dict(data or {})))),
                patch("cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.start_actor") as start_remote_tui,
            ):
                session.start()

            self.assertTrue(session.is_running())
            self.assertEqual(requests, ["initialize", "thread/resume"])
            start_remote_tui.assert_called_once()
            self.assertEqual(
                start_remote_tui.call_args.kwargs.get("command"),
                ["codex", "--search", "--remote", "ws://127.0.0.1:12345"],
            )
            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-stale")
            self.assertEqual(stored.get("runner"), "pty")
            self.assertEqual(stored.get("status"), "resume_failed")
            self.assertFalse(bool(stored.get("resume_eligible")))
            self.assertEqual([item[0] for item in emitted], [])

            session._handle_notification(
                "thread/status/changed",
                {"threadId": "thr-fresh", "status": {"type": "idle", "activeFlags": []}},
            )

            self.assertEqual(str(session._session_state.thread_id or ""), "thr-fresh")
            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-fresh")
            self.assertEqual(stored.get("runner"), "pty")
            self.assertEqual(stored.get("captured_from"), "app_server_remote_tui_status")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
            self.assertEqual(str(stored.get("last_resume_error") or ""), "")
        finally:
            if session is not None:
                session.stop()
            cleanup()

    def test_codex_app_stderr_401_marks_runtime_session_auth_failed(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_codex_app_thread_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            record_codex_app_thread_runtime_session(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "ws://127.0.0.1:12345"],
                provider_thread_id="thr-stale",
                runner="pty",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex", "--search"],
            )

            class FakeProc:
                pid = 12345
                stdin = io.StringIO()
                stdout = io.StringIO()
                stderr = io.StringIO(
                    "failed to connect to websocket: HTTP error: 401 Unauthorized, "
                    "url: wss://api.openai.com/v1/responses\n"
                )

                def poll(self):
                    return None

            session._proc = FakeProc()  # type: ignore[assignment]
            session._running = True
            session._runtime_provider_thread_id = "thr-stale"
            session._stderr_loop()

            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-stale")
            self.assertEqual(stored.get("status"), "auth_failed")
            self.assertFalse(bool(stored.get("resume_eligible")))
            self.assertIn("401 Unauthorized", str(stored.get("last_resume_error") or ""))
        finally:
            cleanup()

    def test_codex_app_stale_stderr_401_does_not_mark_fresh_runtime_session(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_codex_app_thread_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            command = ["codex", "app-server", "--listen", "ws://127.0.0.1:12345"]
            record_codex_app_thread_runtime_session(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                command=command,
                provider_thread_id="thr-fresh",
                runner="pty",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex", "--search"],
            )
            session._runtime_command = command
            session._runtime_provider_thread_id = "thr-stale"

            class FakeProc:
                pid = 12346
                stdin = io.StringIO()
                stdout = io.StringIO()
                stderr = io.StringIO(
                    "failed to connect to websocket: HTTP error: 401 Unauthorized, "
                    "url: wss://api.openai.com/v1/responses\n"
                )

                def poll(self):
                    return None

            session._proc = FakeProc()  # type: ignore[assignment]
            session._running = True
            session._stderr_loop()

            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-fresh")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
            self.assertEqual(str(stored.get("last_resume_error") or ""), "")
        finally:
            cleanup()

    def test_codex_app_startup_401_marks_existing_resume_session_auth_failed(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_codex_app_thread_runtime_session

        home, cleanup = self._with_home()
        session = None
        try:
            cwd = Path(home)
            command = ["codex", "app-server", "--listen", "ws://127.0.0.1:12345"]
            record_codex_app_thread_runtime_session(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                command=command,
                provider_thread_id="thr-existing",
                runner="pty",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex", "--search"],
            )

            class FakeProc:
                pid = 12347
                stdin = io.StringIO()
                stdout = io.StringIO()
                stderr = io.StringIO(
                    "failed to connect to websocket: HTTP error: 401 Unauthorized, "
                    "url: wss://api.openai.com/v1/responses\n"
                )

                def poll(self):
                    return None

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 0

                def kill(self):
                    return None

            class FakeThread:
                def __init__(self, *args, **kwargs):
                    self.args = args
                    self.kwargs = kwargs

                def start(self):
                    name = str(self.kwargs.get("name") or "")
                    target = self.kwargs.get("target")
                    if "cccc-codex-err:" in name and callable(target):
                        target()

            class FakeWs:
                def send(self, data):
                    return None

                def recv(self):
                    return ""

                def close(self):
                    return None

            requests: list[str] = []
            started_pty: list[dict] = []

            def fake_request(method, params, *, timeout):
                requests.append(method)
                if method == "initialize":
                    return {}
                raise AssertionError(f"unexpected request after startup 401: {method}")

            def fake_start_actor(**kwargs):
                started_pty.append(dict(kwargs))

                class _PtySession:
                    pid = 22222

                    def is_running(self):
                        return True

                return _PtySession()

            with (
                patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.codex_app_sessions._connect_websocket", return_value=FakeWs()),
                patch("cccc.daemon.codex_app_sessions.resolve_subprocess_argv", side_effect=lambda argv: list(argv or [])),
                patch.object(session, "_request", side_effect=fake_request),
                patch("cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.start_actor", side_effect=fake_start_actor),
            ):
                session.start()

            self.assertEqual(requests, ["initialize"])
            self.assertEqual(len(started_pty), 1)
            self.assertNotIn("resume", started_pty[0]["command"])
            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-existing")
            self.assertEqual(stored.get("status"), "auth_failed")
            self.assertFalse(bool(stored.get("resume_eligible")))
            self.assertIn("401 Unauthorized", str(stored.get("last_resume_error") or ""))
        finally:
            if session is not None:
                session.stop()
            cleanup()

    def test_codex_pty_app_resume_launches_remote_tui_with_thread_id(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_codex_app_thread_runtime_session

        home, cleanup = self._with_home()
        session = None
        try:
            cwd = Path(home)
            record_codex_app_thread_runtime_session(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "ws://127.0.0.1:12345"],
                provider_thread_id="thr-existing",
                runner="pty",
                status="usable",
                captured_from="app_server_remote_tui_thread_list",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex", "--search"],
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

            class FakeWs:
                def send(self, data):
                    return None

                def recv(self):
                    time.sleep(1)
                    return ""

                def close(self):
                    return None

            requests: list[tuple[str, dict]] = []
            started_pty: list[dict] = []

            def fake_request(method, params, *, timeout):
                requests.append((method, dict(params or {})))
                if method == "initialize":
                    return {}
                if method == "thread/resume":
                    return {"thread": {"id": "thr-existing"}}
                raise AssertionError(f"unexpected request: {method}")

            def fake_start_actor(**kwargs):
                started_pty.append(dict(kwargs))

                class _PtySession:
                    pid = 22222

                return _PtySession()

            with (
                patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.codex_app_sessions._connect_websocket", return_value=FakeWs()),
                patch("cccc.daemon.codex_app_sessions.resolve_subprocess_argv", side_effect=lambda argv: list(argv or [])),
                patch.object(session, "_request", side_effect=fake_request),
                patch("cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.start_actor", side_effect=fake_start_actor),
            ):
                session.start()

            self.assertEqual([item[0] for item in requests], ["initialize", "thread/resume"])
            self.assertEqual(requests[1][1].get("threadId"), "thr-existing")
            self.assertEqual(len(started_pty), 1)
            self.assertEqual(
                started_pty[0]["command"],
                ["codex", "--search", "resume", "thr-existing", "--remote", "ws://127.0.0.1:12345"],
            )
            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-existing")
            self.assertEqual(stored.get("captured_from"), "app_server_thread_resume")
            self.assertEqual(stored.get("runner"), "pty")
        finally:
            if session is not None:
                session.stop()
            cleanup()

    def test_codex_pty_app_stale_resume_starts_plain_remote_tui(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_codex_app_thread_runtime_session

        home, cleanup = self._with_home()
        session = None
        try:
            cwd = Path(home)
            record_codex_app_thread_runtime_session(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "ws://127.0.0.1:12345"],
                provider_thread_id="thr-bootstrap-stale",
                runner="pty",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex", "--search"],
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

            class FakeWs:
                def send(self, data):
                    return None

                def recv(self):
                    time.sleep(1)
                    return ""

                def close(self):
                    return None

            requests: list[tuple[str, dict]] = []
            started_pty: list[dict] = []

            def fake_request(method, params, *, timeout):
                requests.append((method, dict(params or {})))
                if method == "initialize":
                    return {}
                if method == "thread/resume" and params.get("threadId") == "thr-bootstrap-stale":
                    raise RuntimeError("no rollout found for thread id thr-bootstrap-stale")
                raise AssertionError(f"unexpected request: {method} {params}")

            def fake_start_actor(**kwargs):
                started_pty.append(dict(kwargs))

                class _PtySession:
                    pid = 22222

                return _PtySession()

            with (
                patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.codex_app_sessions._connect_websocket", return_value=FakeWs()),
                patch("cccc.daemon.codex_app_sessions.resolve_subprocess_argv", side_effect=lambda argv: list(argv or [])),
                patch.object(session, "_request", side_effect=fake_request),
                patch("cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.start_actor", side_effect=fake_start_actor),
            ):
                session.start()

            self.assertEqual(
                [item[0] for item in requests],
                ["initialize", "thread/resume"],
            )
            self.assertEqual(started_pty[0]["command"], ["codex", "--search", "--remote", "ws://127.0.0.1:12345"])
            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-bootstrap-stale")
            self.assertEqual(stored.get("status"), "resume_failed")
            self.assertFalse(bool(stored.get("resume_eligible")))

            session._handle_notification(
                "turn/started",
                {"turn": {"id": "turn-fresh", "threadId": "thr-fresh-after-stale"}},
            )

            self.assertEqual(str(session._session_state.thread_id or ""), "thr-fresh-after-stale")
            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-fresh-after-stale")
            self.assertEqual(stored.get("runner"), "pty")
            self.assertEqual(stored.get("captured_from"), "app_server_remote_tui_turn_started")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
            self.assertEqual(str(stored.get("last_resume_error") or ""), "")
        finally:
            if session is not None:
                session.stop()
            cleanup()

    def test_codex_pty_app_remote_tui_exit_does_not_mark_thread_unresumable(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, CodexAppSessionManager
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_codex_app_thread_runtime_session

        home, cleanup = self._with_home()
        try:
            manager = CodexAppSessionManager()
            cwd = Path(home)
            record_codex_app_thread_runtime_session(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "ws://127.0.0.1:12345"],
                provider_thread_id="thr-fresh-but-dead",
                runner="pty",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex"],
            )

            class _Proc:
                pid = os.getpid()

                def poll(self):
                    return None

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 0

            class _Ws:
                def close(self):
                    return None

            class _PtySession:
                pid = 22222

            session._proc = _Proc()
            session._ws = _Ws()
            session._running = True
            session._pty_session = _PtySession()
            session._session_state.thread_id = "thr-fresh-but-dead"
            manager._sessions[("g_test", "peer1")] = session

            handled = manager.handle_remote_tui_exit(group_id="g_test", actor_id="peer1", pid=22222)

            self.assertTrue(handled)
            self.assertFalse(session.is_running())
            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-fresh-but-dead")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
            self.assertEqual(str(stored.get("last_resume_error") or ""), "")
        finally:
            manager.stop_actor(group_id="g_test", actor_id="peer1")
            cleanup()

    def test_codex_pty_app_remote_tui_start_failure_does_not_mark_resume_failed(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import read_runtime_session

        home, cleanup = self._with_home()
        session = None
        try:
            cwd = Path(home)
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex"],
            )

            class FakeProc:
                pid = 12345
                stdin = io.StringIO()
                stdout = io.StringIO()
                stderr = io.StringIO()

                def poll(self):
                    return None

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 0

            class FakeThread:
                def __init__(self, *args, **kwargs):
                    self.args = args
                    self.kwargs = kwargs

                def start(self):
                    return None

            class FakeWs:
                def send(self, data):
                    return None

                def recv(self):
                    time.sleep(1)
                    return ""

                def close(self):
                    return None

            def fake_request(method, params, *, timeout):
                if method == "initialize":
                    return {}
                raise AssertionError(f"unexpected request: {method}")

            with (
                patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
                patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=FakeProc()),
                patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=FakeThread),
                patch("cccc.daemon.codex_app_sessions._connect_websocket", return_value=FakeWs()),
                patch.object(session, "_request", side_effect=fake_request),
                patch("cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.start_actor", side_effect=RuntimeError("remote tui failed")),
            ):
                with self.assertRaisesRegex(RuntimeError, "remote tui failed"):
                    session.start()

            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored, {})
        finally:
            if session is not None:
                session.stop()
            cleanup()

    def test_codex_pty_app_session_adopts_thread_from_status_notification(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(group_id="g_test", actor_id="peer1", cwd=Path(home), env={})
            session._running = True
            session._session_state.status = "idle"

            session._handle_notification(
                "thread/status/changed",
                {"threadId": "thread-from-tui", "status": {"type": "active", "activeFlags": []}},
            )

            self.assertEqual(str(session._session_state.thread_id or ""), "thread-from-tui")
            self.assertEqual(str(session._session_state.status or ""), "idle")
            self.assertIsNone(session._session_state.current_task_id)
        finally:
            cleanup()

    def test_codex_pty_app_records_thread_started_notification(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession
        from cccc.daemon.runtime_session_ops import (
            mark_runtime_session_resume_failed,
            read_runtime_session,
            record_codex_app_thread_runtime_session,
        )

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            command = ["codex", "app-server", "--listen", "ws://127.0.0.1:12345"]
            record_codex_app_thread_runtime_session(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                command=command,
                provider_thread_id="thr-stale",
                runner="pty",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            mark_runtime_session_resume_failed(group_id="g_test", actor_id="peer1", error="thread not found")
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=cwd,
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex"],
            )
            session._running = True
            session._runtime_command = command

            session._handle_notification("thread/started", {"thread": {"id": "thr-fresh-started"}})

            self.assertEqual(str(session._session_state.thread_id or ""), "thr-fresh-started")
            stored = read_runtime_session("g_test", "peer1")
            self.assertEqual(stored.get("provider_thread_id"), "thr-fresh-started")
            self.assertEqual(stored.get("runner"), "pty")
            self.assertEqual(stored.get("captured_from"), "app_server_remote_tui_thread_started")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
            self.assertEqual(str(stored.get("last_resume_error") or ""), "")
        finally:
            cleanup()

    def test_codex_pty_app_remote_tui_exit_stops_app_server_session(self) -> None:
        from cccc.daemon.server import _handle_pty_session_exit
        from cccc.kernel.actors import add_actor, update_actor
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry

        home, cleanup = self._with_home()
        try:
            reg = load_registry()
            created = create_group(reg, title="pty-app-exit", topic="")
            group = load_group(created.group_id)
            self.assertIsNotNone(group)
            add_actor(group, actor_id="peer1", title="Peer 1", runtime="codex", runner="pty")  # type: ignore[arg-type]
            update_actor(group, "peer1", {"runtime_state_source": "app_server"})  # type: ignore[arg-type]
            group.save()  # type: ignore[union-attr]

            class _Session:
                group_id = created.group_id
                actor_id = "peer1"
                pid = 12345

            with patch("cccc.daemon.server._remove_pty_state_if_pid", return_value=True), patch(
                "cccc.daemon.server.codex_app_supervisor.stop_actor"
            ) as stop_actor:
                _handle_pty_session_exit(_Session())  # type: ignore[arg-type]

            stop_actor.assert_called_once_with(group_id=created.group_id, actor_id="peer1")
        finally:
            cleanup()

    def test_codex_pty_app_observer_websocket_close_keeps_remote_tui_session(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
            )

            class _Proc:
                pid = os.getpid()

                @staticmethod
                def poll():
                    return None

            class _Ws:
                @staticmethod
                def recv():
                    raise RuntimeError("observer websocket closed")

            class _PtySession:
                pid = 22222

            session._proc = _Proc()
            session._ws = _Ws()
            session._pty_session = _PtySession()
            session._running = True

            with patch(
                "cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.actor_running",
                return_value=True,
            ), patch.object(
                session,
                "stop",
            ) as stop:
                session._websocket_loop()

            stop.assert_not_called()
            self.assertTrue(session.is_running())
        finally:
            cleanup()

    def test_codex_pty_app_start_replaces_app_server_when_remote_tui_is_missing(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, CodexAppSessionManager

        home, cleanup = self._with_home()
        try:
            manager = CodexAppSessionManager()
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
                remote_tui_base_command=["codex", "--search"],
            )

            class _Proc:
                pid = os.getpid()
                terminated = False

                def poll(self):
                    return None

                def terminate(self):
                    self.terminated = True

                def wait(self, timeout=None):
                    return 0

            session._proc = _Proc()
            session._running = True
            session._session_state.thread_id = "thr-fresh"
            manager._sessions[("g_test", "peer1")] = session

            with patch(
                "cccc.daemon.codex_app_sessions._codex_cli_available",
                return_value=True,
            ), patch.object(CodexAppSession, "start", autospec=True) as start:
                returned = manager.start_pty_app_actor(
                    group_id="g_test",
                    actor_id="peer1",
                    cwd=Path(home),
                    env={},
                    remote_tui_base_command=["codex", "--search"],
                )

            self.assertIsNot(returned, session)
            self.assertFalse(session.is_running())
            self.assertIs(manager._sessions[("g_test", "peer1")], returned)
            self.assertTrue(returned.start_remote_tui)
            start.assert_called_once_with(returned)
        finally:
            manager.stop_actor(group_id="g_test", actor_id="peer1")
            cleanup()

    def test_codex_pty_app_observer_websocket_close_does_not_persist_actor_stop(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
                start_remote_tui=True,
            )

            class _Proc:
                pid = os.getpid()

                @staticmethod
                def poll():
                    return None

            class _Ws:
                @staticmethod
                def recv():
                    raise RuntimeError("observer websocket closed")

            class _PtySession:
                pid = 22222

            session._proc = _Proc()
            session._ws = _Ws()
            session._pty_session = _PtySession()
            session._running = True

            with patch(
                "cccc.daemon.codex_app_sessions.pty_runner.SUPERVISOR.actor_running",
                return_value=False,
            ), patch.object(
                session,
                "stop",
            ) as stop:
                session._websocket_loop()

            stop.assert_called_once_with(persist_actor_stopped=False)
        finally:
            cleanup()

    def test_codex_app_stop_closes_websocket_with_bounded_timeout(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, _WEBSOCKET_CLOSE_TIMEOUT_SECONDS

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
            )

            class _Proc:
                pid = 12345

                @staticmethod
                def poll():
                    return None

                @staticmethod
                def terminate():
                    return None

                @staticmethod
                def wait(timeout=None):
                    return 0

            class _Ws:
                def __init__(self) -> None:
                    self.set_timeouts: list[float] = []
                    self.close_timeouts: list[float] = []

                def settimeout(self, timeout):
                    self.set_timeouts.append(float(timeout))

                def close(self, *, timeout=None):
                    self.close_timeouts.append(float(timeout))

            ws = _Ws()
            session._proc = _Proc()
            session._ws = ws
            session._running = True

            session.stop()

            self.assertEqual(ws.set_timeouts, [_WEBSOCKET_CLOSE_TIMEOUT_SECONDS])
            self.assertEqual(ws.close_timeouts, [_WEBSOCKET_CLOSE_TIMEOUT_SECONDS])
        finally:
            cleanup()

    def test_codex_app_stop_prefers_immediate_websocket_shutdown(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_test",
                actor_id="peer1",
                cwd=Path(home),
                env={},
                listen_url="ws://127.0.0.1:12345",
                transport="websocket",
                persist_headless_state=False,
            )

            class _Proc:
                pid = 12345

                @staticmethod
                def poll():
                    return None

                @staticmethod
                def terminate():
                    return None

                @staticmethod
                def wait(timeout=None):
                    return 0

            class _Ws:
                shutdown_called = False
                close_called = False

                def shutdown(self):
                    self.shutdown_called = True

                def close(self, *, timeout=None):
                    self.close_called = True

            ws = _Ws()
            session._proc = _Proc()
            session._ws = ws
            session._running = True

            session.stop()

            self.assertTrue(ws.shutdown_called)
            self.assertFalse(ws.close_called)
        finally:
            cleanup()

    def test_codex_app_stop_uses_windows_process_tree_termination_for_real_popen(self) -> None:
        from cccc.daemon import codex_app_sessions

        class _Proc:
            pid = 12345
            terminate_called = False

            def terminate(self):
                self.terminate_called = True

            def wait(self, timeout=None):
                return 0

        proc = _Proc()

        with patch("cccc.daemon.codex_app_sessions.os.name", "nt"), patch(
            "cccc.daemon.codex_app_sessions._is_subprocess_popen",
            return_value=True,
        ), patch("cccc.daemon.codex_app_sessions.terminate_pid", return_value=True) as terminate_pid:
            codex_app_sessions._terminate_codex_app_server_process(proc)

        terminate_pid.assert_called_once_with(12345, timeout_s=2.0, include_group=True, force=True)
        self.assertFalse(proc.terminate_called)

    def test_codex_app_shutdown_stdout_close_does_not_persist_actor_stop(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession, CodexAppSessionManager

        home, cleanup = self._with_home()
        try:
            manager = CodexAppSessionManager()
            session = CodexAppSession(
                group_id="g_codex_shutdown",
                actor_id="peer1",
                cwd=Path(home),
                env={},
            )

            class FakeProc:
                pid = os.getpid()
                stdin = io.StringIO()
                stdout = io.StringIO("")
                stderr = io.StringIO()

                def poll(self):
                    return None

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 0

            session._proc = FakeProc()
            session._running = True
            manager._sessions[("g_codex_shutdown", "peer1")] = session

            with patch("cccc.daemon.codex_app_sessions.persist_actor_process_exit_stopped") as persist_stopped:
                manager.begin_shutdown()
                session._stdout_loop()

            persist_stopped.assert_not_called()
            self.assertFalse(session.is_running())
        finally:
            cleanup()

    def test_codex_remote_tui_command_filters_prompt_and_keeps_approval_flags(self) -> None:
        from cccc.daemon.codex_app_sessions import _codex_remote_tui_command

        command = _codex_remote_tui_command(
            [
                "codex",
                "-c",
                "shell_environment_policy.inherit=all",
                "--dangerously-bypass-approvals-and-sandbox",
                "--search",
                "do not forward this prompt",
            ],
            "ws://127.0.0.1:12345",
        )
        self.assertEqual(
            command,
            [
                "codex",
                "-c",
                "shell_environment_policy.inherit=all",
                "--dangerously-bypass-approvals-and-sandbox",
                "--search",
                "--remote",
                "ws://127.0.0.1:12345",
            ],
        )

    def test_codex_remote_tui_command_accepts_windows_cmd_shim(self) -> None:
        from cccc.daemon.codex_app_sessions import _codex_remote_tui_command

        command = _codex_remote_tui_command(
            [
                r"C:\Tools\codex.cmd",
                "-m",
                "gpt-5.5",
                "--search",
                "do not forward this prompt",
            ],
            "ws://127.0.0.1:12345",
        )

        self.assertEqual(
            command,
            [
                r"C:\Tools\codex.cmd",
                "-m",
                "gpt-5.5",
                "--search",
                "--remote",
                "ws://127.0.0.1:12345",
            ],
        )

    def test_codex_websocket_idle_timeout_does_not_stop_session(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        home, cleanup = self._with_home()
        try:
            session = CodexAppSession(
                group_id="g_ws_timeout",
                actor_id="peer1",
                cwd=Path(home),
                env={},
                transport="websocket",
            )

            class FakeProc:
                pid = 12345

                def poll(self):
                    return None

                def terminate(self):
                    return None

                def wait(self, timeout=None):
                    return 0

            class FakeWsTimeout(Exception):
                pass

            FakeWsTimeout.__name__ = "WebSocketTimeoutException"

            release = threading.Event()
            entered_second_recv = threading.Event()

            class FakeWs:
                count = 0

                def recv(self):
                    self.count += 1
                    if self.count == 1:
                        raise FakeWsTimeout("timed out")
                    entered_second_recv.set()
                    release.wait(timeout=1)
                    raise RuntimeError("closed")

                def close(self):
                    release.set()

            session._proc = FakeProc()
            session._ws = FakeWs()
            session._running = True

            thread = threading.Thread(target=session._websocket_loop, daemon=True)
            thread.start()
            self.assertTrue(entered_second_recv.wait(timeout=1))
            self.assertTrue(session.is_running())
            release.set()
            thread.join(timeout=1)
            self.assertFalse(thread.is_alive())
        finally:
            cleanup()

if __name__ == "__main__":
    unittest.main()
