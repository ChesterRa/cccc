import os
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebPresentationBrowserApi(unittest.TestCase):
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

    def test_browser_surface_session_start_route_calls_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "browser-route", "topic": "", "by": "user"})
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
                    "width": 1600,
                    "height": 900,
                    "started_at": "2026-03-21T00:00:00Z",
                    "updated_at": "2026-03-21T00:00:01Z",
                    "last_frame_seq": 1,
                    "last_frame_at": "2026-03-21T00:00:01Z",
                    "controller_attached": False,
                },
            ):
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
                            f"/api/v1/groups/{group_id}/presentation/browser_surface/session",
                            json={"slot": "slot-2", "url": "http://127.0.0.1:3000", "width": 1600, "height": 900, "by": "user"},
                        )

            self.assertEqual(resp.status_code, 200)
            payload = resp.json()
            self.assertTrue(bool(payload.get("ok")))
            self.assertEqual(payload["result"]["browser_surface"]["state"], "ready")
            self.assertEqual(payload["result"]["browser_surface"]["width"], 1600)
        finally:
            cleanup()

    def test_browser_surface_session_info_route_returns_idle_state(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "browser-info-route", "topic": "", "by": "user"})
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
                    resp = client.get(f"/api/v1/groups/{group_id}/presentation/browser_surface/session?slot=slot-1")

            self.assertEqual(resp.status_code, 200)
            payload = resp.json()
            self.assertTrue(bool(payload.get("ok")))
            self.assertEqual(payload["result"]["browser_surface"]["state"], "idle")
        finally:
            cleanup()

    def test_browser_surface_session_close_route_calls_daemon(self) -> None:
        _, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "browser-close-route", "topic": "", "by": "user"})
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
            ):
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
                            f"/api/v1/groups/{group_id}/presentation/browser_surface/session/close",
                            json={"slot": "slot-4", "by": "user"},
                        )

            self.assertEqual(resp.status_code, 200)
            payload = resp.json()
            self.assertTrue(bool(payload.get("ok")))
            self.assertTrue(bool(payload["result"]["closed"]))
        finally:
            cleanup()

    def test_browser_surface_websocket_uses_large_stream_limit(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.ports.web.routes import groups as groups_routes

            create, _ = self._call("group_create", {"title": "browser-ws-limit", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            group_id = str((create.result or {}).get("group_id") or "")
            captured: dict[str, object] = {}

            async def fake_open_connection(host: str, port: int, *, limit: int | None = None):
                captured["host"] = host
                captured["port"] = port
                captured["limit"] = limit
                raise RuntimeError("simulated connect failure")

            with patch("cccc.ports.web.routes.groups.get_daemon_endpoint", return_value={"transport": "tcp", "host": "127.0.0.1", "port": 9001}), patch(
                "cccc.ports.web.routes.groups.check_group",
                return_value=None,
            ), patch(
                "cccc.ports.web.routes.groups.load_group",
                return_value=SimpleNamespace(group_id=group_id),
            ), patch(
                "cccc.ports.web.routes.groups.asyncio.open_connection",
                side_effect=fake_open_connection,
            ):
                with self._client() as client:
                    with client.websocket_connect(f"/api/v1/groups/{group_id}/presentation/browser_surface/ws?slot=slot-1") as ws:
                        payload = ws.receive_json()

            self.assertEqual(payload["error"]["code"], "daemon_unavailable")
            self.assertEqual(captured.get("limit"), groups_routes._PRESENTATION_BROWSER_STREAM_LIMIT_BYTES)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
