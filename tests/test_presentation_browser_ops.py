import os
import tempfile
import unittest
from unittest.mock import patch


class TestPresentationBrowserOps(unittest.TestCase):
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

    def test_presentation_browser_open_dispatches_to_runtime(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "browser-open", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            with patch(
                "cccc.daemon.group.presentation_browser_ops.open_browser_surface_session",
                return_value={
                    "active": True,
                    "state": "ready",
                    "message": "ready",
                    "error": {},
                    "strategy": "playwright_chromium_cdp",
                    "url": "http://127.0.0.1:3000",
                    "width": 1440,
                    "height": 900,
                    "started_at": "2026-03-21T00:00:00Z",
                    "updated_at": "2026-03-21T00:00:01Z",
                    "last_frame_seq": 1,
                    "last_frame_at": "2026-03-21T00:00:01Z",
                    "controller_attached": False,
                },
            ) as open_mock:
                resp, _ = self._call(
                    "presentation_browser_open",
                    {
                        "group_id": group_id,
                        "slot": "slot-2",
                        "by": "user",
                        "url": "http://127.0.0.1:3000",
                        "width": 1440,
                        "height": 900,
                    },
                )

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            open_mock.assert_called_once_with(
                group_id=group_id,
                slot_id="slot-2",
                url="http://127.0.0.1:3000",
                width=1440,
                height=900,
            )
            surface = (resp.result or {}).get("browser_surface") or {}
            self.assertEqual(surface.get("state"), "ready")
            self.assertEqual(surface.get("strategy"), "playwright_chromium_cdp")
        finally:
            cleanup()

    def test_presentation_browser_info_returns_idle_without_session(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "browser-info", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            resp, _ = self._call("presentation_browser_info", {"group_id": group_id, "slot": "slot-1"})

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            surface = (resp.result or {}).get("browser_surface") or {}
            self.assertFalse(bool(surface.get("active")))
            self.assertEqual(surface.get("state"), "idle")
        finally:
            cleanup()

    def test_presentation_browser_close_dispatches_to_runtime(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "browser-close", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")

            with patch(
                "cccc.daemon.group.presentation_browser_ops.close_browser_surface_session",
                return_value={
                    "closed": True,
                    "browser_surface": {
                        "active": False,
                        "state": "idle",
                        "message": "No browser surface session is active.",
                        "error": {},
                        "strategy": "",
                        "url": "",
                        "width": 0,
                        "height": 0,
                        "started_at": "",
                        "updated_at": "",
                        "last_frame_seq": 0,
                        "last_frame_at": "",
                        "controller_attached": False,
                    },
                },
            ) as close_mock:
                resp, _ = self._call("presentation_browser_close", {"group_id": group_id, "slot": "slot-3", "by": "user"})

            self.assertTrue(resp.ok, getattr(resp, "error", None))
            close_mock.assert_called_once_with(group_id=group_id, slot_id="slot-3")
            self.assertTrue(bool((resp.result or {}).get("closed")))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
