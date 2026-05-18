from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path


class TestCodexAppThreadOps(unittest.TestCase):
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

    def test_existing_codex_thread_resumes(self) -> None:
        from cccc.daemon.codex_app_thread_ops import start_codex_app_thread
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_codex_app_thread_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            command = ["codex", "app-server", "--listen", "stdio://"]
            record_codex_app_thread_runtime_session(
                group_id="g_codex_resume",
                actor_id="peer1",
                cwd=cwd,
                command=command,
                provider_thread_id="thr_existing",
                runner="pty",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            calls: list[tuple[str, dict]] = []

            def request(method: str, params: dict, *, timeout: float):
                calls.append((method, dict(params or {})))
                if method == "thread/resume":
                    return {"thread": {"id": "thr_existing"}}
                raise AssertionError(f"unexpected request: {method}")

            result = start_codex_app_thread(
                request=request,
                group_id="g_codex_resume",
                actor_id="peer1",
                cwd=cwd,
                command=command,
                model="",
                runner="pty",
            )

            self.assertEqual(result.thread_id, "thr_existing")
            self.assertTrue(result.resumed)
            self.assertEqual([call[0] for call in calls], ["thread/resume"])
            self.assertNotIn("cwd", calls[0][1])
            stored = read_runtime_session("g_codex_resume", "peer1")
            self.assertEqual(stored.get("captured_from"), "app_server_thread_resume")
            self.assertEqual(stored.get("runner"), "pty")
        finally:
            cleanup()

    def test_failed_codex_thread_resume_is_skipped_on_next_start(self) -> None:
        from cccc.daemon.codex_app_thread_ops import start_codex_app_thread
        from cccc.daemon.runtime_session_ops import read_runtime_session, record_codex_app_thread_runtime_session

        home, cleanup = self._with_home()
        try:
            cwd = Path(home)
            command = ["codex", "app-server", "--listen", "stdio://"]
            record_codex_app_thread_runtime_session(
                group_id="g_codex_resume",
                actor_id="peer1",
                cwd=cwd,
                command=command,
                provider_thread_id="thr_stale",
                runner="pty",
                status="usable",
                captured_from="app_server_thread_start",
                resume_eligible=True,
            )
            calls: list[str] = []

            def failing_request(method: str, params: dict, *, timeout: float):
                calls.append(method)
                if method == "thread/resume":
                    raise RuntimeError("thread not found")
                raise AssertionError(f"unexpected request: {method}")

            with self.assertRaises(RuntimeError):
                start_codex_app_thread(
                    request=failing_request,
                    group_id="g_codex_resume",
                    actor_id="peer1",
                    cwd=cwd,
                    command=command,
                    runner="pty",
                )

            self.assertEqual(calls, ["thread/resume"])
            failed = read_runtime_session("g_codex_resume", "peer1")
            self.assertEqual(failed.get("provider_thread_id"), "thr_stale")
            self.assertEqual(failed.get("status"), "resume_failed")
            self.assertFalse(bool(failed.get("resume_eligible")))

            calls.clear()

            def fresh_request(method: str, params: dict, *, timeout: float):
                calls.append(method)
                if method == "thread/start":
                    return {"thread": {"id": "thr_fresh"}}
                raise AssertionError(f"unexpected request: {method}")

            result = start_codex_app_thread(
                request=fresh_request,
                group_id="g_codex_resume",
                actor_id="peer1",
                cwd=cwd,
                command=command,
                runner="pty",
            )

            self.assertEqual(calls, ["thread/start"])
            self.assertFalse(result.resumed)
            self.assertEqual(result.thread_id, "thr_fresh")
            fresh = read_runtime_session("g_codex_resume", "peer1")
            self.assertEqual(fresh.get("provider_thread_id"), "thr_fresh")
            self.assertEqual(fresh.get("status"), "usable")
        finally:
            cleanup()
