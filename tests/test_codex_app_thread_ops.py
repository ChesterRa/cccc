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
                provider_thread_id="thr_existing",
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

            thread_id, resumed = start_codex_app_thread(
                request=request,
                group_id="g_codex_resume",
                actor_id="peer1",
                cwd=cwd,
                command=command,
                model="",
            )

            self.assertEqual(thread_id, "thr_existing")
            self.assertTrue(resumed)
            self.assertEqual([call[0] for call in calls], ["thread/resume"])
            self.assertNotIn("cwd", calls[0][1])
            stored = read_runtime_session("g_codex_resume", "peer1")
            self.assertEqual(stored.get("captured_from"), "app_server_thread_resume")
        finally:
            cleanup()
