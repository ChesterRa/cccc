import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebPresentationApi(unittest.TestCase):
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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_group_presentation_get_route_returns_snapshot(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "web-presentation", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            publish, _ = self._call(
                "presentation_publish",
                {
                    "group_id": group_id,
                    "by": "user",
                    "title": "Report",
                    "content": "# report",
                },
            )
            self.assertTrue(publish.ok, getattr(publish, "error", None))

            with patch("cccc.ports.web.app.call_daemon", side_effect=AssertionError("presentation_get should not call daemon")):
                with self._client() as client:
                    resp = client.get(f"/api/v1/groups/{group_id}/presentation")

            self.assertEqual(resp.status_code, 200)
            payload = resp.json()
            self.assertTrue(bool(payload.get("ok")))
            self.assertEqual(payload["result"]["presentation"]["highlight_slot_id"], "slot-1")
            self.assertEqual(payload["result"]["presentation"]["slots"][0]["card"]["title"], "Report")
        finally:
            cleanup()

    def test_group_presentation_asset_route_serves_inline_blob(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "web-presentation-asset", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            publish, _ = self._call(
                "presentation_publish",
                {
                    "group_id": group_id,
                    "by": "user",
                    "slot": "slot-1",
                    "card_type": "web_preview",
                    "title": "Preview",
                    "content": "<html><body><h1>hello</h1></body></html>",
                },
            )
            self.assertTrue(publish.ok, getattr(publish, "error", None))

            with self._client() as client:
                resp = client.get(f"/api/v1/groups/{group_id}/presentation/slots/slot-1/asset")

            self.assertEqual(resp.status_code, 200)
            self.assertIn("inline", str(resp.headers.get("content-disposition") or ""))
            self.assertTrue(str(resp.headers.get("content-type") or "").startswith("text/html"))
            self.assertIn("<h1>hello</h1>", resp.text)
        finally:
            cleanup()

    def test_group_presentation_ref_snapshot_upload_stores_blob(self) -> None:
        home, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "web-presentation-ref-snapshot", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            with self._client() as client:
                resp = client.post(
                    f"/api/v1/groups/{group_id}/presentation/ref_snapshot",
                    data={
                        "by": "user",
                        "slot": "slot-2",
                        "source": "browser_surface",
                        "captured_at": "2026-03-22T12:00:00Z",
                        "width": "1280",
                        "height": "720",
                    },
                    files={"file": ("snapshot.jpg", b"fake-jpeg-bytes", "image/jpeg")},
                )

            self.assertEqual(resp.status_code, 200)
            payload = resp.json()
            self.assertTrue(bool(payload.get("ok")))
            snapshot = payload["result"]["snapshot"]
            self.assertTrue(str(snapshot.get("path") or "").startswith("state/blobs/"))
            self.assertEqual(str(snapshot.get("source") or ""), "browser_surface")
            self.assertEqual(int(snapshot.get("width") or 0), 1280)
            self.assertEqual(int(snapshot.get("height") or 0), 720)
            self.assertTrue((Path(home) / "groups" / group_id / str(snapshot.get("path") or "")).exists())
        finally:
            cleanup()

    def test_group_presentation_asset_route_serves_latest_workspace_file(self) -> None:
        _, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace:
                html_path = os.path.join(workspace, "demo.html")
                with open(html_path, "w", encoding="utf-8") as handle:
                    handle.write("<html><body><h1>v1</h1></body></html>")

                create, _ = self._call("group_create", {"title": "web-presentation-workspace", "topic": "", "by": "user"})
                self.assertTrue(create.ok, getattr(create, "error", None))
                group_id = str((create.result or {}).get("group_id") or "")

                attach, _ = self._call("attach", {"group_id": group_id, "path": workspace, "by": "user"})
                self.assertTrue(attach.ok, getattr(attach, "error", None))

                publish, _ = self._call(
                    "presentation_publish",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "slot": "slot-1",
                        "path": "demo.html",
                    },
                )
                self.assertTrue(publish.ok, getattr(publish, "error", None))

                with self._client() as client:
                    first = client.get(f"/api/v1/groups/{group_id}/presentation/slots/slot-1/asset")
                    self.assertEqual(first.status_code, 200)
                    self.assertIn("<h1>v1</h1>", first.text)

                    with open(html_path, "w", encoding="utf-8") as handle:
                        handle.write("<html><body><h1>v2</h1></body></html>")

                    second = client.get(f"/api/v1/groups/{group_id}/presentation/slots/slot-1/asset?v=2")

                self.assertEqual(second.status_code, 200)
                self.assertIn("<h1>v2</h1>", second.text)
                self.assertEqual(second.headers.get("cache-control"), "no-store")
        finally:
            cleanup()

    def test_group_presentation_asset_route_supports_attachment_download(self) -> None:
        _, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace:
                file_path = os.path.join(workspace, "artifact.bin")
                with open(file_path, "wb") as handle:
                    handle.write(b"\x00\x01\x02demo")

                create, _ = self._call("group_create", {"title": "web-presentation-download", "topic": "", "by": "user"})
                self.assertTrue(create.ok, getattr(create, "error", None))
                group_id = str((create.result or {}).get("group_id") or "")

                attach, _ = self._call("attach", {"group_id": group_id, "path": workspace, "by": "user"})
                self.assertTrue(attach.ok, getattr(attach, "error", None))

                publish, _ = self._call(
                    "presentation_publish",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "slot": "slot-1",
                        "path": "artifact.bin",
                    },
                )
                self.assertTrue(publish.ok, getattr(publish, "error", None))

                with self._client() as client:
                    resp = client.get(f"/api/v1/groups/{group_id}/presentation/slots/slot-1/asset?download=1")

                self.assertEqual(resp.status_code, 200)
                self.assertIn("attachment", str(resp.headers.get("content-disposition") or ""))
                self.assertEqual(resp.headers.get("cache-control"), "no-store")
                self.assertEqual(resp.content, b"\x00\x01\x02demo")
        finally:
            cleanup()

    def test_group_presentation_publish_route_pins_url(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "web-presentation-publish", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            def fake_call_daemon(req: dict):
                resp, _ = self._call(str(req.get("op") or ""), dict(req.get("args") or {}))
                payload = {"ok": bool(resp.ok)}
                if resp.result is not None:
                    payload["result"] = resp.result
                if resp.error is not None:
                    payload["error"] = resp.error.model_dump(mode="json", exclude_none=True)
                return payload

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    resp = client.post(
                        f"/api/v1/groups/{group_id}/presentation/publish",
                        json={"slot": "slot-3", "url": "https://example.com/dashboard", "by": "user"},
                    )

            self.assertEqual(resp.status_code, 200)
            payload = resp.json()
            self.assertTrue(bool(payload.get("ok")))
            self.assertEqual(payload["result"]["slot_id"], "slot-3")
            self.assertEqual(payload["result"]["card"]["card_type"], "web_preview")
            self.assertEqual(payload["result"]["presentation"]["highlight_slot_id"], "slot-3")
        finally:
            cleanup()

    def test_group_presentation_publish_upload_route_accepts_markdown_file(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "web-presentation-upload", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            def fake_call_daemon(req: dict):
                resp, _ = self._call(str(req.get("op") or ""), dict(req.get("args") or {}))
                payload = {"ok": bool(resp.ok)}
                if resp.result is not None:
                    payload["result"] = resp.result
                if resp.error is not None:
                    payload["error"] = resp.error.model_dump(mode="json", exclude_none=True)
                return payload

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    resp = client.post(
                        f"/api/v1/groups/{group_id}/presentation/publish_upload",
                        data={"slot": "slot-1", "by": "user"},
                        files={"file": ("notes.md", b"# hello\n\nworld", "text/markdown")},
                    )

            self.assertEqual(resp.status_code, 200)
            payload = resp.json()
            self.assertTrue(bool(payload.get("ok")))
            self.assertEqual(payload["result"]["slot_id"], "slot-1")
            self.assertEqual(payload["result"]["card"]["card_type"], "markdown")
            self.assertEqual(payload["result"]["card"]["title"], "notes.md")
            self.assertEqual(payload["result"]["presentation"]["highlight_slot_id"], "slot-1")
        finally:
            cleanup()

    def test_group_presentation_workspace_list_and_publish_route(self) -> None:
        _, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace:
                os.makedirs(os.path.join(workspace, "docs"), exist_ok=True)
                with open(os.path.join(workspace, "docs", "report.md"), "w", encoding="utf-8") as handle:
                    handle.write("# report\n")

                create, _ = self._call("group_create", {"title": "web-presentation-workspace-list", "topic": "", "by": "user"})
                self.assertTrue(create.ok, getattr(create, "error", None))
                group_id = str((create.result or {}).get("group_id") or "")

                attach, _ = self._call("attach", {"group_id": group_id, "path": workspace, "by": "user"})
                self.assertTrue(attach.ok, getattr(attach, "error", None))

                def fake_call_daemon(req: dict):
                    resp, _ = self._call(str(req.get("op") or ""), dict(req.get("args") or {}))
                    payload = {"ok": bool(resp.ok)}
                    if resp.result is not None:
                        payload["result"] = resp.result
                    if resp.error is not None:
                        payload["error"] = resp.error.model_dump(mode="json", exclude_none=True)
                    return payload

                with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                    with self._client() as client:
                        listing = client.get(f"/api/v1/groups/{group_id}/presentation/workspace/list")
                        publish = client.post(
                            f"/api/v1/groups/{group_id}/presentation/publish_workspace",
                            json={"slot": "slot-4", "path": "docs/report.md", "by": "user"},
                        )

                self.assertEqual(listing.status_code, 200)
                listing_payload = listing.json()
                self.assertTrue(bool(listing_payload.get("ok")))
                items = listing_payload["result"]["items"]
                docs_item = next((item for item in items if str(item.get("name") or "") == "docs"), None)
                self.assertIsNotNone(docs_item)
                self.assertTrue(bool((docs_item or {}).get("is_dir")))

                self.assertEqual(publish.status_code, 200)
                publish_payload = publish.json()
                self.assertTrue(bool(publish_payload.get("ok")))
                self.assertEqual(publish_payload["result"]["slot_id"], "slot-4")
                self.assertEqual(publish_payload["result"]["card"]["content"]["mode"], "workspace_link")
                self.assertEqual(publish_payload["result"]["card"]["content"]["workspace_rel_path"], "docs/report.md")
        finally:
            cleanup()

    def test_group_presentation_clear_route_resets_slot(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "web-presentation-clear", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")
            publish, _ = self._call(
                "presentation_publish",
                {
                    "group_id": group_id,
                    "by": "user",
                    "slot": "slot-2",
                    "title": "Deck",
                    "content": "# deck",
                },
            )
            self.assertTrue(publish.ok, getattr(publish, "error", None))

            def fake_call_daemon(req: dict):
                resp, _ = self._call(str(req.get("op") or ""), dict(req.get("args") or {}))
                payload = {"ok": bool(resp.ok)}
                if resp.result is not None:
                    payload["result"] = resp.result
                if resp.error is not None:
                    payload["error"] = resp.error.model_dump(mode="json", exclude_none=True)
                return payload

            with patch("cccc.ports.web.app.call_daemon", side_effect=fake_call_daemon):
                with self._client() as client:
                    resp = client.post(
                        f"/api/v1/groups/{group_id}/presentation/clear",
                        json={"slot": "slot-2", "by": "user"},
                    )

            self.assertEqual(resp.status_code, 200)
            payload = resp.json()
            self.assertTrue(bool(payload.get("ok")))
            self.assertEqual(payload["result"]["cleared_slots"], ["slot-2"])
            slot_two = next(
                (slot for slot in payload["result"]["presentation"]["slots"] if str(slot.get("slot_id") or "") == "slot-2"),
                None,
            )
            self.assertIsNotNone(slot_two)
            self.assertIsNone((slot_two or {}).get("card"))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
