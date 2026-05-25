import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestRuntimeSessionOps(unittest.TestCase):
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

        return Path(td), cleanup

    def test_parse_resume_hints_uses_explicit_resume_only(self) -> None:
        from cccc.daemon.runtime_session_ops import parse_codex_status_session_id, parse_runtime_resume_hint

        claude = parse_runtime_resume_hint(
            "Resume this session with:\nclaude --resume 42e9ef0c-3b75-43a0-9056-eef13dd1061d"
        )
        self.assertEqual(claude.get("runtime"), "claude")
        self.assertEqual(claude.get("provider_session_id"), "42e9ef0c-3b75-43a0-9056-eef13dd1061d")

        codex = parse_runtime_resume_hint(
            "To continue this session, run codex resume 019dbe1d-cd97-7d31-9ba6-212d3e57b15c"
        )
        self.assertEqual(codex.get("runtime"), "codex")
        self.assertEqual(codex.get("provider_session_id"), "019dbe1d-cd97-7d31-9ba6-212d3e57b15c")

        self.assertEqual(parse_runtime_resume_hint("claude --continue"), {})
        self.assertEqual(
            parse_codex_status_session_id("Session:                     019dbe1d-cd97-7d31-9ba6-212d3e57b15c"),
            "019dbe1d-cd97-7d31-9ba6-212d3e57b15c",
        )
        gemini = parse_runtime_resume_hint("Resume later with: gemini --resume gemini-session-1234")
        self.assertEqual(gemini.get("runtime"), "gemini")
        self.assertEqual(gemini.get("provider_session_id"), "gemini-session-1234")

    def test_pty_resume_verify_default_is_long_enough_for_slow_provider_resume(self) -> None:
        from cccc.daemon.runtime_session_ops import _pty_resume_verify_seconds

        with patch.dict(os.environ, {}, clear=True):
            self.assertEqual(_pty_resume_verify_seconds(), 20.0)

    def test_pty_resume_verify_seconds_env_override(self) -> None:
        from cccc.daemon.runtime_session_ops import _pty_resume_verify_seconds

        with patch.dict(os.environ, {"CCCC_PTY_RESUME_VERIFY_SECONDS": "3.5"}):
            self.assertEqual(_pty_resume_verify_seconds(), 3.5)

    def test_claude_first_start_generates_explicit_session_id(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import prepare_initial_pty_session_command, read_runtime_session

            cwd = home / "repo"
            cwd.mkdir()
            with patch("cccc.daemon.runtime_session_ops.uuid.uuid4", return_value="42e9ef0c-3b75-43a0-9056-eef13dd1061d"):
                command, doc = prepare_initial_pty_session_command(
                    group_id="g1",
                    actor_id="peer1",
                    runtime="claude",
                    cwd=cwd,
                    base_command=["claude", "--dangerously-skip-permissions"],
                    env={},
                    max_backlog_bytes=1000,
                )

            self.assertEqual(
                command,
                [
                    "claude",
                    "--session-id",
                    "42e9ef0c-3b75-43a0-9056-eef13dd1061d",
                    "--dangerously-skip-permissions",
                ],
            )
            self.assertIsNotNone(doc)
            stored = read_runtime_session("g1", "peer1")
            self.assertEqual(stored.get("provider_session_id"), "42e9ef0c-3b75-43a0-9056-eef13dd1061d")
            self.assertEqual(stored.get("captured_from"), "claude_generated_session_id")
            self.assertTrue(bool(stored.get("resume_eligible")))
        finally:
            cleanup()

    def test_claude_existing_session_control_is_not_rewritten(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import prepare_initial_pty_session_command, read_runtime_session

            cwd = home / "repo"
            cwd.mkdir()
            command, doc = prepare_initial_pty_session_command(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                base_command=["claude", "--continue"],
                env={},
                max_backlog_bytes=1000,
            )

            self.assertEqual(command, ["claude", "--continue"])
            self.assertIsNone(doc)
            self.assertEqual(read_runtime_session("g1", "peer1"), {})
        finally:
            cleanup()

    def test_gemini_first_start_generates_explicit_session_id(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import prepare_initial_pty_session_command, read_runtime_session

            cwd = home / "repo"
            cwd.mkdir()
            with patch("cccc.daemon.runtime_session_ops.uuid.uuid4", return_value="a3df810a-6a19-48e8-b75b-772d3ee65721"):
                command, doc = prepare_initial_pty_session_command(
                    group_id="g1",
                    actor_id="peer1",
                    runtime="gemini",
                    cwd=cwd,
                    base_command=["gemini", "--yolo"],
                    env={},
                    max_backlog_bytes=1000,
                )

            self.assertEqual(command, ["gemini", "--session-id", "a3df810a-6a19-48e8-b75b-772d3ee65721", "--yolo"])
            self.assertIsNotNone(doc)
            stored = read_runtime_session("g1", "peer1")
            self.assertEqual(stored.get("runtime"), "gemini")
            self.assertEqual(stored.get("provider_session_id"), "a3df810a-6a19-48e8-b75b-772d3ee65721")
            self.assertEqual(stored.get("resume_command_hint"), "gemini --resume a3df810a-6a19-48e8-b75b-772d3ee65721")
            self.assertEqual(stored.get("captured_from"), "gemini_generated_session_id")
            self.assertTrue(bool(stored.get("resume_eligible")))
        finally:
            cleanup()

    def test_gemini_existing_session_control_or_subcommand_is_not_rewritten(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import prepare_initial_pty_session_command, read_runtime_session

            cwd = home / "repo"
            cwd.mkdir()
            command, doc = prepare_initial_pty_session_command(
                group_id="g1",
                actor_id="peer1",
                runtime="gemini",
                cwd=cwd,
                base_command=["gemini", "--session-id", "user-owned-session", "--yolo"],
                env={},
                max_backlog_bytes=1000,
            )
            self.assertEqual(command, ["gemini", "--session-id", "user-owned-session", "--yolo"])
            self.assertIsNone(doc)

            command, doc = prepare_initial_pty_session_command(
                group_id="g1",
                actor_id="peer-short",
                runtime="gemini",
                cwd=cwd,
                base_command=["gemini", "-r", "latest", "--yolo"],
                env={},
                max_backlog_bytes=1000,
            )
            self.assertEqual(command, ["gemini", "-r", "latest", "--yolo"])
            self.assertIsNone(doc)

            command, doc = prepare_initial_pty_session_command(
                group_id="g1",
                actor_id="peer2",
                runtime="gemini",
                cwd=cwd,
                base_command=["gemini", "mcp", "add", "cccc"],
                env={},
                max_backlog_bytes=1000,
            )
            self.assertEqual(command, ["gemini", "mcp", "add", "cccc"])
            self.assertIsNone(doc)
            self.assertEqual(read_runtime_session("g1", "peer1"), {})
            self.assertEqual(read_runtime_session("g1", "peer2"), {})
            self.assertEqual(read_runtime_session("g1", "peer-short"), {})
        finally:
            cleanup()

    def test_existing_claude_session_prepares_explicit_resume(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                prepare_pty_resume_command,
                record_pty_runtime_session,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=["claude", "--dangerously-skip-permissions"],
                provider_session_id="42e9ef0c-3b75-43a0-9056-eef13dd1061d",
                captured_from="claude_generated_session_id",
            )

            command, resume_doc = prepare_pty_resume_command(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                base_command=["claude", "--dangerously-skip-permissions"],
            )
            self.assertIsNotNone(resume_doc)
            self.assertEqual(
                command,
                ["claude", "--resume", "42e9ef0c-3b75-43a0-9056-eef13dd1061d", "--dangerously-skip-permissions"],
            )
        finally:
            cleanup()

    def test_existing_gemini_session_prepares_explicit_resume(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                prepare_pty_resume_command,
                record_pty_runtime_session,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="gemini",
                cwd=cwd,
                command=["gemini", "--yolo"],
                provider_session_id="a3df810a-6a19-48e8-b75b-772d3ee65721",
                captured_from="gemini_generated_session_id",
            )

            command, resume_doc = prepare_pty_resume_command(
                group_id="g1",
                actor_id="peer1",
                runtime="gemini",
                cwd=cwd,
                base_command=["gemini", "--yolo"],
            )
            self.assertIsNotNone(resume_doc)
            self.assertEqual(command, ["gemini", "--resume", "a3df810a-6a19-48e8-b75b-772d3ee65721", "--yolo"])
        finally:
            cleanup()

    def test_runtime_resume_env_disables_initial_and_resume_commands(self) -> None:
        home, cleanup = self._with_home()
        old = os.environ.get("CCCC_RUNTIME_RESUME")
        os.environ["CCCC_RUNTIME_RESUME"] = "0"
        try:
            from cccc.daemon.runtime_session_ops import (
                prepare_initial_pty_session_command,
                prepare_pty_resume_command,
                record_pty_runtime_session,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=["claude"],
                provider_session_id="42e9ef0c-3b75-43a0-9056-eef13dd1061d",
                captured_from="test",
            )

            command, doc = prepare_pty_resume_command(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                base_command=["claude"],
            )
            self.assertEqual(command, ["claude"])
            self.assertIsNone(doc)

            command, doc = prepare_initial_pty_session_command(
                group_id="g1",
                actor_id="peer2",
                runtime="claude",
                cwd=cwd,
                base_command=["claude"],
                env={},
                max_backlog_bytes=1000,
            )
            self.assertEqual(command, ["claude"])
            self.assertIsNone(doc)
        finally:
            if old is None:
                os.environ.pop("CCCC_RUNTIME_RESUME", None)
            else:
                os.environ["CCCC_RUNTIME_RESUME"] = old
            cleanup()

    def test_initial_preflight_failure_does_not_write_planned_claude_session(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import read_runtime_session, start_pty_actor_with_runtime_resume

            cwd = home / "repo"
            cwd.mkdir()

            with self.assertRaises(RuntimeError):
                start_pty_actor_with_runtime_resume(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["claude"],
                    env={},
                    runtime="claude",
                    max_backlog_bytes=1000,
                    runtime_start_preflight_error=lambda runtime, command, runner="pty": "missing claude",
                )

            self.assertEqual(read_runtime_session("g1", "peer1"), {})
        finally:
            cleanup()

    def test_codex_first_start_schedules_status_capture_after_launch(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import start_pty_actor_with_runtime_resume

            cwd = home / "repo"
            cwd.mkdir()

            class FallbackSession:
                pid = 123

            calls: list[list[str]] = []
            scheduled: list[dict] = []

            def fake_start_actor(**kwargs):
                calls.append(list(kwargs.get("command") or []))
                return FallbackSession()

            with patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.start_actor",
                side_effect=fake_start_actor,
            ), patch(
                "cccc.daemon.runtime_session_ops._schedule_codex_pty_status_capture",
                side_effect=lambda **kwargs: scheduled.append(kwargs),
            ):
                session = start_pty_actor_with_runtime_resume(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["codex", "-c", "shell_environment_policy.inherit=all"],
                    env={},
                    runtime="codex",
                    max_backlog_bytes=1000,
                    runtime_start_preflight_error=lambda runtime, command, runner="pty": "",
                )

            self.assertIsInstance(session, FallbackSession)
            self.assertEqual(calls, [["codex", "-c", "shell_environment_policy.inherit=all"]])
            self.assertEqual(len(scheduled), 1)
            self.assertEqual(scheduled[0]["group_id"], "g1")
            self.assertEqual(scheduled[0]["actor_id"], "peer1")
            self.assertEqual(scheduled[0]["base_command"], ["codex", "-c", "shell_environment_policy.inherit=all"])
        finally:
            cleanup()

    def test_already_running_initial_start_does_not_write_generated_session_id(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import read_runtime_session, start_pty_actor_with_runtime_resume

            cwd = home / "repo"
            cwd.mkdir()

            class ExistingSession:
                pid = 444

            calls: list[list[str]] = []

            def fake_start_actor(**kwargs):
                calls.append(list(kwargs.get("command") or []))
                return ExistingSession()

            with patch("cccc.daemon.runtime_session_ops.uuid.uuid4", return_value="unused-session-id"), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.actor_running",
                return_value=True,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.start_actor",
                side_effect=fake_start_actor,
            ):
                session = start_pty_actor_with_runtime_resume(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["gemini", "--yolo"],
                    env={},
                    runtime="gemini",
                    max_backlog_bytes=1000,
                    runtime_start_preflight_error=lambda runtime, command, runner="pty": "",
                )

            self.assertIsInstance(session, ExistingSession)
            self.assertEqual(calls, [["gemini", "--yolo"]])
            self.assertEqual(read_runtime_session("g1", "peer1"), {})
        finally:
            cleanup()

    def test_codex_status_capture_records_current_pty_session_id(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import _capture_codex_pty_session_from_status, read_runtime_session

            cwd = home / "repo"
            cwd.mkdir()
            writes: list[bytes] = []
            def fake_tail_output(**kwargs):
                if writes:
                    return b"Welcome to Codex\n/status\nSession:                     019dbe1d-cd97-7d31-9ba6-212d3e57b15c\n"
                return b"Welcome to Codex\n"

            def fake_write_input(**kwargs):
                writes.append(bytes(kwargs.get("data") or b""))
                return True

            with patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.actor_running",
                return_value=True,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.tail_output",
                side_effect=fake_tail_output,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.write_input",
                side_effect=fake_write_input,
            ), patch.dict(os.environ, {"CCCC_CODEX_PTY_STATUS_SUBMIT_DELAY_SECONDS": "0"}):
                doc = _capture_codex_pty_session_from_status(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["codex"],
                    timeout_seconds=1.0,
                )

            self.assertEqual(writes, [b"/status", b"\r"])
            self.assertEqual(doc.get("captured_from"), "codex_status_command")
            stored = read_runtime_session("g1", "peer1")
            self.assertEqual(stored.get("provider_session_id"), "019dbe1d-cd97-7d31-9ba6-212d3e57b15c")
            self.assertEqual(stored.get("captured_from"), "codex_status_command")
        finally:
            cleanup()

    def test_codex_status_capture_uses_bracketed_paste_when_available(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import _capture_codex_pty_session_from_status

            cwd = home / "repo"
            cwd.mkdir()
            writes: list[bytes] = []

            def fake_tail_output(**kwargs):
                if writes:
                    return b"Session:                     019dbe1d-cd97-7d31-9ba6-212d3e57b15c\n"
                return b"Welcome to Codex\n"

            def fake_write_input(**kwargs):
                writes.append(bytes(kwargs.get("data") or b""))
                return True

            with patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.actor_running",
                return_value=True,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.tail_output",
                side_effect=fake_tail_output,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.bracketed_paste_enabled",
                return_value=True,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.write_input",
                side_effect=fake_write_input,
            ), patch.dict(os.environ, {"CCCC_CODEX_PTY_STATUS_SUBMIT_DELAY_SECONDS": "0"}):
                doc = _capture_codex_pty_session_from_status(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["codex"],
                    timeout_seconds=1.0,
                )

            self.assertEqual(writes, [b"\x1b[200~/status\x1b[201~", b"\r"])
            self.assertEqual(doc.get("captured_from"), "codex_status_command")
        finally:
            cleanup()

    def test_codex_status_capture_without_session_id_does_not_record(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import _record_codex_pty_session_from_status_text, read_runtime_session

            cwd = home / "repo"
            cwd.mkdir()
            doc = _record_codex_pty_session_from_status_text(
                group_id="g1",
                actor_id="peer1",
                cwd=cwd,
                base_command=["codex"],
                text="Codex status\nModel: gpt-5\n",
            )
            self.assertEqual(doc, {})
            self.assertEqual(read_runtime_session("g1", "peer1"), {})
        finally:
            cleanup()

    def test_resume_start_failure_falls_back_to_fresh_actor(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                read_runtime_session,
                record_pty_runtime_session,
                start_pty_actor_with_runtime_resume,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=["claude"],
                provider_session_id="old-session-id",
                captured_from="test",
            )

            calls: list[list[str]] = []

            class FreshSession:
                pid = 222

            def fake_start_actor(**kwargs):
                calls.append(list(kwargs.get("command") or []))
                if len(calls) == 1:
                    raise RuntimeError("resume id not found")
                return FreshSession()

            new_session_id = "a3df810a-6a19-48e8-b75b-772d3ee65721"
            with patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.start_actor",
                side_effect=fake_start_actor,
            ), patch("cccc.daemon.runtime_session_ops.uuid.uuid4", return_value=new_session_id):
                session = start_pty_actor_with_runtime_resume(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["claude"],
                    env={},
                    runtime="claude",
                    max_backlog_bytes=1000,
                    runtime_start_preflight_error=lambda runtime, command, runner="pty": "",
                )

            self.assertIsInstance(session, FreshSession)
            self.assertEqual(
                calls,
                [["claude", "--resume", "old-session-id"], ["claude", "--session-id", new_session_id]],
            )
            stored = read_runtime_session("g1", "peer1")
            self.assertEqual(stored.get("runtime"), "claude")
            self.assertEqual(stored.get("provider_session_id"), new_session_id)
            self.assertEqual(stored.get("captured_from"), "claude_generated_session_id")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
            self.assertEqual(str(stored.get("last_resume_error") or ""), "")
        finally:
            cleanup()

    def test_gemini_resume_failure_fallback_generates_new_durable_session(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                read_runtime_session,
                record_pty_runtime_session,
                start_pty_actor_with_runtime_resume,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="gemini",
                cwd=cwd,
                command=["gemini", "--yolo"],
                provider_session_id="old-gemini-session-id",
                captured_from="gemini_generated_session_id",
            )

            calls: list[list[str]] = []

            class FreshSession:
                pid = 222

            def fake_start_actor(**kwargs):
                calls.append(list(kwargs.get("command") or []))
                if len(calls) == 1:
                    raise RuntimeError("resume session not found")
                return FreshSession()

            new_session_id = "42e9ef0c-3b75-43a0-9056-eef13dd1061d"
            with patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.start_actor",
                side_effect=fake_start_actor,
            ), patch("cccc.daemon.runtime_session_ops.uuid.uuid4", return_value=new_session_id):
                session = start_pty_actor_with_runtime_resume(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["gemini", "--yolo"],
                    env={},
                    runtime="gemini",
                    max_backlog_bytes=1000,
                    runtime_start_preflight_error=lambda runtime, command, runner="pty": "",
                )

            self.assertIsInstance(session, FreshSession)
            self.assertEqual(
                calls,
                [
                    ["gemini", "--resume", "old-gemini-session-id", "--yolo"],
                    ["gemini", "--session-id", new_session_id, "--yolo"],
                ],
            )
            stored = read_runtime_session("g1", "peer1")
            self.assertEqual(stored.get("runtime"), "gemini")
            self.assertEqual(stored.get("provider_session_id"), new_session_id)
            self.assertEqual(stored.get("resume_command_hint"), f"gemini --resume {new_session_id}")
            self.assertEqual(stored.get("captured_from"), "gemini_generated_session_id")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
            self.assertEqual(str(stored.get("last_resume_error") or ""), "")
        finally:
            cleanup()

    def test_resume_quick_exit_falls_back_to_fresh_actor(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                read_runtime_session,
                record_pty_runtime_session,
                start_pty_actor_with_runtime_resume,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=["claude"],
                provider_session_id="old-session-id",
                captured_from="test",
            )

            class ResumeSession:
                pid = 111

            class FreshSession:
                pid = 222

            calls: list[list[str]] = []

            def fake_start_actor(**kwargs):
                calls.append(list(kwargs.get("command") or []))
                return ResumeSession() if len(calls) == 1 else FreshSession()

            new_session_id = "a3df810a-6a19-48e8-b75b-772d3ee65721"
            with patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.start_actor",
                side_effect=fake_start_actor,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.actor_running",
                return_value=False,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.tail_output",
                return_value=b"No conversation found for resume id",
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.stop_actor",
            ), patch("cccc.daemon.runtime_session_ops.uuid.uuid4", return_value=new_session_id):
                session = start_pty_actor_with_runtime_resume(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["claude"],
                    env={},
                    runtime="claude",
                    max_backlog_bytes=1000,
                    runtime_start_preflight_error=lambda runtime, command, runner="pty": "",
                )

            self.assertIsInstance(session, FreshSession)
            self.assertEqual(
                calls,
                [["claude", "--resume", "old-session-id"], ["claude", "--session-id", new_session_id]],
            )
            stored = read_runtime_session("g1", "peer1")
            self.assertEqual(stored.get("runtime"), "claude")
            self.assertEqual(stored.get("provider_session_id"), new_session_id)
            self.assertEqual(stored.get("captured_from"), "claude_generated_session_id")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
            self.assertEqual(str(stored.get("last_resume_error") or ""), "")
        finally:
            cleanup()

    def test_pty_resume_process_quick_exit_falls_back_to_fresh_actor(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                read_runtime_session,
                record_pty_runtime_session,
                start_pty_actor_with_runtime_resume,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="codex",
                cwd=cwd,
                command=["codex"],
                provider_session_id="019dbe1d-cd97-7d31-9ba6-212d3e57b15c",
                captured_from="codex_status_command",
            )

            class ResumeSession:
                pid = 111

            class FreshSession:
                pid = 222

            calls: list[list[str]] = []
            stopped: list[tuple[str, str]] = []
            scheduled: list[dict] = []

            def fake_start_actor(**kwargs):
                calls.append(list(kwargs.get("command") or []))
                return ResumeSession() if len(calls) == 1 else FreshSession()

            with patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.start_actor",
                side_effect=fake_start_actor,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.actor_running",
                return_value=False,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.tail_output",
                return_value=b"Error: thread not found",
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.stop_actor",
                side_effect=lambda **kwargs: stopped.append((kwargs.get("group_id"), kwargs.get("actor_id"))),
            ), patch(
                "cccc.daemon.runtime_session_ops._schedule_codex_pty_status_capture",
                side_effect=lambda **kwargs: scheduled.append(kwargs),
            ):
                session = start_pty_actor_with_runtime_resume(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["codex"],
                    env={},
                    runtime="codex",
                    max_backlog_bytes=1000,
                    runtime_start_preflight_error=lambda runtime, command, runner="pty": "",
                )

            self.assertIsInstance(session, FreshSession)
            self.assertEqual(calls, [["codex", "resume", "019dbe1d-cd97-7d31-9ba6-212d3e57b15c"], ["codex"]])
            self.assertEqual(stopped, [("g1", "peer1")])
            self.assertEqual(len(scheduled), 1)
            self.assertEqual(read_runtime_session("g1", "peer1"), {})
        finally:
            cleanup()

    def test_failed_pty_resume_metadata_is_skipped_on_next_start(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                mark_runtime_session_resume_failed,
                prepare_pty_resume_command,
                record_pty_runtime_session,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="gemini",
                cwd=cwd,
                command=["gemini", "--yolo"],
                provider_session_id="old-session-id",
                captured_from="gemini_generated_session_id",
            )
            command, resume_doc = prepare_pty_resume_command(
                group_id="g1",
                actor_id="peer1",
                runtime="gemini",
                cwd=cwd,
                base_command=["gemini", "--yolo"],
            )
            self.assertEqual(command, ["gemini", "--resume", "old-session-id", "--yolo"])
            self.assertIsNotNone(resume_doc)

            mark_runtime_session_resume_failed(group_id="g1", actor_id="peer1", error="resume id not found")
            command, resume_doc = prepare_pty_resume_command(
                group_id="g1",
                actor_id="peer1",
                runtime="gemini",
                cwd=cwd,
                base_command=["gemini", "--yolo"],
            )

            self.assertEqual(command, ["gemini", "--yolo"])
            self.assertIsNone(resume_doc)
        finally:
            cleanup()

    def test_pty_resume_success_does_not_wait_full_verify_window(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import record_pty_runtime_session, start_pty_actor_with_runtime_resume

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="codex",
                cwd=cwd,
                command=["codex"],
                provider_session_id="019dbe1d-cd97-7d31-9ba6-212d3e57b15c",
                captured_from="codex_status_command",
            )

            class ResumeSession:
                pid = 111

            calls: list[list[str]] = []
            monitors: list[dict] = []

            def fake_start_actor(**kwargs):
                calls.append(list(kwargs.get("command") or []))
                return ResumeSession()

            with patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.start_actor",
                side_effect=fake_start_actor,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.actor_running",
                return_value=True,
            ), patch(
                "cccc.daemon.runtime_session_ops.pty_runner.SUPERVISOR.tail_output",
                return_value=b"Restored session\n",
            ), patch(
                "cccc.daemon.runtime_session_ops._pty_resume_foreground_verify_seconds",
                return_value=0.0,
            ), patch(
                "cccc.daemon.runtime_session_ops._pty_resume_verify_seconds",
                return_value=20.0,
            ), patch(
                "cccc.daemon.runtime_session_ops._schedule_pty_resume_failure_monitor",
                side_effect=lambda **kwargs: monitors.append(kwargs),
            ):
                session = start_pty_actor_with_runtime_resume(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=["codex"],
                    env={},
                    runtime="codex",
                    max_backlog_bytes=1000,
                    runtime_start_preflight_error=lambda runtime, command, runner="pty": "",
                )

            self.assertIsInstance(session, ResumeSession)
            self.assertEqual(calls, [["codex", "resume", "019dbe1d-cd97-7d31-9ba6-212d3e57b15c"]])
            self.assertEqual(len(monitors), 1)
            self.assertEqual(monitors[0]["expected_pid"], 111)
            self.assertGreaterEqual(float(monitors[0]["timeout_seconds"]), 20.0)
        finally:
            cleanup()

    def test_runtime_session_provider_identity_selects_resume_path(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                mark_runtime_session_auth_failed,
                prepare_codex_app_thread_resume,
                prepare_headless_runtime_resume,
                prepare_pty_resume_command,
                read_runtime_session,
                record_codex_app_thread_runtime_session,
                record_pty_runtime_session,
                runtime_session_command_fingerprint,
                write_runtime_session,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_pty_runtime_session(
                group_id="g1",
                actor_id="codex-peer",
                runtime="codex",
                cwd=cwd,
                command=["codex"],
                provider_session_id="019dbe1d-cd97-7d31-9ba6-212d3e57b15c",
                captured_from="codex_status_command",
            )
            self.assertEqual(
                prepare_headless_runtime_resume(
                    group_id="g1",
                    actor_id="codex-peer",
                    runtime="codex",
                    cwd=cwd,
                    command=["codex", "app-server", "--listen", "stdio://"],
                ),
                {},
            )

            record_codex_app_thread_runtime_session(
                group_id="g1",
                actor_id="codex-peer",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "stdio://"],
                provider_thread_id="thread-123",
                runner="pty",
                captured_from="app_server_thread_start",
            )
            doc = prepare_codex_app_thread_resume(
                group_id="g1",
                actor_id="codex-peer",
                cwd=cwd,
                command=["codex", "app-server", "--listen", "stdio://"],
            )
            self.assertEqual(doc.get("provider_thread_id"), "thread-123")
            self.assertEqual(doc.get("runner"), "pty")
            command, resume_doc = prepare_pty_resume_command(
                group_id="g1",
                actor_id="codex-peer",
                runtime="codex",
                cwd=cwd,
                base_command=["codex"],
            )
            self.assertEqual(command, ["codex"])
            self.assertIsNone(resume_doc)

            mark_runtime_session_auth_failed(
                group_id="g1",
                actor_id="codex-peer",
                error="failed to connect to websocket: HTTP error: 401 Unauthorized",
            )
            stored = read_runtime_session("g1", "codex-peer")
            self.assertEqual(stored.get("status"), "auth_failed")
            self.assertFalse(bool(stored.get("resume_eligible")))
            self.assertEqual(stored.get("failure_count"), 1)
            self.assertIn("401 Unauthorized", str(stored.get("last_resume_error") or ""))
            self.assertEqual(
                prepare_codex_app_thread_resume(
                    group_id="g1",
                    actor_id="codex-peer",
                    cwd=cwd,
                    command=["codex", "app-server", "--listen", "stdio://"],
                ),
                {},
            )

            write_runtime_session(
                "g1",
                "dirty-codex-peer",
                {
                    "runtime": "codex",
                    "runner": "pty",
                    "workspace_path": str(cwd.resolve()),
                    "command_fingerprint": runtime_session_command_fingerprint(
                        ["codex", "app-server", "--listen", "stdio://"]
                    ),
                    "model": "",
                    "provider_session_id": "019dbe1d-cd97-7d31-9ba6-212d3e57b15c",
                    "provider_thread_id": "thread-123",
                    "status": "usable",
                    "resume_eligible": True,
                },
            )
            self.assertEqual(
                prepare_codex_app_thread_resume(
                    group_id="g1",
                    actor_id="dirty-codex-peer",
                    cwd=cwd,
                    command=["codex", "app-server", "--listen", "stdio://"],
                ),
                {},
            )
            command, resume_doc = prepare_pty_resume_command(
                group_id="g1",
                actor_id="dirty-codex-peer",
                runtime="codex",
                cwd=cwd,
                base_command=["codex"],
            )
            self.assertEqual(command, ["codex"])
            self.assertIsNone(resume_doc)
        finally:
            cleanup()

    def test_headless_resume_metadata_is_usable_when_provider_supports_resume(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                prepare_headless_runtime_resume,
                read_runtime_session,
                record_headless_runtime_session,
            )

            cwd = home / "repo"
            cwd.mkdir()
            record_headless_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=["claude", "-p", "--input-format", "stream-json"],
                provider_session_id="42e9ef0c-3b75-43a0-9056-eef13dd1061d",
                status="usable",
                captured_from="stream_json_init",
                resume_eligible=True,
            )
            doc = prepare_headless_runtime_resume(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=["claude", "-p", "--input-format", "stream-json"],
            )
            self.assertEqual(doc.get("provider_session_id"), "42e9ef0c-3b75-43a0-9056-eef13dd1061d")
            stored = read_runtime_session("g1", "peer1")
            self.assertEqual(stored.get("runner"), "headless")
            self.assertEqual(stored.get("status"), "usable")
            self.assertTrue(bool(stored.get("resume_eligible")))
            self.assertTrue(str(stored.get("last_resume_attempt_at") or ""))
        finally:
            cleanup()

    def test_claude_headless_launch_uses_explicit_session_id_or_resume(self) -> None:
        home, cleanup = self._with_home()
        try:
            from cccc.daemon.runtime_session_ops import (
                prepare_claude_headless_launch_command,
                record_headless_runtime_session,
            )

            cwd = home / "repo"
            cwd.mkdir()
            base = ["claude", "-p", "--input-format", "stream-json"]
            with patch("cccc.daemon.runtime_session_ops.uuid.uuid4", return_value="42e9ef0c-3b75-43a0-9056-eef13dd1061d"):
                command, doc, kind = prepare_claude_headless_launch_command(
                    group_id="g1",
                    actor_id="peer1",
                    cwd=cwd,
                    base_command=base,
                )
            self.assertEqual(kind, "fresh")
            self.assertEqual(
                command,
                ["claude", "--session-id", "42e9ef0c-3b75-43a0-9056-eef13dd1061d", "-p", "--input-format", "stream-json"],
            )
            self.assertEqual((doc or {}).get("provider_session_id"), "42e9ef0c-3b75-43a0-9056-eef13dd1061d")

            record_headless_runtime_session(
                group_id="g1",
                actor_id="peer1",
                runtime="claude",
                cwd=cwd,
                command=base,
                provider_session_id="434d41d0-02e9-4760-9f13-155d04cde834",
                status="usable",
                captured_from="stream_json_init",
                resume_eligible=True,
            )
            command, doc, kind = prepare_claude_headless_launch_command(
                group_id="g1",
                actor_id="peer1",
                cwd=cwd,
                base_command=base,
            )
            self.assertEqual(kind, "resume")
            self.assertEqual(
                command,
                ["claude", "--resume", "434d41d0-02e9-4760-9f13-155d04cde834", "-p", "--input-format", "stream-json"],
            )
            self.assertEqual((doc or {}).get("provider_session_id"), "434d41d0-02e9-4760-9f13-155d04cde834")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
