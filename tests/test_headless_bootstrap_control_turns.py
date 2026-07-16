import io
import unittest
from pathlib import Path
from unittest.mock import Mock, patch


class _FakeProc:
    def __init__(self) -> None:
        self.stdin = Mock()
        self.stdout = io.StringIO("")
        self.stderr = io.StringIO("")
        self.pid = 1234

    def poll(self):
        return None

    def terminate(self) -> None:
        return None

    def wait(self, timeout: float | None = None) -> int:
        return 0

    def kill(self) -> None:
        return None


def _thread_with_order(order: list[str], label: str) -> Mock:
    thread = Mock()
    thread.start.side_effect = lambda: order.append(label)
    return thread


class TestHeadlessBootstrapControlTurns(unittest.TestCase):
    def test_claude_start_queues_bootstrap_before_turn_thread(self) -> None:
        from cccc.daemon.claude_app_sessions import ClaudeAppSession

        proc = _FakeProc()
        order: list[str] = []
        session = ClaudeAppSession(group_id="g_test", actor_id="peer1", cwd=Path("."), env={})
        threads = [
            _thread_with_order(order, "stdout"),
            _thread_with_order(order, "stderr"),
            _thread_with_order(order, "turn"),
        ]

        with (
            patch("cccc.daemon.claude_app_sessions.subprocess.Popen", return_value=proc) as popen,
            patch("cccc.daemon.claude_app_sessions.ensure_mcp_installed", return_value=True) as ensure_mcp,
            patch("cccc.daemon.claude_app_sessions.threading.Thread", side_effect=threads),
            patch("cccc.daemon.claude_app_sessions.time.sleep", return_value=None),
            patch.object(session, "_persist_state"),
            patch.object(session, "_queue_bootstrap_control_turn", side_effect=lambda: order.append("bootstrap") or True),
        ):
            session.start()

        self.assertEqual(order, ["stdout", "stderr", "bootstrap", "turn"])
        self.assertIn("--include-hook-events", popen.call_args.args[0])
        ensure_mcp.assert_called_once()
        popen_env = popen.call_args.kwargs.get("env") or {}
        self.assertEqual(popen_env.get("CCCC_GROUP_ID"), "g_test")
        self.assertEqual(popen_env.get("CCCC_ACTOR_ID"), "peer1")

    def test_codex_start_queues_bootstrap_before_turn_thread(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        proc = _FakeProc()
        order: list[str] = []
        session = CodexAppSession(group_id="g_test", actor_id="peer1", cwd=Path("."), env={})
        threads = [
            _thread_with_order(order, "stdout"),
            _thread_with_order(order, "stderr"),
            _thread_with_order(order, "turn"),
        ]

        with (
            patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=proc) as popen,
            patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True) as ensure_mcp,
            patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=threads),
            patch.object(session, "_persist_state"),
            patch.object(session, "_emit"),
            patch.object(
                session,
                "_request",
                side_effect=[{}, {"thread": {"id": "thread-1"}}],
            ) as request,
            patch.object(session, "_queue_bootstrap_control_turn", side_effect=lambda: order.append("bootstrap") or True),
        ):
            session.start()

        self.assertEqual(order, ["stdout", "stderr", "bootstrap", "turn"])
        thread_start_params = request.call_args_list[1].args[1]
        self.assertNotIn("model", thread_start_params)
        ensure_mcp.assert_called_once()
        popen_env = popen.call_args.kwargs.get("env") or {}
        self.assertEqual(popen_env.get("CCCC_GROUP_ID"), "g_test")
        self.assertEqual(popen_env.get("CCCC_ACTOR_ID"), "peer1")

    def test_codex_start_passes_explicit_model_to_thread_start(self) -> None:
        from cccc.daemon.codex_app_sessions import CodexAppSession

        proc = _FakeProc()
        session = CodexAppSession(group_id="g_test", actor_id="peer1", cwd=Path("."), env={}, model="gpt-5.3-codex-spark")
        threads = [
            _thread_with_order([], "stdout"),
            _thread_with_order([], "stderr"),
            _thread_with_order([], "turn"),
        ]

        with (
            patch("cccc.daemon.codex_app_sessions.subprocess.Popen", return_value=proc),
            patch("cccc.daemon.codex_app_sessions.ensure_mcp_installed", return_value=True),
            patch("cccc.daemon.codex_app_sessions.threading.Thread", side_effect=threads),
            patch.object(session, "_persist_state"),
            patch.object(session, "_emit"),
            patch.object(
                session,
                "_request",
                side_effect=[{}, {"thread": {"id": "thread-1"}}],
            ) as request,
            patch.object(session, "_queue_bootstrap_control_turn", return_value=True),
        ):
            session.start()

        thread_start_params = request.call_args_list[1].args[1]
        self.assertEqual(thread_start_params.get("model"), "gpt-5.3-codex-spark")


if __name__ == "__main__":
    unittest.main()
