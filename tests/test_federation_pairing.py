import os
import json
import tempfile
import unittest
from pathlib import Path


class TestFederationPairing(unittest.TestCase):
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

    def test_local_identity_is_stable_and_secret_free(self) -> None:
        from cccc.kernel.federation.pairing import get_local_identity

        _, cleanup = self._with_home()
        try:
            first = get_local_identity()
            second = get_local_identity()

            self.assertEqual(first["node_id"], second["node_id"])
            self.assertEqual(first["peer_id"], second["peer_id"])
            self.assertTrue(first["node_id"].startswith("node_"))
            self.assertTrue(first["peer_id"].startswith("12D3Koo"))
            self.assertNotIn("secret", first)
            self.assertNotIn("private_key", first)
        finally:
            cleanup()

    def test_invite_request_approve_creates_federation_session_registration(self) -> None:
        from cccc.kernel.federation.pairing import (
            approve_pairing_request,
            create_pairing_invite,
            create_pairing_request,
            get_pairing_request,
            list_trusts,
            revoke_trust,
        )
        from cccc.kernel.federation.registration import get_registration

        _, cleanup = self._with_home()
        try:
            invite = create_pairing_invite(
                group_id="g_local",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
                ttl_seconds=600,
            )

            self.assertTrue(invite["invite_id"].startswith("pinv_"))
            self.assertRegex(invite["pairing_code"], r"^[A-Z0-9]{4}-[A-Z0-9]{4}$")
            self.assertEqual(invite["status"], "pending")
            self.assertEqual(invite["transport"], "federation_session")
            pairing_store = Path(os.environ["CCCC_HOME"]) / "federation_pairing.yaml"
            persisted = pairing_store.read_text(encoding="utf-8")
            self.assertNotIn(invite["pairing_code"], persisted)
            self.assertIn("pairing_code_hash", persisted)

            request = create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_remote",
                requester_group_title="Remote Group",
                requester_peer_id="peer_remote",
                requester_endpoint="http://remote.example:8848",
                requester_multiaddrs=["/ip4/127.0.0.1/tcp/4001/p2p/peer_remote"],
            )
            self.assertEqual(request["status"], "pending")
            self.assertEqual(request["invite_id"], invite["invite_id"])
            self.assertEqual(request["multiaddrs"], [])

            stored_request = get_pairing_request(request["request_id"])
            self.assertIsNotNone(stored_request)
            assert stored_request is not None
            self.assertEqual(stored_request["status"], "pending")

            approved = approve_pairing_request(request["request_id"], approver_user_id="user-a")
            self.assertEqual(approved["status"], "approved")
            registration = approved["registration"]
            self.assertEqual(registration["transport"], "federation_session")
            self.assertEqual(registration["group_id"], "g_local")
            self.assertEqual(registration["url"], "http://remote.example:8848")
            self.assertEqual(registration["remote_group_id"], "g_remote")
            self.assertEqual(registration["remote_peer_id"], "peer_remote")
            self.assertEqual(registration["multiaddrs"], [])
            self.assertEqual(registration["status"], "active")
            self.assertEqual(approved["trust"]["transport"], "federation_session")
            self.assertEqual(approved["trust"]["multiaddrs"], [])
            self.assertEqual(approved["trust"]["remote_group_title"], "Remote Group")
            self.assertEqual(approved["trust"]["remote_endpoint"], "http://remote.example:8848")
            stored_registration = get_registration(registration["registration_id"])
            self.assertIsNotNone(stored_registration)
            assert stored_registration is not None
            self.assertEqual(stored_registration["remote_peer_id"], "peer_remote")

            after = get_pairing_request(request["request_id"])
            self.assertIsNotNone(after)
            assert after is not None
            self.assertEqual(after["status"], "approved")
            self.assertEqual(after["registration_id"], registration["registration_id"])
            self.assertEqual(after["approved_by"], "user-a")

            replay = approve_pairing_request(request["request_id"], approver_user_id="user-b")
            self.assertEqual(replay["status"], "approved")
            self.assertEqual(replay["registration"]["registration_id"], registration["registration_id"])

            trusts = list_trusts(group_id="g_local")
            self.assertEqual(len(trusts), 1)
            revoked = revoke_trust(trusts[0]["trust_id"], revoked_by="user-a")
            self.assertEqual(revoked["status"], "revoked")
            self.assertIsNone(get_registration(registration["registration_id"]))
            self.assertEqual(list_trusts(group_id="g_local")[0]["status"], "revoked")
        finally:
            cleanup()

    def test_can_create_approved_federation_session_registration_with_endpoint(self) -> None:
        from cccc.kernel.federation.pairing import _upsert_approved_session_registration  # type: ignore[attr-defined]

        _, cleanup = self._with_home()
        try:
            registration = _upsert_approved_session_registration(
                "g_local",
                "http://remote.example:8848",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
            )

            self.assertEqual(registration["transport"], "federation_session")
            self.assertEqual(registration["url"], "http://remote.example:8848")
            self.assertEqual(registration["group_id"], "g_local")
            self.assertEqual(registration["remote_group_id"], "g_remote")
            self.assertEqual(registration["remote_peer_id"], "peer_remote")
            self.assertEqual(registration["multiaddrs"], [])
        finally:
            cleanup()

    def test_approve_pairing_request_without_remote_endpoint_creates_session_only_registration(self) -> None:
        from cccc.kernel.federation.pairing import approve_pairing_request, create_pairing_invite, create_pairing_request
        from cccc.kernel.federation.registration import list_registrations

        _, cleanup = self._with_home()
        try:
            invite = create_pairing_invite(group_id="g_local", ttl_seconds=600)
            request = create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_remote",
                requester_peer_id="peer_remote",
            )

            approved = approve_pairing_request(request["request_id"], approver_user_id="user-a")

            self.assertEqual(approved["registration"]["transport"], "federation_session")
            self.assertEqual(approved["registration"]["url"], "federation-session://peer_remote")
            self.assertEqual(approved["trust"]["remote_endpoint"], "")
            self.assertEqual(list_registrations()[0]["url"], "federation-session://peer_remote")
        finally:
            cleanup()

    def test_pairing_state_changes_publish_global_events(self) -> None:
        from cccc.kernel.federation.pairing import (
            approve_pairing_request,
            create_pairing_invite,
            create_pairing_request,
            reject_pairing_request,
            revoke_trust,
        )

        home, cleanup = self._with_home()
        try:
            invite = create_pairing_invite(group_id="g_local", ttl_seconds=600)
            request = create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_remote",
                requester_peer_id="peer_remote",
                requester_endpoint="http://remote.example:8848",
            )
            approve_pairing_request(request["request_id"], approver_user_id="user-a")

            second_invite = create_pairing_invite(group_id="g_local", ttl_seconds=600)
            rejected = create_pairing_request(
                second_invite["pairing_code"],
                requester_group_id="g_remote_2",
                requester_peer_id="peer_remote_2",
            )
            reject_pairing_request(rejected["request_id"], rejected_by="user-a")
            approved_trust = approve_pairing_request(request["request_id"], approver_user_id="user-b")["trust"]
            revoke_trust(approved_trust["trust_id"], revoked_by="user-b")

            events_path = home / "daemon" / "ccccd.events.jsonl"
            events = [json.loads(line) for line in events_path.read_text(encoding="utf-8").splitlines()]
            kinds = [event["kind"] for event in events]
            self.assertIn("federation.pairing.invite_created", kinds)
            self.assertIn("federation.pairing.request_created", kinds)
            self.assertIn("federation.pairing.request_approved", kinds)
            self.assertIn("federation.pairing.request_rejected", kinds)
            self.assertIn("federation.pairing.trust_revoked", kinds)
            self.assertTrue(all(event["data"].get("group_id") == "g_local" for event in events))
        finally:
            cleanup()

    def test_delete_pairing_outbound_removes_local_sent_request(self) -> None:
        from cccc.kernel.federation.pairing import delete_pairing_outbound, list_pairing_outbounds, upsert_pairing_outbound

        _, cleanup = self._with_home()
        try:
            upsert_pairing_outbound({
                "outbound_id": "pout_failed",
                "local_group_id": "g_joiner",
                "issuer_endpoint": "https://issuer.example",
                "issuer_group_id": "g_issuer",
                "issuer_peer_id": "peer_issuer",
                "invite_id": "pinv_1",
                "status": "failed",
                "last_error": "remote pairing request failed",
            })

            deleted = delete_pairing_outbound("pout_failed")

            self.assertTrue(deleted)
            self.assertEqual(list_pairing_outbounds(group_id="g_joiner"), [])
            self.assertFalse(delete_pairing_outbound("pout_failed"))
        finally:
            cleanup()

    def test_list_trusts_backfills_remote_display_from_approved_outbound(self) -> None:
        from cccc.kernel.federation.pairing import list_trusts, upsert_pairing_outbound
        from cccc.kernel.federation.pairing import _load_store, _save_store  # type: ignore[attr-defined]

        home, cleanup = self._with_home()
        try:
            store = _load_store(home)
            store["trusts"]["ptrust_old"] = {
                "trust_id": "ptrust_old",
                "request_id": "preq_old",
                "registration_id": "reg_old",
                "group_id": "g_joiner",
                "remote_group_id": "g_issuer",
                "remote_peer_id": "peer_issuer",
                "multiaddrs": [],
                "transport": "federation_session",
                "status": "active",
                "created_at": "2026-06-15T00:00:00Z",
                "updated_at": "2026-06-15T00:00:00Z",
            }
            _save_store(store, home)
            upsert_pairing_outbound({
                "outbound_id": "pout_old",
                "local_group_id": "g_joiner",
                "issuer_endpoint": "https://issuer.example",
                "issuer_group_id": "g_issuer",
                "issuer_group_title": "Issuer Group",
                "issuer_peer_id": "peer_issuer",
                "invite_id": "pinv_old",
                "status": "approved",
            }, home=home)

            trust = list_trusts(group_id="g_joiner", home=home)[0]

            self.assertEqual(trust["remote_endpoint"], "https://issuer.example")
            self.assertEqual(trust["remote_group_title"], "Issuer Group")
        finally:
            cleanup()

    def test_invite_can_be_created_for_local_group_without_remote_identity(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite

        _, cleanup = self._with_home()
        try:
            invite = create_pairing_invite(group_id="g_local", ttl_seconds=600)

            self.assertEqual(invite["group_id"], "g_local")
            self.assertEqual(invite["remote_group_id"], "")
            self.assertEqual(invite["remote_peer_id"], "")
            self.assertRegex(invite["pairing_code"], r"^[A-Z0-9]{4}-[A-Z0-9]{4}$")
        finally:
            cleanup()

    def test_pairing_code_is_single_use(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite, create_pairing_request

        _, cleanup = self._with_home()
        try:
            invite = create_pairing_invite(
                group_id="g_local",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
                ttl_seconds=600,
            )
            create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_remote",
                requester_peer_id="peer_remote",
            )
            with self.assertRaises(ValueError) as ctx:
                create_pairing_request(
                    invite["pairing_code"],
                    requester_group_id="g_remote",
                    requester_peer_id="peer_remote",
                )
            self.assertIn("already used", str(ctx.exception))
        finally:
            cleanup()

    def test_expired_pairing_code_is_rejected_without_raw_code_leak(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite, create_pairing_request

        _, cleanup = self._with_home()
        try:
            invite = create_pairing_invite(
                group_id="g_local",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
                ttl_seconds=-1,
            )
            with self.assertRaises(ValueError) as ctx:
                create_pairing_request(
                    invite["pairing_code"],
                    requester_group_id="g_remote",
                    requester_peer_id="peer_remote",
                )
            self.assertIn("expired", str(ctx.exception))
            self.assertNotIn(invite["pairing_code"], str(ctx.exception))
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
