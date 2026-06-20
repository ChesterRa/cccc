import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestFederationPairingRemote(unittest.TestCase):
    def _home(self):
        td_ctx = tempfile.TemporaryDirectory()
        td = Path(td_ctx.__enter__())

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)

        return td, cleanup

    def test_connection_payload_carries_issuer_endpoint_and_integrity(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload, parse_connection_payload

        home, cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=home)
            payload = build_connection_payload(
                invite,
                issuer_endpoint="https://issuer.example/base/",
                issuer_group_title="Issuer",
                home=home,
            )

            self.assertEqual(payload["issuer_endpoint"], "https://issuer.example")
            self.assertEqual(payload["issuer_group_id"], "g_issuer")
            self.assertEqual(payload["issuer_group_title"], "Issuer")
            self.assertEqual(payload["code"], invite["pairing_code"])
            self.assertEqual(payload["nonce"], invite["invite_id"])
            self.assertTrue(str(payload["integrity"]).startswith("sha256:"))
            parsed = parse_connection_payload(payload)
            self.assertTrue(parsed.is_remote)
            self.assertEqual(parsed.pairing_code, invite["pairing_code"])
            self.assertEqual(parsed.issuer_group_id, "g_issuer")
            self.assertEqual(parsed.issuer_group_title, "Issuer")
        finally:
            cleanup()

    def test_connection_payload_allows_private_issuer_endpoint_declaration(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload

        home, cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=home)
            payload = build_connection_payload(
                invite,
                issuer_endpoint="http://10.0.0.5:8858/base/",
                issuer_group_title="Issuer",
                home=home,
            )

            self.assertEqual(payload["issuer_endpoint"], "http://10.0.0.5:8858")
            self.assertTrue(str(payload["integrity"]).startswith("sha256:"))
        finally:
            cleanup()

    def test_connection_payload_rejects_metadata_and_link_local_declarations(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload

        home, cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=home)
            with self.assertRaises(ValueError):
                build_connection_payload(invite, issuer_endpoint="http://169.254.169.254/latest", home=home)
            with self.assertRaises(ValueError):
                build_connection_payload(invite, issuer_endpoint="http://169.254.1.20:8858", home=home)
        finally:
            cleanup()

    def test_connection_payload_integrity_covers_issuer_group_title(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(
                invite,
                issuer_endpoint="http://127.0.0.1:5555",
                issuer_group_title="Issuer Group",
                home=issuer_home,
            )
            tampered = {**payload, "issuer_group_title": "Changed Group"}

            with self.assertRaises(ValueError) as ctx:
                submit_remote_pairing_request(
                    tampered,
                    local_group_id="g_joiner",
                    allow_localhost=True,
                    home=joiner_home,
                )

            self.assertIn("integrity mismatch", str(ctx.exception))
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_legacy_connection_payload_without_group_title_still_validates(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite, list_pairing_outbounds
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(
                invite,
                issuer_endpoint="http://127.0.0.1:5555",
                issuer_group_title="",
                home=issuer_home,
            )
            payload.pop("issuer_group_title", None)

            def fake_client(_endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
                return {"request": {
                    "request_id": "preq_remote",
                    "invite_id": body["invite_id"],
                    "group_id": "g_issuer",
                    "remote_group_id": body["requester_group_id"],
                    "remote_peer_id": body["requester_peer_id"],
                    "status": "pending",
                }}

            submit_remote_pairing_request(
                payload,
                local_group_id="g_joiner",
                client=fake_client,
                allow_localhost=True,
                home=joiner_home,
            )

            self.assertEqual(len(list_pairing_outbounds(group_id="g_joiner", home=joiner_home)), 1)
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_endpoint_policy_allows_private_ip_and_allows_localhost_for_dev(self) -> None:
        from cccc.kernel.federation.pairing_remote import normalize_issuer_endpoint

        with self.assertRaises(ValueError):
            normalize_issuer_endpoint("http://169.254.169.254/latest", allow_localhost=True)

        self.assertEqual(normalize_issuer_endpoint("http://10.0.0.5:5555", allow_localhost=True), "http://10.0.0.5:5555")
        self.assertEqual(normalize_issuer_endpoint("http://127.0.0.1:5555/ui/"), "http://127.0.0.1:5555")
        self.assertEqual(normalize_issuer_endpoint("http://localhost:5555"), "http://localhost:5555")

    def test_remote_submit_allows_private_endpoint_by_default(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://10.0.0.5:8858", home=issuer_home)
            calls = []

            def fake_client(endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
                calls.append((endpoint, body, timeout_seconds))
                return {"request": {"request_id": "preq_remote", "status": "pending"}}

            outbound = submit_remote_pairing_request(payload, local_group_id="g_joiner", client=fake_client, home=joiner_home)

            self.assertEqual(calls[0][0], "http://10.0.0.5:8858/api/federation/pairing/requests/remote")
            self.assertEqual(outbound["status"], "submitted")
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_remote_submit_rejects_metadata_and_link_local(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            for endpoint in ("http://169.254.169.254/latest", "http://169.254.1.20:8858"):
                with self.assertRaises(ValueError):
                    payload = build_connection_payload(invite, issuer_endpoint=endpoint, home=issuer_home)
                    submit_remote_pairing_request(payload, local_group_id="g_joiner", client=lambda *_args, **_kwargs: {}, home=joiner_home)
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_submit_remote_pairing_request_records_outbound_without_local_code_lookup(self) -> None:
        from cccc.kernel.federation.pairing import list_pairing_outbounds
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request
        from cccc.kernel.federation.pairing import create_pairing_invite

        joiner_home, cleanup = self._home()
        calls = []

        def fake_client(endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
            calls.append((endpoint, body, timeout_seconds))
            return {
                "request": {
                    "request_id": "preq_remote",
                    "group_id": "g_issuer",
                    "remote_group_id": body["requester_group_id"],
                    "remote_peer_id": body["requester_peer_id"],
                    "status": "pending",
                }
            }

        try:
            issuer_home, issuer_cleanup = self._home()
            try:
                invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
                payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)
            finally:
                issuer_cleanup()

            result = submit_remote_pairing_request(
                payload,
                local_group_id="g_joiner",
                local_group_title="Joiner",
                client=fake_client,
                allow_localhost=True,
                home=joiner_home,
            )

            self.assertEqual(calls[0][0], "http://127.0.0.1:5555/api/federation/pairing/requests/remote")
            self.assertEqual(calls[0][1]["pairing_code"], payload["code"])
            self.assertEqual(calls[0][1]["invite_id"], payload["nonce"])
            self.assertIn("requester_endpoint", calls[0][1])
            self.assertEqual(calls[0][1]["requester_group_id"], "g_joiner")
            self.assertEqual(result["status"], "submitted")
            self.assertEqual(result["issuer_group_id"], "g_issuer")
            outbounds = list_pairing_outbounds(group_id="g_joiner", home=joiner_home)
            self.assertEqual(len(outbounds), 1)
            self.assertEqual(outbounds[0]["outbound_id"], result["outbound_id"])
            self.assertEqual(outbounds[0]["status"], "submitted")
        finally:
            cleanup()

    def test_submit_remote_pairing_request_does_not_send_direct_multiaddrs(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        calls = []
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)

            def fake_client(endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
                calls.append((endpoint, body, timeout_seconds))
                return {
                    "request": {
                        "request_id": "preq_remote",
                        "group_id": "g_issuer",
                        "remote_group_id": body["requester_group_id"],
                        "remote_peer_id": body["requester_peer_id"],
                        "multiaddrs": body["requester_multiaddrs"],
                        "status": "pending",
                    }
                }

            submit_remote_pairing_request(
                payload,
                local_group_id="g_joiner",
                client=fake_client,
                allow_localhost=True,
                home=joiner_home,
            )

            self.assertEqual(calls[0][1]["requester_multiaddrs"], [])
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_submit_remote_pairing_request_ignores_routable_web_host_for_direct_multiaddrs(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        calls = []
        old_home = os.environ.get("CCCC_HOME")
        old_host = os.environ.get("CCCC_WEB_HOST")
        try:
            os.environ["CCCC_HOME"] = str(joiner_home)
            os.environ.pop("CCCC_WEB_HOST", None)
            (joiner_home / "settings.yaml").write_text(
                "\n".join(
                    [
                        "remote_access:",
                        "  provider: manual",
                        "  enabled: true",
                        "  web_host: 172.30.79.171",
                        "",
                    ]
                ),
                encoding="utf-8",
            )
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)

            def fake_client(endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
                calls.append((endpoint, body, timeout_seconds))
                return {
                    "request": {
                        "request_id": "preq_remote",
                        "group_id": "g_issuer",
                        "remote_group_id": body["requester_group_id"],
                        "remote_peer_id": body["requester_peer_id"],
                        "multiaddrs": body["requester_multiaddrs"],
                        "status": "pending",
                    }
                }

            submit_remote_pairing_request(
                payload,
                local_group_id="g_joiner",
                client=fake_client,
                allow_localhost=True,
                home=joiner_home,
            )

            self.assertEqual(calls[0][1]["requester_multiaddrs"], [])
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home
            if old_host is None:
                os.environ.pop("CCCC_WEB_HOST", None)
            else:
                os.environ["CCCC_WEB_HOST"] = old_host
            issuer_cleanup()
            joiner_cleanup()

    def test_submit_remote_pairing_request_without_routable_host_has_no_direct_multiaddrs(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        calls = []
        old_home = os.environ.get("CCCC_HOME")
        old_public_url = os.environ.get("CCCC_WEB_PUBLIC_URL")
        old_host = os.environ.get("CCCC_WEB_HOST")
        try:
            os.environ["CCCC_HOME"] = str(joiner_home)
            os.environ.pop("CCCC_WEB_PUBLIC_URL", None)
            os.environ.pop("CCCC_WEB_HOST", None)
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)

            def fake_client(endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
                calls.append((endpoint, body, timeout_seconds))
                return {
                    "request": {
                        "request_id": "preq_remote",
                        "group_id": "g_issuer",
                        "remote_group_id": body["requester_group_id"],
                        "remote_peer_id": body["requester_peer_id"],
                        "multiaddrs": body["requester_multiaddrs"],
                        "status": "pending",
                    }
                }

            submit_remote_pairing_request(
                payload,
                local_group_id="g_joiner",
                client=fake_client,
                allow_localhost=True,
                home=joiner_home,
            )

            self.assertEqual(calls[0][1]["requester_multiaddrs"], [])
        finally:
            for key, value in {
                "CCCC_HOME": old_home,
                "CCCC_WEB_PUBLIC_URL": old_public_url,
                "CCCC_WEB_HOST": old_host,
            }.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
            issuer_cleanup()
            joiner_cleanup()

    def test_sync_remote_pairing_outbound_creates_joiner_trust_after_issuer_approval(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult
        from cccc.kernel.federation.pairing import create_pairing_invite, list_pairing_outbounds, list_trusts
        from cccc.kernel.federation.registration import list_registrations
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request, sync_remote_pairing_outbound

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(
                invite,
                issuer_endpoint="http://127.0.0.1:5555",
                issuer_group_title="Issuer Group",
                home=issuer_home,
            )

            def submit_client(_endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
                from cccc.kernel.federation.pairing import create_pairing_request, approve_pairing_request

                request = create_pairing_request(
                    body["pairing_code"],
                    requester_group_id=body["requester_group_id"],
                    requester_peer_id=body["requester_peer_id"],
                    requester_endpoint=body["requester_endpoint"],
                    invite_id=body["invite_id"],
                    home=issuer_home,
                )
                approve_pairing_request(request["request_id"], approver_user_id="issuer-admin", home=issuer_home)
                return {"request": request}

            with patch("cccc.kernel.federation.pairing_remote._requester_endpoint", return_value="http://joiner.example:8848"):
                outbound = submit_remote_pairing_request(
                    payload,
                    local_group_id="g_joiner",
                    client=submit_client,
                    allow_localhost=True,
                    home=joiner_home,
                )

            def status_client(endpoint: str, *, timeout_seconds: float = 3.0) -> dict:
                self.assertIn(f"request_id={outbound['remote_request']['request_id']}", endpoint)
                self.assertIn(f"invite_id={outbound['invite_id']}", endpoint)
                from cccc.kernel.federation.pairing import get_pairing_request_public_status

                return {"request": get_pairing_request_public_status(outbound["remote_request"]["request_id"], invite_id=outbound["invite_id"], home=issuer_home)}

            synced = sync_remote_pairing_outbound(
                outbound["outbound_id"],
                client=status_client,
                allow_localhost=True,
                home=joiner_home,
            )

            self.assertEqual(synced["status"], "approved")
            self.assertEqual(synced["local_group_id"], "g_joiner")
            self.assertEqual(list_pairing_outbounds(group_id="g_joiner", home=joiner_home)[0]["status"], "approved")
            trusts = list_trusts(group_id="g_joiner", home=joiner_home)
            self.assertEqual(len(trusts), 1)
            self.assertEqual(trusts[0]["remote_group_id"], "g_issuer")
            self.assertEqual(trusts[0]["remote_peer_id"], payload["issuer_peer_id"])
            self.assertEqual(trusts[0]["remote_endpoint"], "http://127.0.0.1:5555")
            self.assertEqual(trusts[0]["remote_group_title"], "Issuer Group")
            self.assertEqual(trusts[0]["transport"], "federation_session")
            registrations = list_registrations(home=joiner_home)
            self.assertEqual(len(registrations), 1)
            self.assertEqual(registrations[0]["transport"], "federation_session")
            self.assertEqual(registrations[0]["url"], "http://127.0.0.1:5555")
            self.assertEqual(registrations[0]["remote_group_id"], "g_issuer")
            self.assertEqual(registrations[0]["remote_peer_id"], payload["issuer_peer_id"])
            self.assertEqual(registrations[0]["credential_ref"], "")

            enqueue_remote_send(
                src_group_id="g_joiner",
                registration_id=registrations[0]["registration_id"],
                idempotency_key="send-1",
                payload={"text": "hello cross instance"},
                home=joiner_home,
            )
            captured = {}

            class CapturingSessionTransport:
                transport = "federation_session"
                capabilities = frozenset()

                def deliver(self, envelope):
                    captured["transport_name"] = envelope.transport
                    captured["target"] = envelope.target
                    captured["payload"] = envelope.payload
                    captured["credential"] = envelope.credential
                    return RemoteSendResult(ok=True, status="sent", remote_event_id="evt_remote", transport="federation_session")

            receipt = deliver_enqueued(
                registration_id=registrations[0]["registration_id"],
                idempotency_key="send-1",
                home=joiner_home,
                transport_factory=lambda name: CapturingSessionTransport(),
            )
            self.assertEqual(receipt["status"], "sent")
            self.assertEqual(captured["transport_name"], "federation_session")
            self.assertEqual(captured["target"].url, "http://127.0.0.1:5555")
            self.assertEqual(captured["target"].remote_group_id, "g_issuer")
            self.assertEqual(captured["payload"].text, "hello cross instance")
            self.assertEqual(captured["credential"], "")
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_sync_status_failure_keeps_submitted_outbound_state(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite, list_pairing_outbounds
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request, sync_remote_pairing_outbound

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)

            def submit_client(_endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
                return {"request": {
                    "request_id": "preq_remote",
                    "invite_id": body["invite_id"],
                    "group_id": "g_issuer",
                    "remote_group_id": body["requester_group_id"],
                    "remote_peer_id": body["requester_peer_id"],
                    "status": "pending",
                }}

            with patch("cccc.kernel.federation.pairing_remote._requester_endpoint", return_value="http://joiner.example:8848"):
                outbound = submit_remote_pairing_request(
                    payload,
                    local_group_id="g_joiner",
                    client=submit_client,
                    allow_localhost=True,
                    home=joiner_home,
                )

            def unauthorized_status_client(_endpoint: str, *, timeout_seconds: float = 3.0) -> dict:
                raise ValueError("remote pairing status failed")

            synced = sync_remote_pairing_outbound(
                outbound["outbound_id"],
                client=unauthorized_status_client,
                allow_localhost=True,
                home=joiner_home,
            )

            self.assertEqual(synced["status"], "submitted")
            self.assertEqual(list_pairing_outbounds(group_id="g_joiner", home=joiner_home)[0]["status"], "submitted")
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_sync_remote_pairing_keeps_session_route_with_issuer_endpoint(self) -> None:
        from cccc.kernel.federation import pairing
        from cccc.kernel.federation.pairing import create_pairing_invite, list_trusts
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request, sync_remote_pairing_outbound
        from cccc.kernel.federation.registration import get_registration

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(
                invite,
                issuer_endpoint="http://127.0.0.1:5555",
                issuer_group_title="Issuer Group",
                home=issuer_home,
            )

            def submit_client(_endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
                request = pairing.create_pairing_request(
                    body["pairing_code"],
                    requester_group_id=body["requester_group_id"],
                    requester_peer_id=body["requester_peer_id"],
                    requester_endpoint=body["requester_endpoint"],
                    invite_id=body["invite_id"],
                    home=issuer_home,
                )
                pairing.approve_pairing_request(request["request_id"], approver_user_id="issuer-admin", home=issuer_home)
                return {"request": request}

            with patch("cccc.kernel.federation.pairing_remote._requester_endpoint", return_value="http://joiner.example:8848"):
                outbound = submit_remote_pairing_request(
                    payload,
                    local_group_id="g_joiner",
                    client=submit_client,
                    allow_localhost=True,
                    home=joiner_home,
                )

            def status_client(_endpoint: str, *, timeout_seconds: float = 3.0) -> dict:
                return {"request": pairing.get_pairing_request_public_status(
                    outbound["remote_request"]["request_id"],
                    invite_id=outbound["invite_id"],
                    home=issuer_home,
                )}

            sync_remote_pairing_outbound(
                outbound["outbound_id"],
                client=status_client,
                allow_localhost=True,
                home=joiner_home,
            )

            trusts = list_trusts(group_id="g_joiner", home=joiner_home)
            self.assertEqual(len(trusts), 1)
            self.assertEqual(trusts[0]["transport"], "federation_session")
            self.assertEqual(trusts[0]["remote_endpoint"], "http://127.0.0.1:5555")
            registration = get_registration(trusts[0]["registration_id"], home=joiner_home)
            self.assertIsNotNone(registration)
            assert registration is not None
            self.assertEqual(registration["transport"], "federation_session")
            self.assertEqual(registration["url"], "http://127.0.0.1:5555")
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_sync_remote_pairing_without_issuer_endpoint_keeps_session_only_route(self) -> None:
        from cccc.kernel.federation.pairing import list_trusts, upsert_pairing_outbound
        from cccc.kernel.federation.pairing_outbound_sync import approve_outbound_from_remote_request
        from cccc.kernel.federation.registration import get_registration

        joiner_home, cleanup = self._home()
        try:
            outbound = upsert_pairing_outbound(
                {
                    "local_group_id": "g_joiner",
                    "issuer_group_id": "g_issuer",
                    "issuer_group_title": "Issuer Group",
                    "issuer_endpoint": "",
                    "issuer_peer_id": "peer_issuer",
                    "invite_id": "pinv_1",
                    "remote_request": {"request_id": "preq_1"},
                    "status": "submitted",
                },
                home=joiner_home,
            )

            approved = approve_outbound_from_remote_request(
                outbound["outbound_id"],
                {"request_id": "preq_1", "status": "approved"},
                home=joiner_home,
            )

            self.assertEqual(approved["outbound"]["status"], "approved")
            self.assertEqual(approved["trust"]["remote_endpoint"], "")
            self.assertEqual(approved["registration"]["url"], "federation-session://peer_issuer")
            trusts = list_trusts(group_id="g_joiner", home=joiner_home)
            self.assertEqual(len(trusts), 1)
            self.assertEqual(trusts[0]["remote_endpoint"], "")
            registration = get_registration(trusts[0]["registration_id"], home=joiner_home)
            self.assertIsNotNone(registration)
            assert registration is not None
            self.assertEqual(registration["url"], "federation-session://peer_issuer")
        finally:
            cleanup()

    def test_approved_remote_pairing_session_registration_requires_active_session(self) -> None:
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.kernel.federation import pairing
        from cccc.kernel.federation.pairing import create_pairing_invite
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request, sync_remote_pairing_outbound
        from cccc.kernel.federation.registration import list_registrations

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)

            def submit_client(_endpoint: str, body: dict, *, timeout_seconds: float = 3.0) -> dict:
                request = pairing.create_pairing_request(
                    body["pairing_code"],
                    requester_group_id=body["requester_group_id"],
                    requester_peer_id=body["requester_peer_id"],
                    requester_endpoint=body["requester_endpoint"],
                    invite_id=body["invite_id"],
                    home=issuer_home,
                )
                pairing.approve_pairing_request(request["request_id"], approver_user_id="issuer-admin", home=issuer_home)
                return {"request": request}

            outbound = submit_remote_pairing_request(
                payload,
                local_group_id="g_joiner",
                client=submit_client,
                allow_localhost=True,
                home=joiner_home,
            )

            def status_client(_endpoint: str, *, timeout_seconds: float = 3.0) -> dict:
                return {"request": pairing.get_pairing_request_public_status(
                    outbound["remote_request"]["request_id"],
                    invite_id=outbound["invite_id"],
                    home=issuer_home,
                )}

            sync_remote_pairing_outbound(
                outbound["outbound_id"],
                client=status_client,
                allow_localhost=True,
                home=joiner_home,
            )
            registration = list_registrations(home=joiner_home)[0]
            old_home = os.environ.get("CCCC_HOME")
            os.environ["CCCC_HOME"] = str(joiner_home)
            try:
                result = handle_remote_send(
                    {
                        "group_id": "g_joiner",
                        "registration_id": registration["registration_id"],
                        "idempotency_key": "send-credential",
                        "payload": {"text": "hello", "to": ["@foreman"]},
                    }
                )
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home

            self.assertTrue(result.ok)
            self.assertEqual(result.result["receipt"]["status"], "retrying")
            self.assertEqual(result.result["receipt"]["error"]["code"], "peer_session_unavailable")
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_tampered_integrity_rejects_before_remote_call_and_does_not_store_outbound(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite, list_pairing_outbounds
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        calls = []
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)
            payload["issuer_group_id"] = "g_tampered"

            def fake_client(*_args, **_kwargs):
                calls.append(True)
                return {}

            with self.assertRaises(ValueError):
                submit_remote_pairing_request(
                    payload,
                    local_group_id="g_joiner",
                    client=fake_client,
                    allow_localhost=True,
                    home=joiner_home,
                )

            self.assertEqual(calls, [])
            self.assertEqual(list_pairing_outbounds(group_id="g_joiner", home=joiner_home), [])
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_missing_integrity_rejects_remote_payload_before_remote_call(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite, list_pairing_outbounds
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        calls = []
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)
            payload.pop("integrity", None)

            def fake_client(*_args, **_kwargs):
                calls.append(True)
                return {}

            with self.assertRaises(ValueError):
                submit_remote_pairing_request(
                    payload,
                    local_group_id="g_joiner",
                    client=fake_client,
                    allow_localhost=True,
                    home=joiner_home,
                )

            self.assertEqual(calls, [])
            self.assertEqual(list_pairing_outbounds(group_id="g_joiner", home=joiner_home), [])
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_missing_nonce_rejects_remote_payload_before_remote_call(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite, list_pairing_outbounds
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        calls = []
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)
            payload.pop("nonce", None)
            payload.pop("invite_id", None)

            def fake_client(*_args, **_kwargs):
                calls.append(True)
                return {}

            with self.assertRaises(ValueError):
                submit_remote_pairing_request(
                    payload,
                    local_group_id="g_joiner",
                    client=fake_client,
                    allow_localhost=True,
                    home=joiner_home,
                )

            self.assertEqual(calls, [])
            self.assertEqual(list_pairing_outbounds(group_id="g_joiner", home=joiner_home), [])
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_remote_error_is_sanitized_before_persisting_outbound(self) -> None:
        from cccc.kernel.federation.pairing import create_pairing_invite, list_pairing_outbounds
        from cccc.kernel.federation.pairing_remote import build_connection_payload, submit_remote_pairing_request

        issuer_home, issuer_cleanup = self._home()
        joiner_home, joiner_cleanup = self._home()
        try:
            invite = create_pairing_invite(group_id="g_issuer", ttl_seconds=600, home=issuer_home)
            payload = build_connection_payload(invite, issuer_endpoint="http://127.0.0.1:5555", home=issuer_home)

            def leaking_client(*_args, **_kwargs):
                raise RuntimeError(f"upstream body leaked code={payload['code']} token=secret-token")

            result = submit_remote_pairing_request(
                payload,
                local_group_id="g_joiner",
                client=leaking_client,
                allow_localhost=True,
                home=joiner_home,
            )

            self.assertEqual(result["status"], "failed")
            self.assertNotIn(str(payload["code"]), result["last_error"])
            self.assertNotIn("secret-token", result["last_error"])
            self.assertIn("remote pairing request failed", result["last_error"])
            outbounds = list_pairing_outbounds(group_id="g_joiner", home=joiner_home)
            self.assertEqual(outbounds[0]["last_error"], result["last_error"])
        finally:
            issuer_cleanup()
            joiner_cleanup()

    def test_remote_client_uses_short_timeout_and_rejects_redirects(self) -> None:
        from cccc.kernel.federation.pairing_remote import _NoRedirectHandler, _default_remote_client

        captured = {}

        class FakeOpener:
            def open(self, req, *, timeout):
                captured["url"] = req.full_url
                captured["timeout"] = timeout
                raise RuntimeError("redirect disabled")

        with patch("cccc.kernel.federation.pairing_remote.build_opener", return_value=FakeOpener()):
            with self.assertRaises(RuntimeError):
                _default_remote_client("https://issuer.example/api/federation/pairing/requests/remote", {"pairing_code": "ABCD-1234"}, timeout_seconds=3.0)

        self.assertEqual(captured["url"], "https://issuer.example/api/federation/pairing/requests/remote")
        self.assertEqual(captured["timeout"], 3.0)
        with self.assertRaises(ValueError):
            _NoRedirectHandler().redirect_request(None, None, 302, "Found", {}, "https://other.example/")


if __name__ == "__main__":
    unittest.main()
