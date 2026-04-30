import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebPetContextRoute(unittest.TestCase):
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

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        return create_group(reg, title="pet-context-local", topic="local").group_id

    def test_pet_context_reads_local_summary_snapshot_without_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            local_summary = {
                "version": "ctxv:1",
                "coordination": {"brief": {"current_focus": "Focus"}, "tasks": []},
                "agent_states": [],
                "actors_runtime": [],
                "attention": {},
                "tasks_summary": {},
                "meta": {"summary_snapshot": {"state": "hit"}},
            }

            with patch(
                "cccc.ports.web.routes.groups._get_summary_context_fast",
                return_value=local_summary,
            ) as summary_fast, patch(
                "cccc.ports.web.routes.groups.load_pet_signals",
                return_value={
                    "signals": [],
                    "reply_pressure": {},
                    "coordination_rhythm": {},
                    "task_pressure": {},
                    "proposal_ready": {},
                },
            ) as load_signals, patch(
                "cccc.ports.web.app.call_daemon",
                side_effect=AssertionError("pet-context should not call daemon"),
            ):
                with self._client() as client:
                    resp = client.get(f"/api/v1/groups/{group_id}/pet-context")

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(bool(body.get("ok")), body)
            self.assertIn("snapshot", body.get("result") or {})
            summary_fast.assert_called_once()
            _, kwargs = load_signals.call_args
            self.assertEqual(kwargs.get("recent_chat_limit"), 10)
            self.assertEqual(kwargs.get("recent_chat_source"), "active_tail")
            self.assertEqual(kwargs.get("context_sync_limit"), 0)
            self.assertIs(kwargs.get("include_reply_obligation_status"), False)
        finally:
            cleanup()

    def test_pet_context_fresh_rebuilds_summary_snapshot(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            fresh_summary = {
                "version": "ctxv:fresh",
                "coordination": {"brief": {"current_focus": "Fresh"}, "tasks": []},
                "agent_states": [],
                "actors_runtime": [],
                "attention": {},
                "tasks_summary": {},
                "meta": {"summary_snapshot": {"state": "hit"}},
            }

            with patch(
                "cccc.ports.web.routes.groups._rebuild_summary_snapshot",
                return_value=True,
            ) as rebuild, patch(
                "cccc.ports.web.routes.groups.ContextStorage.load_summary_snapshot",
                return_value={"result": fresh_summary},
            ) as load_summary, patch(
                "cccc.ports.web.routes.groups.load_pet_signals",
                return_value={
                    "signals": [],
                    "reply_pressure": {},
                    "coordination_rhythm": {},
                    "task_pressure": {},
                    "proposal_ready": {},
                },
            ) as load_signals:
                with self._client() as client:
                    resp = client.get(f"/api/v1/groups/{group_id}/pet-context?fresh=1")

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(bool(body.get("ok")), body)
            self.assertIn("snapshot", body.get("result") or {})
            rebuild.assert_called_once_with(group_id)
            load_summary.assert_called_once()
            _, kwargs = load_signals.call_args
            self.assertEqual(kwargs.get("recent_chat_limit"), 50)
            self.assertEqual(kwargs.get("recent_chat_source"), "active_tail")
            self.assertEqual(kwargs.get("context_sync_limit"), 0)
            self.assertIs(kwargs.get("include_reply_obligation_status"), False)
        finally:
            cleanup()
