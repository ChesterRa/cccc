from __future__ import annotations

import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebGroupCreateWithScopeRollback(unittest.TestCase):
    def setUp(self) -> None:
        self._old_home = os.environ.get("CCCC_HOME")
        self._home = tempfile.TemporaryDirectory()
        os.environ["CCCC_HOME"] = self._home.name

    def tearDown(self) -> None:
        self._home.cleanup()
        if self._old_home is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = self._old_home

    @staticmethod
    def _call_daemon(request: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        response, _ = handle_request(DaemonRequest.model_validate(request))
        return response.model_dump(exclude_none=True)

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def test_active_restore_failure_is_explicit(self) -> None:
        from cccc.kernel.active import set_active_group_id

        previous = self._call_daemon(
            {"op": "group_create", "args": {"title": "previous", "by": "user"}}
        )
        set_active_group_id(previous["result"]["group_id"])
        calls = 0

        def fail_active(_group_id: str):
            nonlocal calls
            calls += 1
            raise OSError(f"active failure {calls}")

        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                patch(
                    "cccc.kernel.active.set_active_group_id", side_effect=fail_active
                ),
                self._client() as client,
            ):
                response = client.post(
                    "/api/v1/groups",
                    json={"title": "failed", "path": project, "by": "user"},
                )

        error = response.json().get("error") or {}
        self.assertEqual(error.get("code"), "rollback_failed")
        self.assertIn("active failure 2", error.get("message") or "")

    def test_publish_failure_does_not_roll_back_committed_group(self) -> None:
        from cccc.kernel.active import load_active
        from cccc.kernel.registry import load_registry

        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                patch(
                    "cccc.kernel.events.publish_event",
                    side_effect=OSError("publish failed"),
                ),
                patch(
                    "cccc.daemon.group.group_creation_ops.notify_appended_event",
                    side_effect=OSError("notify failed"),
                ),
                self._client() as client,
            ):
                response = client.post(
                    "/api/v1/groups",
                    json={"title": "committed", "path": project, "by": "user"},
                )

        body = response.json()
        self.assertTrue(body.get("ok"), body)
        group_id = (body.get("result") or {}).get("group_id")
        self.assertIn(group_id, load_registry().groups)
        self.assertEqual(load_active()["active_group_id"], group_id)
