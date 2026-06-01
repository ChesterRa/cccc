import os
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from fastapi.testclient import TestClient


class TestWebGroupRoutesLocal(unittest.TestCase):
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

    def _app(self):
        from cccc.ports.web.app import create_app

        return create_app()

    def _create_group(self) -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        return create_group(reg, title="group-local-read", topic="local topic").group_id

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

    def test_group_show_reads_local_projection_without_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with patch("cccc.ports.web.app.call_daemon", side_effect=AssertionError("group_show should not call daemon")):
                with self._client() as client:
                    resp = client.get(f"/api/v1/groups/{group_id}")
                    self.assertEqual(resp.status_code, 200)
                    body = resp.json()
                    self.assertTrue(bool(body.get("ok")), body)
                    group = (body.get("result") or {}).get("group") or {}
                    self.assertEqual(str(group.get("group_id") or ""), group_id)
                    self.assertEqual(str(group.get("title") or ""), "group-local-read")
                    self.assertEqual(str(group.get("topic") or ""), "local topic")
        finally:
            cleanup()

    def test_legacy_codex_headless_routes_remain_available(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            app = self._app()
            route_paths = {getattr(route, "path", "") for route in app.routes}
            self.assertIn("/api/v1/groups/{group_id}/codex/stream", route_paths)

            with TestClient(app) as client:
                snapshot_resp = client.get(f"/api/v1/groups/{group_id}/codex/snapshot")
                self.assertEqual(snapshot_resp.status_code, 200)
                snapshot_body = snapshot_resp.json()
                self.assertTrue(bool(snapshot_body.get("ok")), snapshot_body)
                self.assertEqual(str((snapshot_body.get("result") or {}).get("group_id") or ""), group_id)
        finally:
            cleanup()

    def test_group_copy_export_preview_import_routes(self) -> None:
        from cccc.kernel.group import load_group

        _, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace_raw:
                group_id = self._create_group()
                group = load_group(group_id)
                self.assertIsNotNone(group)
                assert group is not None
                group.doc["scopes"] = [
                    {
                        "scope_key": "s_test",
                        "url": str(Path(workspace_raw).resolve()),
                        "label": "workspace",
                        "git_remote": "",
                    }
                ]
                group.doc["active_scope_key"] = "s_test"
                group.save()

                with patch("cccc.ports.web.app.call_daemon", side_effect=AssertionError("copy routes should not call daemon IPC")):
                    with self._client() as client:
                        export_resp = client.get(f"/api/v1/groups/{group_id}/copy/export")
                        self.assertEqual(export_resp.status_code, 200)
                        self.assertEqual(export_resp.headers.get("content-type"), "application/zip")
                        content_disposition = str(export_resp.headers.get("content-disposition") or "")
                        content_disposition.encode("latin-1")
                        self.assertIn("filename=", content_disposition)
                        self.assertIn("filename*=UTF-8''", content_disposition)
                        package_bytes = export_resp.content
                        self.assertGreater(len(package_bytes), 0)

                        preview_resp = client.post(
                            "/api/v1/groups/copy/preview_import",
                            files={"file": ("group.zip", package_bytes, "application/zip")},
                        )
                        self.assertEqual(preview_resp.status_code, 200)
                        preview_body = preview_resp.json()
                        self.assertTrue(bool(preview_body.get("ok")), preview_body)
                        preview = ((preview_body.get("result") or {}).get("preview") or {})
                        self.assertEqual(str(preview.get("source_group_id") or ""), group_id)
                        self.assertFalse(bool(preview.get("workspace_included")))
                        self.assertFalse(bool(preview.get("contains_secrets")))

                        import_resp = client.post(
                            "/api/v1/groups/copy/import",
                            data={"workspace_root": str(Path(workspace_raw).resolve()), "title": "Imported copy", "by": "user"},
                            files={"file": ("group.zip", package_bytes, "application/zip")},
                        )
                        self.assertEqual(import_resp.status_code, 200)
                        import_body = import_resp.json()
                        self.assertTrue(bool(import_body.get("ok")), import_body)
                        imported_id = str(((import_body.get("result") or {}).get("group_id")) or "")
                        self.assertTrue(imported_id)
                        self.assertNotEqual(imported_id, group_id)
        finally:
            cleanup()

    def test_copy_export_content_disposition_supports_unicode_filename(self) -> None:
        from cccc.ports.web.routes.groups import _download_content_disposition

        header = _download_content_disposition("cccc-group--中文项目--g_123.zip")

        header.encode("latin-1")
        self.assertIn('filename="cccc-group--____--g_123.zip"', header)
        self.assertIn("filename*=UTF-8''cccc-group--%E4%B8%AD%E6%96%87%E9%A1%B9%E7%9B%AE--g_123.zip", header)

    def test_headless_snapshot_replays_recent_completed_turn(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.headless_events import append_headless_event

            group_id = self._create_group()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            append_headless_event(group.path, group_id=group_id, actor_id="coder", event_type="headless.turn.started", data={"turn_id": "turn-1"})
            append_headless_event(
                group.path,
                group_id=group_id,
                actor_id="coder",
                event_type="headless.activity.started",
                data={"activity_id": "tool-1", "summary": "Run tests", "kind": "tool"},
            )
            append_headless_event(
                group.path,
                group_id=group_id,
                actor_id="coder",
                event_type="headless.message.completed",
                data={"stream_id": "stream-1", "text": "Done", "phase": "final_answer"},
            )
            append_headless_event(group.path, group_id=group_id, actor_id="coder", event_type="headless.turn.completed", data={"turn_id": "turn-1"})

            with self._client() as client:
                resp = client.get(f"/api/v1/groups/{group_id}/headless/snapshot")
                self.assertEqual(resp.status_code, 200)
                body = resp.json()
                self.assertTrue(bool(body.get("ok")), body)
                events = ((body.get("result") or {}).get("events") or [])
                event_types = [str(event.get("type") or "") for event in events]
                self.assertEqual(
                    event_types,
                    [
                        "headless.turn.started",
                        "headless.activity.started",
                        "headless.message.completed",
                        "headless.turn.completed",
                    ],
                )
        finally:
            cleanup()

    def test_headless_snapshot_replays_recent_completed_control_turn(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.group import load_group
            from cccc.kernel.headless_events import append_headless_event

            group_id = self._create_group()
            group = load_group(group_id)
            self.assertIsNotNone(group)
            assert group is not None
            append_headless_event(
                group.path,
                group_id=group_id,
                actor_id="voice-secretary",
                event_type="headless.control.started",
                data={"turn_id": "control-1", "control_kind": "bootstrap"},
            )
            append_headless_event(
                group.path,
                group_id=group_id,
                actor_id="voice-secretary",
                event_type="headless.control.completed",
                data={"turn_id": "control-1", "control_kind": "bootstrap"},
            )

            with self._client() as client:
                resp = client.get(f"/api/v1/groups/{group_id}/headless/snapshot")
                self.assertEqual(resp.status_code, 200)
                body = resp.json()
                self.assertTrue(bool(body.get("ok")), body)
                events = ((body.get("result") or {}).get("events") or [])
                event_types = [str(event.get("type") or "") for event in events]
                self.assertEqual(
                    event_types,
                    [
                        "headless.control.started",
                        "headless.control.completed",
                    ],
                )
        finally:
            cleanup()
