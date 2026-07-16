import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebGroupSettingsPartialUpdate(unittest.TestCase):
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

    def _local_call_daemon(self, req: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        request = DaemonRequest.model_validate(req)
        resp, _ = handle_request(request)
        return resp.model_dump(exclude_none=True)

    def test_delivery_partial_update_preserves_hidden_min_interval(self) -> None:
        from cccc.kernel.group import create_group, load_group
        from cccc.kernel.registry import load_registry
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            reg = load_registry()
            group = create_group(reg, title="delivery-partial", topic="")
            group_id = group.group_id

            loaded = load_group(group_id)
            self.assertIsNotNone(loaded)
            assert loaded is not None
            loaded.doc["delivery"] = {
                "min_interval_seconds": 37,
                "auto_mark_on_delivery": False,
            }
            loaded.save()

            with patch("cccc.ports.web.app.call_daemon", side_effect=self._local_call_daemon):
                app = create_app()
                client = TestClient(app)
                resp = client.put(
                    f"/api/v1/groups/{group_id}/settings",
                    json={"auto_mark_on_delivery": True},
                )
                self.assertEqual(resp.status_code, 200)
                body = resp.json()
                self.assertTrue(body.get("ok"))
                settings = ((body.get("result") or {}).get("settings") or {})

                self.assertEqual(settings.get("min_interval_seconds"), 37)
                self.assertTrue(bool(settings.get("auto_mark_on_delivery")))

            reloaded = load_group(group_id)
            self.assertIsNotNone(reloaded)
            assert reloaded is not None
            delivery = reloaded.doc.get("delivery") if isinstance(reloaded.doc.get("delivery"), dict) else {}
            self.assertEqual(int(delivery.get("min_interval_seconds", -1)), 37)
            self.assertTrue(bool(delivery.get("auto_mark_on_delivery")))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
