import os
import tempfile
import unittest
from pathlib import Path

from fastapi.testclient import TestClient


class TestWebGroupBridgeRoutes(unittest.TestCase):
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

        return Path(td), cleanup

    def _client(self) -> TestClient:
        from cccc.ports.web.app import create_app

        return TestClient(create_app())

    def _tokens(self):
        from cccc.kernel.access_tokens import create_access_token

        admin = create_access_token("admin", is_admin=True)["token"]
        scoped = create_access_token("scoped-user", allowed_groups=["g_local"], is_admin=False)["token"]
        return admin, scoped

    @staticmethod
    def _hdr(token: str) -> dict:
        return {"Authorization": f"Bearer {token}"}

    def test_direct_verify_and_register_routes_are_not_exposed(self) -> None:
        _, cleanup = self._with_home()
        try:
            admin, _ = self._tokens()
            client = self._client()

            verify = client.post(
                "/api/group-bridge/verify",
                json={"group_id": "g_local", "url": "https://hub.example/"},
                headers=self._hdr(admin),
            )
            register = client.post(
                "/api/group-bridge/register",
                json={"group_id": "g_local", "url": "https://hub.example/", "remote_group_id": "g_remote"},
                headers=self._hdr(admin),
            )

            self.assertEqual(verify.status_code, 404)
            self.assertEqual(register.status_code, 404)
        finally:
            cleanup()

    def test_status_filters_pairing_created_registrations_to_allowed_groups(self) -> None:
        from cccc.kernel.group_bridge.pairing import _upsert_approved_session_registration

        home, cleanup = self._with_home()
        try:
            admin, scoped = self._tokens()
            client = self._client()
            _upsert_approved_session_registration(
                "g_local",
                "https://a.example",
                remote_group_id="g_ra",
                remote_peer_id="peer-a",
                home=home,
            )
            _upsert_approved_session_registration(
                "g_other",
                "https://b.example",
                remote_group_id="g_rb",
                remote_peer_id="peer-b",
                home=home,
            )

            admin_regs = client.get("/api/group-bridge/status", headers=self._hdr(admin)).json()["result"]["registrations"]
            self.assertEqual({x["group_id"] for x in admin_regs}, {"g_local", "g_other"})
            self.assertTrue(all("credential_ref" not in x for x in admin_regs))

            scoped_regs = client.get("/api/group-bridge/status", headers=self._hdr(scoped)).json()["result"]["registrations"]
            self.assertEqual({x["group_id"] for x in scoped_regs}, {"g_local"})
        finally:
            cleanup()

    def test_delivery_status_group_scope_and_no_secret_leak(self) -> None:
        from cccc.daemon.group_bridge.remote_dispatch import enqueue_remote_send
        from cccc.kernel.group_bridge.pairing import _upsert_approved_session_registration

        home, cleanup = self._with_home()
        try:
            admin, scoped = self._tokens()
            client = self._client()
            rec = _upsert_approved_session_registration(
                "g_other",
                "https://remote.example",
                remote_group_id="g_r",
                remote_peer_id="peer-r",
                home=home,
            )
            rid = rec["registration_id"]
            enqueue_remote_send(src_group_id="g_other", registration_id=rid, idempotency_key="k1", payload={"text": "hi"}, home=home)

            ok = client.get(f"/api/group-bridge/registrations/{rid}/deliveries/k1", headers=self._hdr(admin))
            self.assertEqual(ok.status_code, 200, ok.text)
            receipt = ok.json()["result"]["receipt"]
            self.assertEqual(receipt["status"], "queued")
            self.assertNotIn("acc_", ok.text)

            denied = client.get(f"/api/group-bridge/registrations/{rid}/deliveries/k1", headers=self._hdr(scoped))
            self.assertEqual(denied.status_code, 403)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
