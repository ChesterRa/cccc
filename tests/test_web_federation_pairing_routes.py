import os
import tempfile
import unittest
from unittest.mock import patch

from fastapi.testclient import TestClient


class TestWebFederationPairingRoutes(unittest.TestCase):
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

    def _admin_header(self) -> dict:
        from cccc.kernel.access_tokens import create_access_token

        token = create_access_token("admin", is_admin=True)["token"]
        return {"Authorization": f"Bearer {token}"}

    def _scoped_header(self, groups: list[str]) -> dict:
        from cccc.kernel.access_tokens import create_access_token

        token = create_access_token("scoped-user", is_admin=False, allowed_groups=groups)["token"]
        return {"Authorization": f"Bearer {token}"}

    def test_pairing_identity_invite_request_approve_flow(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            headers = self._admin_header()

            identity = client.get("/api/federation/pairing/identity", headers=headers)
            self.assertEqual(identity.status_code, 200, identity.text)
            self.assertTrue(identity.json()["result"]["identity"]["peer_id"].startswith("12D3Koo"))

            invite = client.post(
                "/api/federation/pairing/invites",
                json={
                    "group_id": "g_local",
                    "remote_group_id": "g_remote",
                    "remote_peer_id": "peer_remote",
                    "multiaddrs": ["/ip4/127.0.0.1/tcp/4001/p2p/peer_remote"],
                    "ttl_seconds": 600,
                },
                headers=headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]
            self.assertRegex(invite_body["pairing_code"], r"^[A-Z0-9]{4}-[A-Z0-9]{4}$")

            request = client.post(
                "/api/federation/pairing/requests",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "requester_group_id": "g_remote",
                    "requester_peer_id": "peer_remote",
                    "requester_multiaddrs": ["/ip4/127.0.0.1/tcp/4001/p2p/peer_remote"],
                },
                headers=headers,
            )
            self.assertEqual(request.status_code, 200, request.text)
            req_body = request.json()["result"]["request"]
            self.assertEqual(req_body["status"], "pending")

            listed = client.get("/api/federation/pairing/requests?group_id=g_local", headers=headers)
            self.assertEqual(listed.status_code, 200, listed.text)
            self.assertEqual([x["request_id"] for x in listed.json()["result"]["requests"]], [req_body["request_id"]])

            approved = client.post(
                f"/api/federation/pairing/requests/{req_body['request_id']}/approve",
                json={"approver_user_id": "user-a"},
                headers=headers,
            )
            self.assertEqual(approved.status_code, 200, approved.text)
            registration = approved.json()["result"]["registration"]
            self.assertEqual(registration["transport"], "libp2p_cccc")
            self.assertEqual(registration["remote_peer_id"], "peer_remote")
            self.assertNotIn("credential_ref", registration)
            self.assertNotIn(invite_body["pairing_code"], approved.text)

            trusts = client.get("/api/federation/pairing/trusts?group_id=g_local", headers=headers)
            self.assertEqual(trusts.status_code, 200, trusts.text)
            trust = trusts.json()["result"]["trusts"][0]
            self.assertEqual(trust["registration_id"], registration["registration_id"])

            revoked = client.post(
                f"/api/federation/pairing/trusts/{trust['trust_id']}/revoke",
                json={"revoked_by": "user-a"},
                headers=headers,
            )
            self.assertEqual(revoked.status_code, 200, revoked.text)
            self.assertEqual(revoked.json()["result"]["trust"]["status"], "revoked")

            active_trusts = client.get("/api/federation/pairing/trusts?group_id=g_local", headers=headers)
            self.assertEqual(active_trusts.status_code, 200, active_trusts.text)
            self.assertEqual(active_trusts.json()["result"]["trusts"][0]["status"], "revoked")
            status = client.get("/api/federation/status?group_id=g_local", headers=headers)
            self.assertEqual(status.json()["result"]["registrations"], [])
        finally:
            cleanup()

    def test_pairing_request_create_requires_requester_and_invite_group_access(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            admin_headers = self._admin_header()
            scoped_headers = self._scoped_header(["g_allowed"])
            issuer_and_requester_headers = self._scoped_header(["g_local", "g_allowed"])
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_local"},
                headers=admin_headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]

            denied = client.post(
                "/api/federation/pairing/requests",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "requester_group_id": "g_denied",
                    "requester_peer_id": "peer_remote",
                },
                headers=scoped_headers,
            )
            self.assertEqual(denied.status_code, 403, denied.text)

            denied_issuer = client.post(
                "/api/federation/pairing/requests",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "requester_group_id": "g_allowed",
                    "requester_peer_id": "peer_remote",
                },
                headers=scoped_headers,
            )
            self.assertEqual(denied_issuer.status_code, 403, denied_issuer.text)

            allowed = client.post(
                "/api/federation/pairing/requests",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "requester_group_id": "g_allowed",
                    "requester_peer_id": "peer_remote",
                },
                headers=issuer_and_requester_headers,
            )
            self.assertEqual(allowed.status_code, 200, allowed.text)
            request_body = allowed.json()["result"]["request"]
            self.assertEqual(request_body["group_id"], "g_local")
            self.assertEqual(request_body["remote_group_id"], "g_allowed")
        finally:
            cleanup()

    def test_pairing_connection_info_payload_integrity_is_valid(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.federation.pairing_remote import parse_connection_payload, _validate_integrity

            client = self._client()
            headers = self._admin_header()
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_issuer"},
                headers=headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]

            info = client.post(
                "/api/federation/pairing/connection-info",
                json={
                    "group_id": "g_issuer",
                    "invite_id": invite_body["invite_id"],
                    "issuer_endpoint": "issuer.example/path?ignored=1#frag",
                    "issuer_group_title": "Issuer Group",
                },
                headers=headers,
            )

            self.assertEqual(info.status_code, 200, info.text)
            payload = info.json()["result"]["payload"]
            self.assertEqual(payload["issuer_endpoint"], "https://issuer.example")
            self.assertEqual(payload["issuer_group_id"], "g_issuer")
            self.assertEqual(payload["issuer_group_title"], "Issuer Group")
            self.assertEqual(payload["code"], invite_body["pairing_code"])
            self.assertEqual(payload["nonce"], invite_body["invite_id"])
            self.assertTrue(str(payload["integrity"]).startswith("sha256:"))
            parsed = parse_connection_payload(payload)
            _validate_integrity(parsed)
            self.assertNotIn("pairing_code_hash", info.text)
        finally:
            cleanup()

    def test_pairing_connection_info_requires_invite_group_access_and_match(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            admin_headers = self._admin_header()
            scoped_headers = self._scoped_header(["g_allowed"])
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_secret"},
                headers=admin_headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]

            denied_group = client.post(
                "/api/federation/pairing/connection-info",
                json={
                    "group_id": "g_secret",
                    "invite_id": invite_body["invite_id"],
                    "issuer_endpoint": "https://issuer.example",
                    "issuer_group_title": "Secret",
                },
                headers=scoped_headers,
            )
            self.assertEqual(denied_group.status_code, 403, denied_group.text)

            mismatch = client.post(
                "/api/federation/pairing/connection-info",
                json={
                    "group_id": "g_allowed",
                    "invite_id": invite_body["invite_id"],
                    "issuer_endpoint": "https://issuer.example",
                    "issuer_group_title": "Allowed",
                },
                headers=scoped_headers,
            )
            self.assertEqual(mismatch.status_code, 403, mismatch.text)
            self.assertNotIn(invite_body["pairing_code"], mismatch.text)
        finally:
            cleanup()

    def test_pairing_connection_info_rejects_requested_invite_without_leaking_code(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            headers = self._admin_header()
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_issuer"},
                headers=headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]
            request = client.post(
                "/api/federation/pairing/requests",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                    "invite_id": invite_body["invite_id"],
                },
                headers=headers,
            )
            self.assertEqual(request.status_code, 200, request.text)

            info = client.post(
                "/api/federation/pairing/connection-info",
                json={
                    "group_id": "g_issuer",
                    "invite_id": invite_body["invite_id"],
                    "issuer_endpoint": "https://issuer.example",
                    "issuer_group_title": "Issuer",
                },
                headers=headers,
            )

            self.assertEqual(info.status_code, 400, info.text)
            self.assertIn("pending", info.text)
            self.assertNotIn(invite_body["pairing_code"], info.text)
            self.assertNotIn("pairing_code_hash", info.text)
        finally:
            cleanup()

    def test_pairing_connection_info_rejects_expired_invite_without_leaking_code(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            headers = self._admin_header()
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_issuer", "ttl_seconds": -1},
                headers=headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]

            info = client.post(
                "/api/federation/pairing/connection-info",
                json={
                    "group_id": "g_issuer",
                    "invite_id": invite_body["invite_id"],
                    "issuer_endpoint": "https://issuer.example",
                    "issuer_group_title": "Issuer",
                },
                headers=headers,
            )

            self.assertEqual(info.status_code, 400, info.text)
            self.assertIn("expired", info.text)
            self.assertNotIn(invite_body["pairing_code"], info.text)
            self.assertNotIn("pairing_code_hash", info.text)

            listed = client.get("/api/federation/pairing/requests?group_id=g_issuer", headers=headers)
            self.assertEqual(listed.status_code, 200, listed.text)
            self.assertEqual(listed.json()["result"]["requests"], [])
        finally:
            cleanup()

    def test_same_instance_group_connection_info_code_creates_request(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            admin_headers = self._admin_header()
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_issuer"},
                headers=admin_headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]

            request = client.post(
                "/api/federation/pairing/requests",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                },
                headers=admin_headers,
            )
            self.assertEqual(request.status_code, 200, request.text)
            request_body = request.json()["result"]["request"]
            self.assertEqual(request_body["group_id"], "g_issuer")
            self.assertEqual(request_body["remote_group_id"], "g_joiner")
            self.assertEqual(request_body["remote_peer_id"], "peer_joiner")
        finally:
            cleanup()

    def test_remote_issuer_endpoint_accepts_code_and_nonce_without_bearer(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            admin_headers = self._admin_header()
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_issuer"},
                headers=admin_headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]

            request = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "invite_id": invite_body["invite_id"],
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                },
            )
            self.assertEqual(request.status_code, 200, request.text)
            request_body = request.json()["result"]["request"]
            self.assertEqual(request_body["group_id"], "g_issuer")
            self.assertEqual(request_body["remote_group_id"], "g_joiner")
        finally:
            cleanup()

    def test_remote_status_endpoint_requires_request_and_invite_match(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            admin_headers = self._admin_header()
            invite = client.post("/api/federation/pairing/invites", json={"group_id": "g_issuer"}, headers=admin_headers)
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]
            request = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "invite_id": invite_body["invite_id"],
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                },
            )
            self.assertEqual(request.status_code, 200, request.text)
            request_body = request.json()["result"]["request"]

            mismatch = client.get(
                f"/api/federation/pairing/requests/remote/status?request_id={request_body['request_id']}&invite_id=pinv_wrong"
            )
            self.assertEqual(mismatch.status_code, 404, mismatch.text)

            public_client = self._client()
            status = public_client.get(
                f"/api/federation/pairing/requests/remote/status?request_id={request_body['request_id']}&invite_id={invite_body['invite_id']}"
            )
            self.assertEqual(status.status_code, 200, status.text)
            self.assertEqual(status.json()["result"]["request"]["status"], "pending")
            self.assertNotIn(invite_body["pairing_code"], status.text)
            self.assertNotIn("pairing_code_hash", status.text)
        finally:
            cleanup()

    def test_remote_status_endpoint_is_public_when_tokens_exist(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.access_tokens import create_access_token

            client = self._client()
            admin_headers = self._admin_header()
            create_access_token("other-user", is_admin=False, allowed_groups=["g_other"])
            invite = client.post("/api/federation/pairing/invites", json={"group_id": "g_issuer"}, headers=admin_headers)
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]
            request = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "invite_id": invite_body["invite_id"],
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                },
            )
            self.assertEqual(request.status_code, 200, request.text)
            request_body = request.json()["result"]["request"]

            status = client.get(
                f"/api/federation/pairing/requests/remote/status?request_id={request_body['request_id']}&invite_id={invite_body['invite_id']}"
            )

            self.assertEqual(status.status_code, 200, status.text)
            self.assertEqual(status.json()["result"]["request"]["status"], "pending")
        finally:
            cleanup()

    def test_remote_issuer_endpoint_is_schema_and_code_state_bounded(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.federation.registration import list_registrations
            from cccc.kernel.federation.pairing import list_pairing_requests

            client = self._client()
            admin_headers = self._admin_header()
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_issuer", "ttl_seconds": 600},
                headers=admin_headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]

            extra = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "invite_id": invite_body["invite_id"],
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                    "registration_id": "should_not_be_accepted",
                },
            )
            self.assertEqual(extra.status_code, 422, extra.text)

            missing_nonce = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                },
            )
            self.assertEqual(missing_nonce.status_code, 422, missing_nonce.text)
            self.assertEqual(list_pairing_requests(group_id="g_issuer"), [])

            bad_nonce = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "invite_id": "pinv_wrong",
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                },
            )
            self.assertEqual(bad_nonce.status_code, 400, bad_nonce.text)
            self.assertNotIn(invite_body["pairing_code"], bad_nonce.text)
            self.assertNotIn("pairing_code_hash", bad_nonce.text)

            accepted = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "invite_id": invite_body["invite_id"],
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                },
            )
            self.assertEqual(accepted.status_code, 200, accepted.text)
            self.assertEqual(list_registrations(), [])
            self.assertEqual(client.get("/api/federation/pairing/trusts?group_id=g_issuer", headers=admin_headers).json()["result"]["trusts"], [])

            replay = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "invite_id": invite_body["invite_id"],
                    "requester_group_id": "g_joiner2",
                    "requester_peer_id": "peer_joiner2",
                },
            )
            self.assertEqual(replay.status_code, 400, replay.text)
            self.assertNotIn(invite_body["pairing_code"], replay.text)
        finally:
            cleanup()

    def test_remote_issuer_endpoint_rejects_oversized_public_body_fields(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()

            oversized = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": "A" * 4096,
                    "invite_id": "pinv_1",
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                    "requester_multiaddrs": ["/ip4/127.0.0.1/tcp/4001"] * 65,
                },
            )
            self.assertEqual(oversized.status_code, 422, oversized.text)
            self.assertNotIn("pairing_code_hash", oversized.text)
        finally:
            cleanup()

    def test_remote_issuer_endpoint_expired_code_does_not_leak_raw_code(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            admin_headers = self._admin_header()
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_issuer", "ttl_seconds": -1},
                headers=admin_headers,
            )
            self.assertEqual(invite.status_code, 200, invite.text)
            invite_body = invite.json()["result"]["invite"]

            expired = client.post(
                "/api/federation/pairing/requests/remote",
                json={
                    "pairing_code": invite_body["pairing_code"],
                    "invite_id": invite_body["invite_id"],
                    "requester_group_id": "g_joiner",
                    "requester_peer_id": "peer_joiner",
                },
            )
            self.assertEqual(expired.status_code, 400, expired.text)
            self.assertIn("expired", expired.text)
            self.assertNotIn(invite_body["pairing_code"], expired.text)
            self.assertNotIn("pairing_code_hash", expired.text)
        finally:
            cleanup()

    def test_join_remote_request_requires_local_group_access_and_records_outbound(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            scoped_headers = self._scoped_header(["g_joiner"])

            def fake_submit(payload, *, local_group_id, local_group_title="", home=None, **_kwargs):
                self.assertEqual(payload["issuer_group_id"], "g_issuer")
                self.assertEqual(local_group_id, "g_joiner")
                return {"outbound_id": "pout_1", "status": "submitted", "issuer_group_id": "g_issuer"}

            with patch("cccc.ports.web.routes.federation.submit_remote_pairing_request", side_effect=fake_submit):
                denied = client.post(
                    "/api/federation/pairing/remote-requests",
                    json={"payload": {"issuer_group_id": "g_issuer", "issuer_endpoint": "https://issuer.example", "code": "ABCD-1234"}, "local_group_id": "g_denied"},
                    headers=scoped_headers,
                )
                self.assertEqual(denied.status_code, 403, denied.text)

                submitted = client.post(
                    "/api/federation/pairing/remote-requests",
                    json={"payload": {"issuer_group_id": "g_issuer", "issuer_endpoint": "https://issuer.example", "code": "ABCD-1234"}, "local_group_id": "g_joiner"},
                    headers=scoped_headers,
                )
                self.assertEqual(submitted.status_code, 200, submitted.text)
                self.assertEqual(submitted.json()["result"]["outbound"]["status"], "submitted")
                outbounds = client.get("/api/federation/pairing/outbounds?group_id=g_joiner", headers=scoped_headers)
                self.assertEqual(outbounds.status_code, 200, outbounds.text)
                self.assertEqual(outbounds.json()["result"]["outbounds"][0]["outbound_id"], "pout_1")
        finally:
            cleanup()

    def test_pairing_outbound_delete_requires_local_group_access(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.federation.pairing import list_pairing_outbounds, upsert_pairing_outbound

            client = self._client()
            allowed_headers = self._scoped_header(["g_joiner"])
            denied_headers = self._scoped_header(["g_other"])
            upsert_pairing_outbound({
                "outbound_id": "pout_failed",
                "local_group_id": "g_joiner",
                "issuer_endpoint": "https://issuer.example",
                "issuer_group_id": "g_issuer",
                "issuer_peer_id": "peer_issuer",
                "invite_id": "pinv_1",
                "status": "failed",
            })

            denied = client.post("/api/federation/pairing/outbounds/pout_failed/delete", headers=denied_headers)
            self.assertEqual(denied.status_code, 403, denied.text)
            self.assertEqual(len(list_pairing_outbounds(group_id="g_joiner")), 1)

            deleted = client.post("/api/federation/pairing/outbounds/pout_failed/delete", headers=allowed_headers)
            self.assertEqual(deleted.status_code, 200, deleted.text)
            self.assertTrue(deleted.json()["result"]["deleted"])
            self.assertEqual(list_pairing_outbounds(group_id="g_joiner"), [])
        finally:
            cleanup()

    def test_remote_submit_persists_joiner_outbound_and_issuer_inbound_pending(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.federation.pairing import create_pairing_invite, create_pairing_request
            from cccc.kernel.federation.pairing_remote import build_connection_payload

            client = self._client()
            headers = self._admin_header()
            invite = create_pairing_invite(group_id="g_issuer")
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555")

            def fake_submit(payload_arg, *, local_group_id, local_group_title="", home=None, **_kwargs):
                request = create_pairing_request(
                    payload_arg["code"],
                    requester_group_id=local_group_id,
                    requester_peer_id="peer_joiner",
                    invite_id=payload_arg["nonce"],
                )
                return {
                    "outbound_id": "pout_joiner",
                    "status": "submitted",
                    "local_group_id": local_group_id,
                    "issuer_endpoint": payload_arg["issuer_endpoint"],
                    "issuer_group_id": payload_arg["issuer_group_id"],
                    "issuer_peer_id": payload_arg["issuer_peer_id"],
                    "invite_id": payload_arg["nonce"],
                    "remote_request": request,
                    "last_error": "",
                    "updated_at": "2026-06-15T00:00:00Z",
                }

            with patch("cccc.ports.web.routes.federation.submit_remote_pairing_request", side_effect=fake_submit):
                submitted = client.post(
                    "/api/federation/pairing/remote-requests",
                    json={"payload": payload, "local_group_id": "g_joiner", "local_group_title": "Joiner"},
                    headers=headers,
                )

            self.assertEqual(submitted.status_code, 200, submitted.text)
            issuer_pending = client.get("/api/federation/pairing/requests?group_id=g_issuer", headers=headers)
            self.assertEqual(issuer_pending.status_code, 200, issuer_pending.text)
            self.assertEqual(issuer_pending.json()["result"]["requests"][0]["status"], "pending")
            joiner_outbounds = client.get("/api/federation/pairing/outbounds?group_id=g_joiner", headers=headers)
            self.assertEqual(joiner_outbounds.status_code, 200, joiner_outbounds.text)
            self.assertEqual(joiner_outbounds.json()["result"]["outbounds"][0]["status"], "submitted")
        finally:
            cleanup()

    def test_joiner_outbound_sync_after_issuer_approve_creates_local_trust(self) -> None:
        _, cleanup = self._with_home()
        try:
            from cccc.kernel.federation.pairing import (
                approve_pairing_request,
                create_pairing_invite,
                create_pairing_request,
                get_pairing_request,
            )
            from cccc.kernel.federation.pairing_remote import build_connection_payload

            client = self._client()
            headers = self._admin_header()
            invite = create_pairing_invite(group_id="g_issuer")
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555")

            def fake_submit(payload_arg, *, local_group_id, local_group_title="", home=None, **_kwargs):
                request = create_pairing_request(
                    payload_arg["code"],
                    requester_group_id=local_group_id,
                    requester_peer_id="peer_joiner",
                    invite_id=payload_arg["nonce"],
                )
                return {
                    "outbound_id": "pout_joiner",
                    "status": "submitted",
                    "local_group_id": local_group_id,
                    "issuer_endpoint": payload_arg["issuer_endpoint"],
                    "issuer_group_id": payload_arg["issuer_group_id"],
                    "issuer_peer_id": payload_arg["issuer_peer_id"],
                    "invite_id": payload_arg["nonce"],
                    "remote_request": request,
                    "last_error": "",
                    "updated_at": "2026-06-15T00:00:00Z",
                }

            with patch("cccc.ports.web.routes.federation.submit_remote_pairing_request", side_effect=fake_submit):
                submitted = client.post(
                    "/api/federation/pairing/remote-requests",
                    json={"payload": payload, "local_group_id": "g_joiner", "local_group_title": "Joiner"},
                    headers=headers,
                )
            self.assertEqual(submitted.status_code, 200, submitted.text)
            remote_request = submitted.json()["result"]["outbound"]["remote_request"]

            approve_pairing_request(remote_request["request_id"], approver_user_id="issuer-admin")

            def fake_sync(outbound_id, *, home=None, **_kwargs):
                self.assertEqual(outbound_id, "pout_joiner")
                from cccc.kernel.federation.pairing_outbound_sync import approve_outbound_from_remote_request

                approved_request = get_pairing_request(remote_request["request_id"])
                return approve_outbound_from_remote_request(outbound_id, approved_request)["outbound"]

            with patch("cccc.ports.web.routes.federation.sync_remote_pairing_outbound", side_effect=fake_sync):
                synced = client.post("/api/federation/pairing/outbounds/pout_joiner/sync", headers=headers)
            self.assertEqual(synced.status_code, 200, synced.text)
            self.assertEqual(synced.json()["result"]["outbound"]["status"], "approved")

            outbounds = client.get("/api/federation/pairing/outbounds?group_id=g_joiner", headers=headers)
            self.assertEqual(outbounds.status_code, 200, outbounds.text)
            self.assertEqual(outbounds.json()["result"]["outbounds"][0]["status"], "approved")

            trusts = client.get("/api/federation/pairing/trusts?group_id=g_joiner", headers=headers)
            self.assertEqual(trusts.status_code, 200, trusts.text)
            trust = trusts.json()["result"]["trusts"][0]
            self.assertEqual(trust["status"], "active")
            self.assertEqual(trust["remote_group_id"], "g_issuer")
            self.assertEqual(trust["remote_peer_id"], payload["issuer_peer_id"])
            self.assertEqual(trust["remote_endpoint"], "http://127.0.0.1:5555")
            self.assertEqual(trust["transport"], "peer_cccc_http")

            status = client.get("/api/federation/status?group_id=g_joiner", headers=headers)
            self.assertEqual(status.status_code, 200, status.text)
            registration = status.json()["result"]["registrations"][0]
            self.assertEqual(registration["transport"], "peer_cccc_http")
            self.assertEqual(registration["url"], "http://127.0.0.1:5555")
            self.assertEqual(registration["remote_group_id"], "g_issuer")
            self.assertEqual(registration["remote_peer_id"], payload["issuer_peer_id"])
        finally:
            cleanup()

    def test_pairing_request_list_without_group_id_filters_scoped_token(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            admin_headers = self._admin_header()
            scoped_headers = self._scoped_header(["g_allowed"])

            for group_id, remote_group_id in (("g_allowed", "g_remote_allowed"), ("g_secret", "g_remote_secret")):
                invite = client.post(
                    "/api/federation/pairing/invites",
                    json={"group_id": group_id},
                    headers=admin_headers,
                )
                self.assertEqual(invite.status_code, 200, invite.text)
                invite_body = invite.json()["result"]["invite"]
                request = client.post(
                    "/api/federation/pairing/requests",
                    json={
                        "pairing_code": invite_body["pairing_code"],
                        "requester_group_id": remote_group_id,
                        "requester_peer_id": f"peer_{remote_group_id}",
                    },
                    headers=admin_headers,
                )
                self.assertEqual(request.status_code, 200, request.text)

            scoped_all = client.get("/api/federation/pairing/requests", headers=scoped_headers)
            self.assertEqual(scoped_all.status_code, 200, scoped_all.text)
            self.assertEqual({item["group_id"] for item in scoped_all.json()["result"]["requests"]}, {"g_allowed"})

            allowed = client.get("/api/federation/pairing/requests?group_id=g_allowed", headers=scoped_headers)
            self.assertEqual(allowed.status_code, 200, allowed.text)

            denied = client.get("/api/federation/pairing/requests?group_id=g_secret", headers=scoped_headers)
            self.assertEqual(denied.status_code, 403, denied.text)
        finally:
            cleanup()

    def test_pairing_trust_list_without_group_id_filters_scoped_token(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            admin_headers = self._admin_header()
            scoped_headers = self._scoped_header(["g_allowed"])

            for group_id, remote_group_id in (("g_allowed", "g_remote_allowed"), ("g_secret", "g_remote_secret")):
                invite = client.post(
                    "/api/federation/pairing/invites",
                    json={"group_id": group_id},
                    headers=admin_headers,
                )
                self.assertEqual(invite.status_code, 200, invite.text)
                invite_body = invite.json()["result"]["invite"]
                request = client.post(
                    "/api/federation/pairing/requests",
                    json={
                        "pairing_code": invite_body["pairing_code"],
                        "requester_group_id": remote_group_id,
                        "requester_peer_id": f"peer_{remote_group_id}",
                    },
                    headers=admin_headers,
                )
                self.assertEqual(request.status_code, 200, request.text)
                request_body = request.json()["result"]["request"]
                approved = client.post(
                    f"/api/federation/pairing/requests/{request_body['request_id']}/approve",
                    json={"approver_user_id": "admin"},
                    headers=admin_headers,
                )
                self.assertEqual(approved.status_code, 200, approved.text)

            scoped_all = client.get("/api/federation/pairing/trusts", headers=scoped_headers)
            self.assertEqual(scoped_all.status_code, 200, scoped_all.text)
            self.assertEqual({item["group_id"] for item in scoped_all.json()["result"]["trusts"]}, {"g_allowed"})

            allowed = client.get("/api/federation/pairing/trusts?group_id=g_allowed", headers=scoped_headers)
            self.assertEqual(allowed.status_code, 200, allowed.text)

            denied = client.get("/api/federation/pairing/trusts?group_id=g_secret", headers=scoped_headers)
            self.assertEqual(denied.status_code, 403, denied.text)
        finally:
            cleanup()

    def test_reject_blocks_approve(self) -> None:
        _, cleanup = self._with_home()
        try:
            client = self._client()
            headers = self._admin_header()
            invite = client.post(
                "/api/federation/pairing/invites",
                json={"group_id": "g_local", "remote_group_id": "g_remote", "remote_peer_id": "peer_remote"},
                headers=headers,
            ).json()["result"]["invite"]
            request = client.post(
                "/api/federation/pairing/requests",
                json={
                    "pairing_code": invite["pairing_code"],
                    "requester_group_id": "g_remote",
                    "requester_peer_id": "peer_remote",
                },
                headers=headers,
            ).json()["result"]["request"]

            rejected = client.post(
                f"/api/federation/pairing/requests/{request['request_id']}/reject",
                json={"rejected_by": "user-a", "reason": "no"},
                headers=headers,
            )
            self.assertEqual(rejected.status_code, 200, rejected.text)
            self.assertEqual(rejected.json()["result"]["request"]["status"], "rejected")

            approved = client.post(
                f"/api/federation/pairing/requests/{request['request_id']}/approve",
                json={"approver_user_id": "user-a"},
                headers=headers,
            )
            self.assertEqual(approved.status_code, 400)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
