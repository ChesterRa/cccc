from __future__ import annotations

import threading
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch


class TestWindowsSupportDiagnostics(unittest.TestCase):
    def test_platform_support_reports_missing_pywinpty(self) -> None:
        from cccc.runners import platform_support

        missing = ModuleNotFoundError("No module named 'winpty'")
        missing.name = "winpty"
        with patch.object(platform_support.os, "name", "nt"), patch.object(
            platform_support.importlib,
            "import_module",
            side_effect=missing,
        ):
            details = platform_support.pty_support_details()

        self.assertFalse(bool(details.get("supported")))
        self.assertEqual(str(details.get("code") or ""), "pywinpty_missing")
        self.assertIn("pywinpty", str(details.get("message") or ""))
        hints = details.get("hints") if isinstance(details.get("hints"), list) else []
        self.assertTrue(any("pip install pywinpty" in str(item) for item in hints))

    def test_platform_support_matches_real_import_path(self) -> None:
        from cccc.runners import platform_support

        with patch.object(platform_support.os, "name", "nt"), patch.object(
            platform_support.importlib,
            "import_module",
            return_value=SimpleNamespace(PtyProcess=object()),
        ):
            details = platform_support.pty_support_details()
            pty_process = platform_support.load_winpty_process_class()

        self.assertTrue(bool(details.get("supported")))
        self.assertEqual(str(details.get("code") or ""), "")
        self.assertIsNotNone(pty_process)

    def test_platform_support_reports_import_failure(self) -> None:
        from cccc.runners import platform_support

        with patch.object(platform_support.os, "name", "nt"), patch.object(
            platform_support.importlib,
            "import_module",
            side_effect=RuntimeError("native import failed"),
        ):
            details = platform_support.pty_support_details()

        self.assertFalse(bool(details.get("supported")))
        self.assertEqual(str(details.get("code") or ""), "winpty_import_failed")
        self.assertIn("native import failed", str(details.get("message") or ""))

    def test_actor_runtime_returns_explicit_windows_pty_error(self) -> None:
        from cccc.daemon.actors import actor_runtime_ops

        group = SimpleNamespace(
            group_id="g1",
            doc={"active_scope_key": "scope1"},
            save=lambda: None,
            ledger_path="ledger.jsonl",
        )
        actor = {
            "id": "peer1",
            "default_scope_key": "scope1",
            "runner": "pty",
            "runtime": "codex",
            "command": ["codex"],
            "env": {},
        }

        with patch.object(actor_runtime_ops, "find_actor", return_value=actor), patch.object(
            actor_runtime_ops.pty_runner,
            "PTY_SUPPORTED",
            False,
            create=False,
        ), patch.object(
            actor_runtime_ops,
            "pty_support_error_message",
            return_value="Windows PTY backend unavailable: install pywinpty to enable ConPTY actors.",
        ):
            result = actor_runtime_ops.start_actor_process(
                group,
                "peer1",
                command=["codex"],
                env={},
                runner="pty",
                runtime="codex",
                by="user",
                find_scope_url=lambda _group, _scope_key: ".",
                effective_runner_kind=lambda runner: runner,
                merge_actor_env_with_private=lambda _gid, _aid, env: dict(env),
                normalize_runtime_command=lambda _runtime, command: list(command),
                ensure_mcp_installed=lambda _runtime, _cwd, **_kwargs: True,
                inject_actor_context_env=lambda env, _gid, _aid: dict(env),
                prepare_pty_env=lambda env: dict(env),
                pty_backlog_bytes=lambda: 1024,
                write_headless_state=lambda _gid, _aid: None,
                write_pty_state=lambda _gid, _aid, _pid: None,
                clear_preamble_sent=lambda _group, _aid: None,
                throttle_reset_actor=lambda _gid, _aid: None,
                supported_runtimes=("codex",),
            )

        self.assertFalse(bool(result.get("success")))
        self.assertIn("pywinpty", str(result.get("error") or ""))

    def test_actor_runtime_returns_explicit_runtime_unavailable_error(self) -> None:
        from cccc.daemon.actors import actor_runtime_ops

        group = SimpleNamespace(
            group_id="g1",
            doc={"active_scope_key": "scope1"},
            save=lambda: None,
            ledger_path="ledger.jsonl",
        )
        actor = {
            "id": "peer1",
            "default_scope_key": "scope1",
            "runner": "pty",
            "runtime": "codex",
            "command": ["codex"],
            "env": {},
        }

        with patch.object(actor_runtime_ops, "find_actor", return_value=actor), patch.object(
            actor_runtime_ops,
            "runtime_start_preflight_error",
            return_value="runtime unavailable: Codex CLI is not installed or not in PATH",
        ), patch.object(actor_runtime_ops.pty_runner, "PTY_SUPPORTED", True, create=False):
            result = actor_runtime_ops.start_actor_process(
                group,
                "peer1",
                command=["codex"],
                env={},
                runner="pty",
                runtime="codex",
                by="user",
                find_scope_url=lambda _group, _scope_key: ".",
                effective_runner_kind=lambda runner: runner,
                merge_actor_env_with_private=lambda _gid, _aid, env: dict(env),
                normalize_runtime_command=lambda _runtime, command: list(command),
                ensure_mcp_installed=lambda _runtime, _cwd, **_kwargs: True,
                inject_actor_context_env=lambda env, _gid, _aid: dict(env),
                prepare_pty_env=lambda env: dict(env),
                pty_backlog_bytes=lambda: 1024,
                write_headless_state=lambda _gid, _aid: None,
                write_pty_state=lambda _gid, _aid, _pid: None,
                clear_preamble_sent=lambda _group, _aid: None,
                throttle_reset_actor=lambda _gid, _aid: None,
                supported_runtimes=("codex",),
            )

        self.assertFalse(bool(result.get("success")))
        self.assertIn("not in PATH", str(result.get("error") or ""))

    def test_windows_pty_does_not_fallback_to_spawn_without_env(self) -> None:
        from cccc.runners import pty_win

        with tempfile.TemporaryDirectory() as td:
            spawn_calls: list[dict[str, object]] = []

            def _spawn(_cmdline: str, **kwargs: object) -> object:
                spawn_calls.append(dict(kwargs))
                raise TypeError("spawn signature mismatch")

            fake_proc = SimpleNamespace(spawn=_spawn)
            with patch.object(pty_win, "PTY_SUPPORTED", True), patch.object(pty_win, "_WINPTY_PROCESS", fake_proc):
                with self.assertRaisesRegex(RuntimeError, "environment forwarding"):
                    pty_win.PtySession(
                        group_id="g1",
                        actor_id="peer1",
                        cwd=Path(td),
                        command=["codex"],
                        env={"CCCC_HOME": td, "CCCC_GROUP_ID": "g1", "CCCC_ACTOR_ID": "peer1"},
                    )

            self.assertEqual(len(spawn_calls), 2)
            self.assertTrue(all("env" in call for call in spawn_calls))

    def test_windows_pty_stop_uses_tree_termination(self) -> None:
        from cccc.runners import pty_win

        session = object.__new__(pty_win.PtySession)
        session._running = True
        session._proc = SimpleNamespace(
            pid=4321,
            isalive=lambda: False,
            exitstatus=0,
            terminate=lambda *args, **kwargs: None,
            kill=lambda *args, **kwargs: None,
            close=lambda *args, **kwargs: None,
        )
        session._notify_wake = lambda: None
        session._thread = SimpleNamespace(is_alive=lambda: False, join=lambda timeout=None: None)
        session._reader_thread = SimpleNamespace(is_alive=lambda: False, join=lambda timeout=None: None)

        with patch.object(pty_win, "terminate_pid", return_value=True) as mock_terminate:
            session.stop()

        mock_terminate.assert_called_once_with(4321, timeout_s=1.0, include_group=True, force=True)

    def test_windows_pty_replies_to_gemini_device_attributes_when_writer_attached(self) -> None:
        from cccc.runners import pty_win

        session = object.__new__(pty_win.PtySession)
        session._query_tail = b""
        session._writer_fd = object()
        session._runtime = "gemini"
        session._lock = threading.RLock()
        writes: list[bytes] = []
        session.write_input = lambda data: writes.append(bytes(data)) or True

        session._maybe_reply_to_terminal_queries(b"\x1b[c")

        self.assertEqual(writes, [b"\x1b[?1;2c"])

        session._query_tail = b""
        session._runtime = "codex"
        writes.clear()

        session._maybe_reply_to_terminal_queries(b"\x1b[c")

        self.assertEqual(writes, [])

    def test_windows_pty_does_not_replay_complete_terminal_query_tail(self) -> None:
        from cccc.runners import pty_win

        session = object.__new__(pty_win.PtySession)
        session._query_tail = b""
        session._writer_fd = object()
        session._runtime = "gemini"
        session._lock = threading.RLock()
        writes: list[bytes] = []
        session.write_input = lambda data: writes.append(bytes(data)) or True

        session._maybe_reply_to_terminal_queries(b"\x1b[c")
        session._maybe_reply_to_terminal_queries(b"Gemini ready")

        self.assertEqual(writes, [b"\x1b[?1;2c"])

        session._query_tail = b""
        writes.clear()

        session._maybe_reply_to_terminal_queries(b"\x1b[")
        session._maybe_reply_to_terminal_queries(b"c")

        self.assertEqual(writes, [b"\x1b[?1;2c"])

    def test_codex_windows_command_still_gets_env_inherit_flag(self) -> None:
        from cccc.daemon import server as daemon_server

        cmd = daemon_server._normalize_runtime_command("codex", [r"C:\Tools\codex.cmd", "--search"])

        self.assertEqual(cmd[0], r"C:\Tools\codex.cmd")
        self.assertEqual(cmd[1:3], ["-c", "shell_environment_policy.inherit=all"])
        self.assertEqual(cmd[3:], ["--search"])


if __name__ == "__main__":
    unittest.main()
