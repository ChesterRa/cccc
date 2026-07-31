from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebGroupCreateWithScopeFailures(unittest.TestCase):
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

    def test_ledger_failure_restores_active_without_global_group_events(self) -> None:
        from cccc.kernel.active import load_active, set_active_group_id

        previous = self._call_daemon(
            {"op": "group_create", "args": {"title": "previous", "by": "user"}}
        )
        previous_id = previous["result"]["group_id"]
        set_active_group_id(previous_id)
        failed_id = "g_failed_saga"
        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                patch("cccc.kernel.group._random_group_id", return_value=failed_id),
                patch(
                    "cccc.daemon.group.group_creation_ops.append_event",
                    side_effect=OSError("ledger failed"),
                ),
                self._client() as client,
            ):
                response = client.post(
                    "/api/v1/groups",
                    json={"title": "failed", "path": project, "by": "user"},
                )

        self.assertFalse(response.json().get("ok"), response.text)
        self.assertEqual(load_active()["active_group_id"], previous_id)
        events_path = Path(self._home.name) / "daemon" / "ccccd.events.jsonl"
        events = [
            json.loads(line)
            for line in events_path.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        leaked = [
            event
            for event in events
            if event["data"].get("group_id") == failed_id
            and event["kind"] in {"group.created", "group.deleted"}
        ]
        self.assertEqual(leaked, [])

    def test_active_failure_restores_previous_active_and_removes_group(self) -> None:
        from cccc.kernel.active import load_active, set_active_group_id
        from cccc.kernel.registry import load_registry

        previous = self._call_daemon(
            {"op": "group_create", "args": {"title": "previous", "by": "user"}}
        )
        previous_id = previous["result"]["group_id"]
        set_active_group_id(previous_id)
        real_set_active = set_active_group_id

        def fail_new_active(group_id: str):
            if group_id != previous_id:
                raise OSError("active failed")
            return real_set_active(group_id)

        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                patch(
                    "cccc.kernel.active.set_active_group_id",
                    side_effect=fail_new_active,
                ),
                self._client() as client,
            ):
                response = client.post(
                    "/api/v1/groups",
                    json={"title": "failed", "path": project, "by": "user"},
                )

        self.assertFalse(response.json().get("ok"), response.text)
        self.assertEqual(load_active()["active_group_id"], previous_id)
        self.assertEqual(set(load_registry().groups), {previous_id})

    def test_active_post_persist_error_is_treated_as_committed(self) -> None:
        from cccc.kernel import active as active_module
        from cccc.kernel.active import load_active

        real_write = active_module.atomic_write_json

        def persist_then_error(path: Path, document: dict, *, indent: int = 2) -> None:
            real_write(path, document, indent=indent)
            raise OSError("trailing active sync error")

        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                patch(
                    "cccc.kernel.active.atomic_write_json",
                    side_effect=persist_then_error,
                ),
                self._client() as client,
            ):
                response = client.post(
                    "/api/v1/groups",
                    json={"title": "committed", "path": project, "by": "user"},
                )

        body = response.json()
        self.assertTrue(body.get("ok"), body)
        self.assertEqual(
            load_active()["active_group_id"],
            (body.get("result") or {}).get("group_id"),
        )

    def test_rollback_failure_is_explicit(self) -> None:
        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                patch(
                    "cccc.daemon.group.group_creation_ops.append_event",
                    side_effect=OSError("ledger failed"),
                ),
                patch(
                    "cccc.daemon.group.group_creation_ops.delete_group",
                    side_effect=OSError("delete failed"),
                ),
                self._client() as client,
            ):
                response = client.post(
                    "/api/v1/groups",
                    json={"title": "failed", "path": project, "by": "user"},
                )

        error = response.json().get("error") or {}
        self.assertEqual(error.get("code"), "rollback_failed")
        self.assertIn("delete failed", error.get("message") or "")

    def test_registry_post_persist_error_is_treated_as_committed(self) -> None:
        from cccc.kernel import registry as registry_module
        from cccc.kernel.registry import load_registry

        load_registry()
        real_write = registry_module.atomic_write_json

        def persist_then_error(path: Path, document: dict, *, indent: int = 2) -> None:
            real_write(path, document, indent=indent)
            raise OSError("trailing sync error")

        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                patch(
                    "cccc.kernel.registry.atomic_write_json",
                    side_effect=persist_then_error,
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
