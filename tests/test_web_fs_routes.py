import errno
import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebFsRoutes(unittest.TestCase):
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

    def test_fs_list_keeps_accessible_desktop_selectable_when_contents_are_private(self) -> None:
        """macOS may allow stat/resolve for Desktop while denying directory enumeration."""
        _, cleanup = self._with_home()
        try:
            desktop = Path.home() / "Desktop"
            desktop_text = str(desktop)
            original_resolve = Path.resolve
            original_exists = Path.exists
            original_is_dir = Path.is_dir
            original_iterdir = Path.iterdir

            def fake_resolve(path: Path, *args, **kwargs):
                if str(path) == desktop_text:
                    return desktop
                return original_resolve(path, *args, **kwargs)

            def fake_exists(path: Path) -> bool:
                if str(path) == desktop_text:
                    return True
                return original_exists(path)

            def fake_is_dir(path: Path) -> bool:
                if str(path) == desktop_text:
                    return True
                return original_is_dir(path)

            def fake_iterdir(path: Path):
                if str(path) == desktop_text:
                    raise PermissionError("Operation not permitted")
                return original_iterdir(path)

            with (
                patch.object(Path, "resolve", fake_resolve),
                patch.object(Path, "exists", fake_exists),
                patch.object(Path, "is_dir", fake_is_dir),
                patch.object(Path, "iterdir", fake_iterdir),
                self._client() as client,
            ):
                resp = client.get(f"/api/v1/fs/list?path={desktop_text}")

            self.assertEqual(resp.status_code, 200)
            body = resp.json()
            self.assertTrue(body.get("ok"), body)
            result = body.get("result") or {}
            self.assertEqual(result.get("path"), desktop_text)
            self.assertEqual(result.get("items"), [])
            self.assertEqual(result.get("readable"), False)
        finally:
            cleanup()

    def test_attach_still_auto_creates_missing_workspace_directory(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.contracts.v1 import DaemonRequest
            from cccc.daemon.server import handle_request

            create_resp, _ = handle_request(
                DaemonRequest.model_validate(
                    {"op": "group_create", "args": {"title": "new-dir", "topic": "", "by": "user"}}
                )
            )
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "")
            workspace = Path(tempfile.gettempdir()) / f"cccc_missing_workspace_{os.getpid()}"
            if workspace.exists():
                self.fail(f"test workspace unexpectedly exists: {workspace}")

            try:
                attach_resp, _ = handle_request(
                    DaemonRequest.model_validate(
                        {"op": "attach", "args": {"group_id": group_id, "path": str(workspace), "by": "user"}}
                    )
                )

                self.assertTrue(attach_resp.ok, getattr(attach_resp, "error", None))
                self.assertTrue(workspace.is_dir())
            finally:
                shutil.rmtree(workspace, ignore_errors=True)
        finally:
            cleanup()

    def test_create_directory_creates_one_child_and_rejects_nested_names(self) -> None:
        _, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as parent, self._client() as client:
                resp = client.post(
                    "/api/v1/fs/directory",
                    json={"parent": parent, "name": " demo "},
                )
                self.assertEqual(resp.status_code, 200, resp.text)
                target = Path(parent) / "demo"
                self.assertTrue(target.is_dir())
                self.assertEqual((resp.json().get("result") or {}).get("path"), str(target.resolve()))

                duplicate = client.post(
                    "/api/v1/fs/directory",
                    json={"parent": parent, "name": "demo"},
                )
                self.assertEqual(duplicate.status_code, 409, duplicate.text)

                nested = client.post(
                    "/api/v1/fs/directory",
                    json={"parent": parent, "name": "nested/path"},
                )
                self.assertEqual(nested.status_code, 400, nested.text)
                self.assertFalse((Path(parent) / "nested").exists())
        finally:
            cleanup()

    def test_create_directory_maps_other_os_errors_to_a_client_error(self) -> None:
        _, cleanup = self._with_home()
        try:
            original_mkdir = Path.mkdir

            def rejected_target_mkdir(path: Path, *args, **kwargs):
                if path.name == "rejected":
                    raise OSError(errno.ENAMETOOLONG, "File name too long")
                return original_mkdir(path, *args, **kwargs)

            with tempfile.TemporaryDirectory() as parent, self._client() as client, patch.object(
                Path,
                "mkdir",
                rejected_target_mkdir,
            ):
                resp = client.post(
                    "/api/v1/fs/directory",
                    json={"parent": parent, "name": "rejected"},
                )

            self.assertEqual(resp.status_code, 400, resp.text)
            error = resp.json().get("error") or {}
            self.assertEqual(error.get("code"), "filesystem_error")
            self.assertIn("File name too long", error.get("message") or "")
        finally:
            cleanup()
