from __future__ import annotations

import os
import tempfile
import unittest
from unittest.mock import patch


class TestCodexNewSessionLifecycle(unittest.TestCase):
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

    def test_restart_fresh_session_starts_codex_app_server_without_resuming_session(self) -> None:
        home, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "fresh-session", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)
            attach, _ = self._call("attach", {"group_id": group_id, "path": home, "by": "user"})
            self.assertTrue(attach.ok, getattr(attach, "error", None))
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

            class _FakeCodexAppSession:
                def remote_tui_pid(self) -> int:
                    return 12345

            with patch("cccc.daemon.server._ensure_mcp_installed", return_value=True), patch(
                "cccc.daemon.actors.actor_lifecycle_ops.codex_app_supervisor.start_pty_app_actor",
                return_value=_FakeCodexAppSession(),
            ) as start_runtime:
                restart, _ = self._call(
                    "actor_restart",
                    {"group_id": group_id, "actor_id": "peer1", "by": "user", "fresh_session": True},
                )

            self.assertTrue(restart.ok, getattr(restart, "error", None))
            start_runtime.assert_called_once()
            self.assertTrue(bool(start_runtime.call_args.kwargs.get("fresh_session")))
            event = (restart.result or {}).get("event") if isinstance(restart.result, dict) else {}
            self.assertEqual(str(event.get("kind") or ""), "actor.restart")
            self.assertTrue(bool((event.get("data") or {}).get("fresh_session")))
            self.assertNotIn("clear" + "_session", event.get("data") or {})
        finally:
            cleanup()
