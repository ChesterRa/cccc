import os
import json
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

import yaml


class TestGroupBridgePairing(unittest.TestCase):
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
        from cccc.kernel.group_bridge.pairing import get_local_identity

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

    def test_concurrent_first_identity_callers_return_the_same_peer(self) -> None:
        from cccc.daemon.group_bridge import identity as signing_identity
        from cccc.kernel.group_bridge.pairing import get_local_identity

        _, cleanup = self._with_home()
        first_save_started = threading.Event()
        release_first_save = threading.Event()
        second_lock_attempted = threading.Event()
        second_finished = threading.Event()
        returned: list[str] = []
        errors: list[BaseException] = []
        original_acquire = signing_identity.acquire_lockfile
        original_save = signing_identity._save_yaml

        def observed_acquire(path, *, blocking=True):
            if threading.current_thread().name == "identity-caller-b":
                second_lock_attempted.set()
            return original_acquire(path, blocking=blocking)

        def controlled_save(path, payload):
            if threading.current_thread().name == "identity-caller-a":
                first_save_started.set()
                if not release_first_save.wait(timeout=3.0):
                    raise AssertionError("timed out waiting to release the identity write")
            return original_save(path, payload)

        def load_identity() -> None:
            try:
                returned.append(str(get_local_identity().get("peer_id") or ""))
            except BaseException as exc:  # pragma: no cover - asserted below
                errors.append(exc)
            finally:
                if threading.current_thread().name == "identity-caller-b":
                    second_finished.set()

        first = threading.Thread(target=load_identity, name="identity-caller-a")
        second = threading.Thread(target=load_identity, name="identity-caller-b")
        try:
            with (
                mock.patch.object(signing_identity, "acquire_lockfile", side_effect=observed_acquire),
                mock.patch.object(signing_identity, "_save_yaml", side_effect=controlled_save),
            ):
                first.start()
                self.assertTrue(first_save_started.wait(timeout=2.0))
                second.start()
                self.assertTrue(second_lock_attempted.wait(timeout=2.0))
                self.assertFalse(second_finished.wait(timeout=0.1))
                release_first_save.set()
                first.join(timeout=3.0)
                second.join(timeout=3.0)

            self.assertFalse(first.is_alive())
            self.assertFalse(second.is_alive())
            self.assertEqual(errors, [])
            self.assertEqual(len(returned), 2)
            self.assertEqual(len(set(returned)), 1)
            self.assertEqual(get_local_identity()["peer_id"], returned[0])
        finally:
            release_first_save.set()
            first.join(timeout=1.0)
            second.join(timeout=1.0)
            cleanup()

    def test_concurrent_update_cannot_resurrect_a_revoked_trust(self) -> None:
        from cccc.kernel.group_bridge import pairing

        _, cleanup = self._with_home()
        initial = {
            "invites": {},
            "requests": {},
            "trusts": {
                "trust_demo": {
                    "trust_id": "trust_demo",
                    "group_id": "g_local",
                    "remote_group_id": "g_remote",
                    "remote_peer_id": "peer_remote",
                    "access_level": "messages",
                    "status": "active",
                    "created_at": "2026-08-11T00:00:00Z",
                    "updated_at": "2026-08-11T00:00:00Z",
                }
            },
            "outbounds": {},
        }
        pairing._save_store(initial)
        revoke_at_save = threading.Event()
        release_revoke = threading.Event()
        update_started = threading.Event()
        update_finished = threading.Event()
        revoked: list[dict] = []
        revoke_errors: list[BaseException] = []
        update_errors: list[BaseException] = []
        original_save = pairing._save_store

        def controlled_save(store, home=None):
            if threading.current_thread().name == "trust-revoke":
                revoke_at_save.set()
                if not release_revoke.wait(timeout=3.0):
                    raise AssertionError("timed out waiting to release trust revocation")
            return original_save(store, home)

        def revoke() -> None:
            try:
                revoked.append(pairing.revoke_trust("trust_demo", revoked_by="owner"))
            except BaseException as exc:  # pragma: no cover - asserted below
                revoke_errors.append(exc)

        def update_access() -> None:
            update_started.set()
            try:
                pairing.update_trust_access_level(
                    "trust_demo", "read", updated_by="concurrent-update"
                )
            except BaseException as exc:
                update_errors.append(exc)
            finally:
                update_finished.set()

        revoke_thread = threading.Thread(target=revoke, name="trust-revoke")
        update_thread = threading.Thread(target=update_access, name="stale-access-update")
        try:
            with mock.patch.object(pairing, "_save_store", side_effect=controlled_save):
                revoke_thread.start()
                self.assertTrue(revoke_at_save.wait(timeout=2.0))
                update_thread.start()
                self.assertTrue(update_started.wait(timeout=2.0))
                self.assertFalse(update_finished.wait(timeout=0.1))
                release_revoke.set()
                revoke_thread.join(timeout=3.0)
                update_thread.join(timeout=3.0)

            self.assertFalse(revoke_thread.is_alive())
            self.assertFalse(update_thread.is_alive())
            self.assertEqual(revoke_errors, [])
            self.assertEqual(len(revoked), 1)
            self.assertEqual(revoked[0]["status"], "revoked")
            self.assertEqual(len(update_errors), 1)
            self.assertIn("trust is not active", str(update_errors[0]))
            self.assertEqual(
                pairing.list_trusts(group_id="g_local")[0]["status"],
                "revoked",
            )
        finally:
            release_revoke.set()
            revoke_thread.join(timeout=1.0)
            update_thread.join(timeout=1.0)
            cleanup()

    def test_invite_request_approve_creates_group_bridge_session_registration(self) -> None:
        from cccc.kernel.group_bridge.pairing import (
            approve_pairing_request,
            create_pairing_invite,
            create_pairing_request,
            get_pairing_request_public_status,
            get_pairing_request,
            list_trusts,
            revoke_trust,
        )
        from cccc.kernel.access_tokens import list_access_tokens, lookup_access_token
        from cccc.kernel.group_bridge.credentials import resolve_pairing_remote_send_token
        from cccc.kernel.group_bridge.registration import get_registration

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
            self.assertEqual(invite["transport"], "group_bridge_session")
            pairing_store = Path(os.environ["CCCC_HOME"]) / "group_bridge_pairing.yaml"
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
            self.assertEqual(registration["transport"], "group_bridge_session")
            self.assertEqual(registration["group_id"], "g_local")
            self.assertEqual(registration["url"], "http://remote.example:8848")
            self.assertEqual(registration["remote_group_id"], "g_remote")
            self.assertEqual(registration["remote_peer_id"], "peer_remote")
            self.assertEqual(registration["multiaddrs"], [])
            self.assertEqual(registration["status"], "active")
            self.assertEqual(approved["trust"]["transport"], "group_bridge_session")
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
            status = get_pairing_request_public_status(request["request_id"], invite_id=invite["invite_id"])
            self.assertIsNotNone(status)
            assert status is not None
            remote_send_token = str(status.get("remote_send_token") or "")
            self.assertTrue(remote_send_token.startswith("frs_"))
            self.assertEqual(list_access_tokens(), [])
            self.assertIsNone(lookup_access_token(remote_send_token))
            store_doc = yaml.safe_load(pairing_store.read_text(encoding="utf-8")) or {}
            credential_ref = str(
                (((store_doc.get("requests") or {}).get(request["request_id"]) or {}).get("remote_send_credential_ref"))
                or ""
            )
            self.assertTrue(credential_ref.startswith("fsec_remote_send_"))
            self.assertEqual(resolve_pairing_remote_send_token(credential_ref), remote_send_token)

            replay = approve_pairing_request(request["request_id"], approver_user_id="user-b")
            self.assertEqual(replay["status"], "approved")
            self.assertEqual(replay["registration"]["registration_id"], registration["registration_id"])

            trusts = list_trusts(group_id="g_local")
            self.assertEqual(len(trusts), 1)
            revoked = revoke_trust(trusts[0]["trust_id"], revoked_by="user-a")
            self.assertEqual(revoked["status"], "revoked")
            self.assertIsNone(get_registration(registration["registration_id"]))
            self.assertEqual(list_trusts(group_id="g_local")[0]["status"], "revoked")
            revoked_status = get_pairing_request_public_status(request["request_id"], invite_id=invite["invite_id"])
            self.assertIsNotNone(revoked_status)
            assert revoked_status is not None
            self.assertEqual(revoked_status["status"], "revoked")
            self.assertNotIn("remote_send_token", revoked_status)
            self.assertEqual(resolve_pairing_remote_send_token(credential_ref), "")
            self.assertEqual(list_access_tokens(), [])
        finally:
            cleanup()

    def test_can_create_approved_group_bridge_session_registration_with_endpoint(self) -> None:
        from cccc.kernel.group_bridge.pairing import _upsert_approved_session_registration  # type: ignore[attr-defined]

        _, cleanup = self._with_home()
        try:
            registration = _upsert_approved_session_registration(
                "g_local",
                "http://remote.example:8848",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
            )

            self.assertEqual(registration["transport"], "group_bridge_session")
            self.assertEqual(registration["url"], "http://remote.example:8848")
            self.assertEqual(registration["group_id"], "g_local")
            self.assertEqual(registration["remote_group_id"], "g_remote")
            self.assertEqual(registration["remote_peer_id"], "peer_remote")
            self.assertEqual(registration["multiaddrs"], [])
        finally:
            cleanup()

    def test_approve_pairing_request_without_remote_endpoint_creates_session_only_registration(self) -> None:
        from cccc.kernel.group_bridge.pairing import approve_pairing_request, create_pairing_invite, create_pairing_request
        from cccc.kernel.group_bridge.registration import list_registrations

        _, cleanup = self._with_home()
        try:
            invite = create_pairing_invite(group_id="g_local", ttl_seconds=600)
            request = create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_remote",
                requester_peer_id="peer_remote",
            )

            approved = approve_pairing_request(request["request_id"], approver_user_id="user-a")

            self.assertEqual(approved["registration"]["transport"], "group_bridge_session")
            self.assertEqual(approved["registration"]["url"], "group-bridge-session://peer_remote")
            self.assertEqual(approved["trust"]["remote_endpoint"], "")
            self.assertEqual(list_registrations()[0]["url"], "group-bridge-session://peer_remote")
        finally:
            cleanup()

    def test_pairing_state_changes_publish_global_events(self) -> None:
        from cccc.kernel.group_bridge.pairing import (
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
            self.assertIn("group_bridge.pairing.invite_created", kinds)
            self.assertIn("group_bridge.pairing.request_created", kinds)
            self.assertIn("group_bridge.pairing.request_approved", kinds)
            self.assertIn("group_bridge.pairing.request_rejected", kinds)
            self.assertIn("group_bridge.pairing.trust_revoked", kinds)
            self.assertTrue(all(event["data"].get("group_id") == "g_local" for event in events))
        finally:
            cleanup()

    def test_delete_pairing_outbound_removes_local_sent_request(self) -> None:
        from cccc.kernel.group_bridge.pairing import delete_pairing_outbound, list_pairing_outbounds, upsert_pairing_outbound

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
        from cccc.kernel.group_bridge.pairing import list_trusts, upsert_pairing_outbound
        from cccc.kernel.group_bridge.pairing import _load_store, _save_store  # type: ignore[attr-defined]

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
                "transport": "group_bridge_session",
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

    def test_update_trust_remote_info_records_remote_snapshot(self) -> None:
        from cccc.kernel.group_bridge.pairing import (
            approve_pairing_request,
            create_pairing_invite,
            create_pairing_request,
            list_trusts,
            update_trust_remote_info,
        )

        _, cleanup = self._with_home()
        try:
            invite = create_pairing_invite(group_id="g_local", ttl_seconds=600)
            request = create_pairing_request(
                invite["pairing_code"],
                requester_group_id="g_remote",
                requester_group_title="Old Remote",
                requester_peer_id="peer_remote",
            )
            trust = approve_pairing_request(request["request_id"], approver_user_id="user-a")["trust"]

            updated = update_trust_remote_info(
                trust["trust_id"],
                remote_group_title="Renamed Remote",
                remote_access_level="read",
            )

            self.assertEqual(updated["remote_group_title"], "Renamed Remote")
            self.assertEqual(updated["remote_access_level"], "read")
            listed = list_trusts(group_id="g_local")[0]
            self.assertEqual(listed["remote_group_title"], "Renamed Remote")
            self.assertEqual(listed["remote_access_level"], "read")
        finally:
            cleanup()

    def test_invite_can_be_created_for_local_group_without_remote_identity(self) -> None:
        from cccc.kernel.group_bridge.pairing import create_pairing_invite

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
        from cccc.kernel.group_bridge.pairing import create_pairing_invite, create_pairing_request

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
        from cccc.kernel.group_bridge.pairing import create_pairing_invite, create_pairing_request

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
