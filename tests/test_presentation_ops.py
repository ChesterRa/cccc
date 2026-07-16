import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestPresentationOps(unittest.TestCase):
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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_presentation_publish_and_get_roundtrip(self) -> None:
        home, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "presentation-demo", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            empty, _ = self._call("presentation_get", {"group_id": group_id})
            self.assertTrue(empty.ok, getattr(empty, "error", None))
            empty_snapshot = (empty.result or {}).get("presentation") or {}
            self.assertEqual(len(empty_snapshot.get("slots") or []), 4)
            self.assertEqual(sum(1 for slot in empty_snapshot.get("slots") or [] if slot.get("card")), 0)

            publish, _ = self._call(
                "presentation_publish",
                {
                    "group_id": group_id,
                    "by": "user",
                    "slot": "auto",
                    "title": "Weekly Summary",
                    "content": "# Weekly Summary\n\n- shipped\n- verified",
                },
            )
            self.assertTrue(publish.ok, getattr(publish, "error", None))
            self.assertEqual(str((publish.result or {}).get("slot_id") or ""), "slot-1")
            card = (publish.result or {}).get("card") or {}
            self.assertEqual(str(card.get("card_type") or ""), "markdown")
            self.assertEqual(str(card.get("title") or ""), "Weekly Summary")

            fetched, _ = self._call("presentation_get", {"group_id": group_id})
            self.assertTrue(fetched.ok, getattr(fetched, "error", None))
            snapshot = (fetched.result or {}).get("presentation") or {}
            self.assertEqual(str(snapshot.get("highlight_slot_id") or ""), "slot-1")
            slots = snapshot.get("slots") or []
            self.assertEqual(str(((slots[0] or {}).get("card") or {}).get("title") or ""), "Weekly Summary")

            state_path = Path(home) / "groups" / group_id / "state" / "presentation.json"
            self.assertTrue(state_path.exists())
            state_doc = json.loads(state_path.read_text(encoding="utf-8"))
            self.assertEqual(str(state_doc.get("highlight_slot_id") or ""), "slot-1")
        finally:
            cleanup()

    def test_inline_web_preview_persists_blob_and_clear_resets_slot(self) -> None:
        home, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "presentation-web-preview", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            publish, _ = self._call(
                "presentation_publish",
                {
                    "group_id": group_id,
                    "by": "user",
                    "slot": "slot-2",
                    "card_type": "web_preview",
                    "title": "Preview",
                    "content": "<html><body><h1>demo</h1></body></html>",
                },
            )
            self.assertTrue(publish.ok, getattr(publish, "error", None))
            card = (publish.result or {}).get("card") or {}
            content = card.get("content") or {}
            blob_rel_path = str(content.get("blob_rel_path") or "")
            self.assertTrue(blob_rel_path.startswith("state/blobs/"))
            self.assertTrue((Path(home) / "groups" / group_id / blob_rel_path).exists())

            with patch(
                "cccc.daemon.group.presentation_ops.close_browser_surface_session",
                return_value={"closed": True, "browser_surface": {"active": False, "state": "idle"}},
            ) as close_mock:
                cleared, _ = self._call(
                    "presentation_clear",
                    {"group_id": group_id, "by": "user", "slot": "slot-2"},
                )
            self.assertTrue(cleared.ok, getattr(cleared, "error", None))
            self.assertEqual((cleared.result or {}).get("cleared_slots") or [], ["slot-2"])
            close_mock.assert_called_once_with(group_id=group_id, slot_id="slot-2")

            fetched, _ = self._call("presentation_get", {"group_id": group_id})
            self.assertTrue(fetched.ok, getattr(fetched, "error", None))
            slots = (fetched.result or {}).get("presentation", {}).get("slots") or []
            slot_two = next((slot for slot in slots if str(slot.get("slot_id") or "") == "slot-2"), None)
            self.assertIsNotNone(slot_two)
            self.assertIsNone((slot_two or {}).get("card"))
        finally:
            cleanup()

    def test_workspace_path_publish_keeps_live_link_metadata(self) -> None:
        _, cleanup = self._with_home()
        try:
            with tempfile.TemporaryDirectory() as workspace:
                notes_path = Path(workspace) / "notes.md"
                notes_path.write_text("# v1\n", encoding="utf-8")

                create, _ = self._call("group_create", {"title": "presentation-workspace", "topic": "", "by": "user"})
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
                        "path": "notes.md",
                    },
                )
                self.assertTrue(publish.ok, getattr(publish, "error", None))
                card = (publish.result or {}).get("card") or {}
                content = card.get("content") or {}
                self.assertEqual(str(card.get("card_type") or ""), "markdown")
                self.assertEqual(str(content.get("mode") or ""), "workspace_link")
                self.assertEqual(str(content.get("workspace_rel_path") or ""), "notes.md")
                self.assertEqual(str(content.get("blob_rel_path") or ""), "")
                self.assertEqual(str(card.get("source_ref") or ""), "notes.md")
        finally:
            cleanup()

    def test_publish_replacement_closes_existing_browser_session_for_slot(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "presentation-replace", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            initial, _ = self._call(
                "presentation_publish",
                {
                    "group_id": group_id,
                    "by": "user",
                    "slot": "slot-3",
                    "url": "https://example.com/one",
                    "card_type": "web_preview",
                    "title": "Initial",
                },
            )
            self.assertTrue(initial.ok, getattr(initial, "error", None))

            with patch(
                "cccc.daemon.group.presentation_ops.close_browser_surface_session",
                return_value={"closed": True, "browser_surface": {"active": False, "state": "idle"}},
            ) as close_mock:
                updated, _ = self._call(
                    "presentation_publish",
                    {
                        "group_id": group_id,
                        "by": "user",
                        "slot": "slot-3",
                        "url": "https://example.com/two",
                        "card_type": "web_preview",
                        "title": "Updated",
                    },
                )

            self.assertTrue(updated.ok, getattr(updated, "error", None))
            close_mock.assert_called_once_with(group_id=group_id, slot_id="slot-3")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
