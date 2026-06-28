import asyncio
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

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

    def _route_paths(self, app) -> set[str]:
        paths: set[str] = set()

        def collect(routes, prefix: str = "") -> None:
            for route in routes:
                path = getattr(route, "path", None)
                if isinstance(path, str):
                    paths.add(f"{prefix}{path}")
                original_router = getattr(route, "original_router", None)
                if original_router is None:
                    continue
                include_context = getattr(route, "include_context", None)
                include_prefix = str(getattr(include_context, "prefix", "") or "")
                collect(getattr(original_router, "routes", []), f"{prefix}{include_prefix}")

        collect(getattr(app, "routes", []))
        return paths

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

    def test_group_attach_rejects_relative_web_path(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with patch("cccc.ports.web.app.call_daemon", side_effect=AssertionError("relative attach should not call daemon")):
                with self._client() as client:
                    resp = client.post(f"/api/v1/groups/{group_id}/attach", json={"path": ".", "by": "user"})
            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertFalse(bool(body.get("ok")))
            self.assertEqual(str((body.get("error") or {}).get("code") or ""), "invalid_scope_path")
        finally:
            cleanup()

    def test_group_attach_sends_absolute_web_path_to_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()
            with tempfile.TemporaryDirectory(prefix="cccc_web_attach_") as project_dir:
                captured: dict = {}

                def _fake_call_daemon(req: dict):
                    captured.update(req)
                    return {"ok": True, "result": {"group_id": group_id, "scope_key": "s_web"}}

                with patch("cccc.ports.web.app.call_daemon", side_effect=_fake_call_daemon):
                    with self._client() as client:
                        resp = client.post(f"/api/v1/groups/{group_id}/attach", json={"path": project_dir, "by": "user"})
                self.assertEqual(resp.status_code, 200)
                body = resp.json()
                self.assertTrue(bool(body.get("ok")), body)
                self.assertEqual(captured.get("op"), "attach")
                self.assertEqual(str((captured.get("args") or {}).get("path") or ""), str(Path(project_dir).resolve()))
        finally:
            cleanup()

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
            route_paths = self._route_paths(app)
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

    def test_group_copy_preview_too_large_error_includes_limit_and_file_size(self) -> None:
        _, cleanup = self._with_home()
        try:
            with patch("cccc.ports.web.routes.group_copy_uploads.WEB_MAX_GROUP_COPY_PACKAGE_BYTES", 8):
                with self._client() as client:
                    resp = client.post(
                        "/api/v1/groups/copy/preview_import",
                        files={"file": ("group.zip", b"123456789", "application/zip")},
                    )

            self.assertEqual(resp.status_code, 413)
            error = resp.json().get("error") or {}
            self.assertEqual(error.get("code"), "copy_package_too_large")
            message = str(error.get("message") or "")
            self.assertIn("max 8 bytes", message)
            self.assertIn("selected file is 9 bytes", message)
        finally:
            cleanup()

    def test_group_copy_export_too_large_returns_413_not_zip_download(self) -> None:
        from cccc.daemon.ops import group_copy_ops

        _, cleanup = self._with_home()
        try:
            group_id = self._create_group()

            with patch.object(group_copy_ops, "MAX_PACKAGE_BYTES", 8):
                with self._client() as client:
                    resp = client.get(f"/api/v1/groups/{group_id}/copy/export")

            self.assertEqual(resp.status_code, 413)
            self.assertNotEqual(resp.headers.get("content-type"), "application/zip")
            error = resp.json().get("error") or {}
            self.assertEqual(error.get("code"), "copy_package_too_large")
            details = error.get("details") or {}
            self.assertEqual(details.get("max_bytes"), 8)
            self.assertGreater(int(details.get("actual_bytes") or 0), 8)
        finally:
            cleanup()

    def test_group_copy_default_package_limit_is_one_gib(self) -> None:
        from cccc.daemon.ops import group_copy_ops
        from cccc.ports.web.routes import group_copy_uploads

        self.assertEqual(group_copy_uploads.WEB_MAX_GROUP_COPY_PACKAGE_BYTES, 1024 * 1024 * 1024)
        self.assertEqual(group_copy_ops.MAX_PACKAGE_BYTES, 1024 * 1024 * 1024)

    def test_group_copy_upload_spool_reads_chunks_and_cleans_up_on_limit(self) -> None:
        class FakeUpload:
            def __init__(self, chunks: list[bytes]):
                self.chunks = list(chunks)
                self.read_sizes: list[int] = []

            async def read(self, size: int = -1) -> bytes:
                self.read_sizes.append(size)
                return self.chunks.pop(0) if self.chunks else b""

        _, cleanup = self._with_home()
        try:
            from cccc.ports.web.routes import group_copy_uploads
            from cccc.ports.web.routes import groups as group_routes

            upload = FakeUpload([b"1234", b"5678", b"9"])
            with patch.object(group_copy_uploads, "WEB_MAX_GROUP_COPY_PACKAGE_BYTES", 8), patch.object(
                group_copy_uploads,
                "WEB_GROUP_COPY_UPLOAD_CHUNK_BYTES",
                4,
            ):
                with self.assertRaises(Exception):
                    asyncio.run(group_copy_uploads.spool_group_copy_upload(upload))

            self.assertEqual(upload.read_sizes, [4, 4, 4])
            stage_dir = group_copy_uploads.group_copy_upload_stage_dir()
            self.assertFalse(any(stage_dir.glob("*.zip")))
        finally:
            cleanup()

    def test_group_copy_preview_stages_upload_id_for_import_reuse(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.ports.web.routes import groups as group_routes

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

                with self._client() as client:
                    export_resp = client.get(f"/api/v1/groups/{group_id}/copy/export")
                    self.assertEqual(export_resp.status_code, 200)
                    package_bytes = export_resp.content

                    preview_resp = client.post(
                        "/api/v1/groups/copy/preview_import",
                        files={"file": ("group.zip", package_bytes, "application/zip")},
                    )
                    self.assertEqual(preview_resp.status_code, 200)
                    preview_body = preview_resp.json()
                    self.assertTrue(bool(preview_body.get("ok")), preview_body)
                    upload_id = str(((preview_body.get("result") or {}).get("upload_id")) or "")
                    self.assertTrue(upload_id)
                    staged_path = group_routes._group_copy_upload_path(upload_id)
                    self.assertTrue(staged_path.is_file())

                    import_resp = client.post(
                        "/api/v1/groups/copy/import",
                        data={"upload_id": upload_id, "workspace_root": str(Path(workspace_raw).resolve()), "title": "Imported copy", "by": "user"},
                    )
                    self.assertEqual(import_resp.status_code, 200)
                    import_body = import_resp.json()
                    self.assertTrue(bool(import_body.get("ok")), import_body)
                    self.assertFalse(staged_path.exists())

                    export_tmp_dir = Path(os.environ["CCCC_HOME"]) / "tmp" / "group-copy-export"
                    self.assertFalse(any(export_tmp_dir.glob("*.zip")))
        finally:
            cleanup()

    def test_group_copy_upload_id_import_keeps_staging_on_fixable_failure_then_deletes_on_success(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.ports.web.routes import groups as group_routes

        home, cleanup = self._with_home()
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

                with self._client() as client:
                    export_resp = client.get(f"/api/v1/groups/{group_id}/copy/export")
                    self.assertEqual(export_resp.status_code, 200)
                    preview_resp = client.post(
                        "/api/v1/groups/copy/preview_import",
                        files={"file": ("group.zip", export_resp.content, "application/zip")},
                    )
                    upload_id = str((((preview_resp.json().get("result") or {}).get("upload_id")) or ""))
                    self.assertTrue(upload_id)
                    staged_path = group_routes._group_copy_upload_path(upload_id)

                    bad_resp = client.post(
                        "/api/v1/groups/copy/import",
                        data={"upload_id": upload_id, "workspace_root": home, "title": "Imported copy", "by": "user"},
                    )
                    self.assertEqual(bad_resp.status_code, 200)
                    self.assertFalse(bool(bad_resp.json().get("ok")), bad_resp.json())
                    self.assertTrue(staged_path.is_file())

                    good_resp = client.post(
                        "/api/v1/groups/copy/import",
                        data={"upload_id": upload_id, "workspace_root": str(Path(workspace_raw).resolve()), "title": "Imported copy", "by": "user"},
                    )
                    self.assertEqual(good_resp.status_code, 200)
                    self.assertTrue(bool(good_resp.json().get("ok")), good_resp.json())
                    self.assertFalse(staged_path.exists())
        finally:
            cleanup()

    def test_group_copy_upload_cleanup_endpoint_deletes_staging(self) -> None:
        from cccc.kernel.group import load_group
        from cccc.ports.web.routes import groups as group_routes

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

                with self._client() as client:
                    export_resp = client.get(f"/api/v1/groups/{group_id}/copy/export")
                    preview_resp = client.post(
                        "/api/v1/groups/copy/preview_import",
                        files={"file": ("group.zip", export_resp.content, "application/zip")},
                    )
                    upload_id = str((((preview_resp.json().get("result") or {}).get("upload_id")) or ""))
                    staged_path = group_routes._group_copy_upload_path(upload_id)
                    self.assertTrue(staged_path.is_file())

                    cleanup_resp = client.delete(f"/api/v1/groups/copy/uploads/{upload_id}")
                    self.assertEqual(cleanup_resp.status_code, 200)
                    self.assertTrue(bool(cleanup_resp.json().get("ok")), cleanup_resp.json())
                    self.assertFalse(staged_path.exists())
        finally:
            cleanup()

    def test_group_copy_import_invalid_upload_id_returns_clear_error(self) -> None:
        _, cleanup = self._with_home()
        try:
            with self._client() as client:
                resp = client.post(
                    "/api/v1/groups/copy/import",
                    data={"upload_id": "missing", "workspace_root": "/tmp", "title": "Imported copy", "by": "user"},
                )
            self.assertEqual(resp.status_code, 404)
            error = resp.json().get("error") or {}
            self.assertEqual(error.get("code"), "copy_upload_not_found")
            self.assertIn("upload not found", str(error.get("message") or ""))
        finally:
            cleanup()

    def test_group_copy_upload_startup_cleanup_removes_expired_staging_without_new_upload(self) -> None:
        import time

        from cccc.ports.web.routes import group_copy_uploads

        home, cleanup = self._with_home()
        try:
            stage_dir = Path(home) / "tmp" / "group-copy-uploads"
            stage_dir.mkdir(parents=True, exist_ok=True)
            old_file = stage_dir / ("a" * 32 + ".zip")
            new_file = stage_dir / ("b" * 32 + ".zip")
            old_file.write_bytes(b"old")
            new_file.write_bytes(b"new")
            now = time.time()
            os.utime(old_file, (now - 10, now - 10))
            os.utime(new_file, (now, now))

            with patch.object(group_copy_uploads, "WEB_GROUP_COPY_UPLOAD_TTL_SECONDS", 1):
                with self._client():
                    pass

            self.assertFalse(old_file.exists())
            self.assertTrue(new_file.exists())
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
