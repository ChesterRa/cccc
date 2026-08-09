from __future__ import annotations

import os
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebGroupCreateWithScope(unittest.TestCase):
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

    def test_single_create_request_attaches_scope_and_exposes_group_id(self) -> None:
        from cccc.kernel.active import load_active
        from cccc.kernel.group import load_group

        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                self._client() as client,
            ):
                response = client.post(
                    "/api/v1/groups",
                    json={
                        "title": "scoped",
                        "topic": "",
                        "path": project,
                        "by": "user",
                    },
                )

        body = response.json()
        self.assertTrue(body.get("ok"), body)
        group_id = str((body.get("result") or {}).get("group_id") or "")
        self.assertTrue(group_id)
        group = load_group(group_id)
        self.assertIsNotNone(group)
        assert group is not None
        self.assertEqual(group.doc["scopes"][0]["url"], str(Path(project).resolve()))
        self.assertEqual(
            (body.get("result") or {}).get("group", {}).get("group_id"), group_id
        )
        self.assertEqual(load_active()["active_group_id"], group_id)
        events_path = Path(self._home.name) / "daemon" / "ccccd.events.jsonl"
        events = [
            json.loads(line)
            for line in events_path.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]
        created = [
            event
            for event in events
            if event["kind"] == "group.created"
            and event["data"].get("group_id") == group_id
        ]
        self.assertEqual(len(created), 1)

    def test_attach_failure_removes_created_group_and_registry_entry(self) -> None:
        from cccc.kernel.registry import load_registry

        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                patch(
                    "cccc.daemon.group.group_creation_ops.attach_scope_to_group",
                    side_effect=OSError("attach failed"),
                ),
                self._client() as client,
            ):
                response = client.post(
                    "/api/v1/groups",
                    json={"title": "rollback", "path": project, "by": "user"},
                )

        body = response.json()
        self.assertFalse(body.get("ok"), body)
        registry = load_registry()
        self.assertEqual(registry.groups, {})
        self.assertEqual(list((Path(self._home.name) / "groups").glob("g_*")), [])

    def test_failed_second_creation_restores_previous_scope_default(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.registry import load_registry

        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                self._client() as client,
            ):
                first = client.post(
                    "/api/v1/groups",
                    json={"title": "first", "path": project, "by": "user"},
                ).json()
                first_id = str((first.get("result") or {}).get("group_id") or "")
                self.assertTrue(first.get("ok"), first)
                first_group = load_group(first_id)
                self.assertIsNotNone(first_group)
                assert first_group is not None
                scope_key = str(first_group.doc["active_scope_key"])

                with patch(
                    "cccc.daemon.group.group_creation_ops.append_event",
                    side_effect=OSError("ledger failed"),
                ):
                    second = client.post(
                        "/api/v1/groups",
                        json={"title": "second", "path": project, "by": "user"},
                    ).json()

            self.assertFalse(second.get("ok"), second)
            registry = load_registry()
            self.assertEqual(list(registry.groups), [first_id])
            self.assertEqual(registry.defaults.get(scope_key), first_id)
            self.assertEqual(
                len(list((Path(self._home.name) / "groups").glob("g_*"))), 1
            )

    def test_missing_path_preserves_legacy_group_create(self) -> None:
        with (
            patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
            self._client() as client,
        ):
            response = client.post(
                "/api/v1/groups",
                json={"title": "legacy", "topic": "old", "by": "user"},
            )

        body = response.json()
        self.assertTrue(body.get("ok"), body)
        self.assertTrue((body.get("result") or {}).get("group_id"))

    def test_present_invalid_path_is_rejected_without_creating_group(self) -> None:
        from cccc.kernel.registry import load_registry

        for invalid in (None, 7, "", "  ", "."):
            with self.subTest(path=invalid):
                with (
                    patch(
                        "cccc.ports.web.app.call_daemon", side_effect=self._call_daemon
                    ),
                    self._client() as client,
                ):
                    response = client.post(
                        "/api/v1/groups",
                        json={"title": "invalid", "path": invalid, "by": "user"},
                    )
                self.assertFalse(response.json().get("ok"), response.text)
                self.assertEqual(load_registry().groups, {})

    def test_same_scope_creates_independent_groups_and_latest_is_default(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.kernel.registry import load_registry

        with tempfile.TemporaryDirectory() as project:
            with (
                patch("cccc.ports.web.app.call_daemon", side_effect=self._call_daemon),
                self._client() as client,
            ):
                first = client.post(
                    "/api/v1/groups",
                    json={"title": "first", "path": project, "by": "user"},
                ).json()
                second = client.post(
                    "/api/v1/groups",
                    json={"title": "second", "path": project, "by": "user"},
                ).json()

            self.assertTrue(first.get("ok"), first)
            self.assertTrue(second.get("ok"), second)
            first_id = str((first.get("result") or {}).get("group_id") or "")
            second_id = str((second.get("result") or {}).get("group_id") or "")
            self.assertTrue(first_id)
            self.assertTrue(second_id)
            self.assertNotEqual(first_id, second_id)
            first_group = load_group(first_id)
            second_group = load_group(second_id)
            self.assertIsNotNone(first_group)
            self.assertIsNotNone(second_group)
            assert first_group is not None
            assert second_group is not None
            self.assertEqual(first_group.doc["scopes"], second_group.doc["scopes"])
            registry = load_registry()
            self.assertEqual(len(registry.groups), 2)
            self.assertEqual(
                registry.defaults[first_group.doc["scopes"][0]["scope_key"]],
                second_id,
            )
