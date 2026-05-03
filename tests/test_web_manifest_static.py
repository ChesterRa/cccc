import os
import tempfile
import unittest
from pathlib import Path

from fastapi.testclient import TestClient
from starlette.websockets import WebSocketDisconnect


class TestWebManifestStatic(unittest.TestCase):
    def _with_static_client(self):
        old_home = os.environ.get("CCCC_HOME")
        old_dist = os.environ.get("CCCC_WEB_DIST")
        home_tmp = tempfile.TemporaryDirectory()
        dist_tmp = tempfile.TemporaryDirectory()
        os.environ["CCCC_HOME"] = home_tmp.name
        os.environ["CCCC_WEB_DIST"] = dist_tmp.name
        dist_path = Path(dist_tmp.name)
        (dist_path / "index.html").write_text("<html><body>ok</body></html>", encoding="utf-8")
        (dist_path / "manifest.webmanifest").write_text('{"name":"CCCC"}\n', encoding="utf-8")

        def cleanup() -> None:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            if old_dist is None:
                os.environ.pop("CCCC_WEB_DIST", None)
            else:
                os.environ["CCCC_WEB_DIST"] = old_dist
            dist_tmp.cleanup()
            home_tmp.cleanup()

        from cccc.ports.web.app import create_app

        return TestClient(create_app()), cleanup

    def test_manifest_is_served_by_staticfiles_without_attachment_header(self) -> None:
        client, cleanup = self._with_static_client()
        try:
            resp = client.get("/ui/manifest.webmanifest")

            self.assertEqual(resp.status_code, 200)
            self.assertTrue(str(resp.headers.get("content-type") or "").startswith("application/manifest+json"))
            self.assertNotIn("content-disposition", resp.headers)
            self.assertEqual(resp.text, '{"name":"CCCC"}\n')
        finally:
            cleanup()

    def test_ui_websocket_path_closes_without_staticfiles_assertion(self) -> None:
        client, cleanup = self._with_static_client()
        try:
            with self.assertRaises(WebSocketDisconnect) as cm:
                with client.websocket_connect("/ui/ws") as ws:
                    ws.receive_text()
            self.assertIsInstance(cm.exception.code, int)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
