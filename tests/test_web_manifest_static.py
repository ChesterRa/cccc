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

    def test_capability_center_path_serves_spa_index(self) -> None:
        client, cleanup = self._with_static_client()
        try:
            for path in ("/ui/capabilities", "/ui/capabilities/"):
                with self.subTest(path=path):
                    resp = client.get(path)

                    self.assertEqual(resp.status_code, 200)
                    self.assertTrue(str(resp.headers.get("content-type") or "").startswith("text/html"))
                    self.assertEqual(resp.text, "<html><body>ok</body></html>")
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

    def test_packaged_ui_dist_contains_remote_pairing_flow(self) -> None:
        dist = Path(__file__).resolve().parents[1] / "src" / "cccc" / "ports" / "web" / "dist"
        bundles = list((dist / "assets").glob("*.js"))
        self.assertTrue(bundles, "JavaScript chunks are missing from packaged web dist")
        text = "\n".join(bundle.read_text(encoding="utf-8") for bundle in bundles)

        self.assertIn("issuer_endpoint", text)
        self.assertIn("federation_session", text)
        self.assertIn("/api/federation/pairing/remote-requests", text)
        self.assertNotIn("cccc.libp2p.connection_info", text)
        self.assertNotIn("libp2p_cccc", text)


if __name__ == "__main__":
    unittest.main()
