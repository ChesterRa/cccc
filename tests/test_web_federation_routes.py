import os
import tempfile
import unittest

from fastapi.testclient import TestClient


class TestWebFederationRoutes(unittest.TestCase):
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

    def _tokens(self):
        from cccc.kernel.access_tokens import create_access_token

        admin = create_access_token("admin", is_admin=True)["token"]
        scoped = create_access_token("scoped-user", allowed_groups=["g_local"], is_admin=False)["token"]
        return admin, scoped

    @staticmethod
    def _hdr(token: str) -> dict:
        return {"Authorization": f"Bearer {token}"}

    def test_register_status_unregister_happy_path(self) -> None:
        _, cleanup = self._with_home()
        try:
            admin, _ = self._tokens()
            client = self._client()

            r = client.post(
                "/api/federation/register",
                json={"group_id": "g_local", "url": "HTTPS://hub.example:443/", "remote_group_id": "g_remote", "credential_ref": "sec_remote_peer"},
                headers=self._hdr(admin),
            )
            self.assertEqual(r.status_code, 200, r.text)
            rec = r.json()["result"]["registration"]
            self.assertEqual(rec["url"], "https://hub.example")
            self.assertEqual(rec["group_id"], "g_local")
            self.assertNotIn("token", rec)
            self.assertNotIn("credential_ref", rec)
            rid = rec["registration_id"]

            s = client.get("/api/federation/status", headers=self._hdr(admin))
            self.assertEqual(s.status_code, 200)
            regs = s.json()["result"]["registrations"]
            self.assertEqual([x["registration_id"] for x in regs], [rid])
            self.assertNotIn("credential_ref", regs[0])
            self.assertNotIn("sec_remote_peer", s.text)

            u = client.post("/api/federation/unregister", json={"registration_id": rid}, headers=self._hdr(admin))
            self.assertEqual(u.status_code, 200)
            self.assertTrue(u.json()["result"]["deleted"])
        finally:
            cleanup()

    def test_verify_rejects_unauthorized_group(self) -> None:
        _, cleanup = self._with_home()
        try:
            _, scoped = self._tokens()  # scoped only to g_local
            client = self._client()
            ok = client.post(
                "/api/federation/verify",
                json={"group_id": "g_local", "url": "https://hub.example/"},
                headers=self._hdr(scoped),
            )
            self.assertEqual(ok.status_code, 200, ok.text)

            denied = client.post(
                "/api/federation/verify",
                json={"group_id": "g_forbidden", "url": "https://hub.example/"},
                headers=self._hdr(scoped),
            )
            self.assertEqual(denied.status_code, 403)
        finally:
            cleanup()

    def test_register_rejects_unauthorized_group(self) -> None:
        _, cleanup = self._with_home()
        try:
            _, scoped = self._tokens()
            client = self._client()
            r = client.post(
                "/api/federation/register",
                json={"group_id": "g_forbidden", "url": "https://hub.example/"},
                headers=self._hdr(scoped),
            )
            self.assertEqual(r.status_code, 403)
        finally:
            cleanup()

    def test_register_rejects_token_shaped_credential_ref(self) -> None:
        _, cleanup = self._with_home()
        try:
            admin, _ = self._tokens()
            client = self._client()
            r = client.post(
                "/api/federation/register",
                json={"group_id": "g_local", "url": "https://hub.example/", "credential_ref": "acc_deadbeefdeadbeef"},
                headers=self._hdr(admin),
            )
            self.assertEqual(r.status_code, 400)
            self.assertNotIn("acc_deadbeefdeadbeef", r.text)
        finally:
            cleanup()

    def test_register_rejects_raw_credential_ref_without_echoing_secret(self) -> None:
        _, cleanup = self._with_home()
        try:
            admin, _ = self._tokens()
            client = self._client()
            raw = "ghp_1234567890abcdef1234567890abcdef123456"
            r = client.post(
                "/api/federation/register",
                json={"group_id": "g_local", "url": "https://hub.example/", "remote_group_id": "g_remote", "credential_ref": raw},
                headers=self._hdr(admin),
            )
            self.assertEqual(r.status_code, 400)
            self.assertNotIn(raw, r.text)
        finally:
            cleanup()

    def test_register_accepts_credential_reference(self) -> None:
        _, cleanup = self._with_home()
        try:
            admin, _ = self._tokens()
            client = self._client()
            r = client.post(
                "/api/federation/register",
                json={"group_id": "g_local", "url": "https://hub.example/", "remote_group_id": "g_remote", "credential_ref": "fsec_remote_peer"},
                headers=self._hdr(admin),
            )
            self.assertEqual(r.status_code, 200, r.text)
            self.assertNotIn("fsec_remote_peer", r.text)
        finally:
            cleanup()

    def test_register_rejects_direct_libp2p_registration(self) -> None:
        _, cleanup = self._with_home()
        try:
            admin, _ = self._tokens()
            client = self._client()
            r = client.post(
                "/api/federation/register",
                json={
                    "group_id": "g_local",
                    "url": "libp2p://peer-remote",
                    "transport": "libp2p_cccc",
                    "remote_group_id": "g_remote",
                    "remote_peer_id": "peer-remote",
                    "multiaddrs": ["/ip4/127.0.0.1/tcp/4001/p2p/peer-remote"],
                    "credential_ref": "fsec_remote_peer",
                },
                headers=self._hdr(admin),
            )
            self.assertEqual(r.status_code, 400)
            self.assertIn("pairing", r.text)
            self.assertNotIn("fsec_remote_peer", r.text)
        finally:
            cleanup()

    def test_status_filters_to_allowed_groups(self) -> None:
        _, cleanup = self._with_home()
        try:
            admin, scoped = self._tokens()
            client = self._client()
            client.post("/api/federation/register", json={"group_id": "g_local", "url": "https://a/", "remote_group_id": "g_ra"}, headers=self._hdr(admin))
            client.post("/api/federation/register", json={"group_id": "g_other", "url": "https://b/", "remote_group_id": "g_rb"}, headers=self._hdr(admin))

            # admin sees both; scoped sees only g_local
            admin_regs = client.get("/api/federation/status", headers=self._hdr(admin)).json()["result"]["registrations"]
            self.assertEqual({x["group_id"] for x in admin_regs}, {"g_local", "g_other"})

            scoped_regs = client.get("/api/federation/status", headers=self._hdr(scoped)).json()["result"]["registrations"]
            self.assertEqual({x["group_id"] for x in scoped_regs}, {"g_local"})
        finally:
            cleanup()

    def test_delivery_status_group_scope_and_no_token_leak(self) -> None:
        from cccc.daemon.federation.remote_dispatch import enqueue_remote_send

        _, cleanup = self._with_home()
        try:
            admin, scoped = self._tokens()
            client = self._client()
            rec = client.post(
                "/api/federation/register",
                json={"group_id": "g_other", "url": "https://hub/", "remote_group_id": "g_r"},
                headers=self._hdr(admin),
            ).json()["result"]["registration"]
            rid = rec["registration_id"]
            enqueue_remote_send(src_group_id="g_other", registration_id=rid, idempotency_key="k1", payload={"text": "hi"})

            # admin (authorized for g_other) reads the receipt
            ok = client.get(f"/api/federation/registrations/{rid}/deliveries/k1", headers=self._hdr(admin))
            self.assertEqual(ok.status_code, 200, ok.text)
            receipt = ok.json()["result"]["receipt"]
            self.assertEqual(receipt["status"], "queued")
            self.assertNotIn("acc_", ok.text)

            # scoped user (only g_local) must be denied this g_other registration
            denied = client.get(f"/api/federation/registrations/{rid}/deliveries/k1", headers=self._hdr(scoped))
            self.assertEqual(denied.status_code, 403)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
