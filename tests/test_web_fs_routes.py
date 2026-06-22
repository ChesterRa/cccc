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
