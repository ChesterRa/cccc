import unittest
import os
import tempfile
from pathlib import Path


class _EnvPatch:
    def __init__(self, **values: str | None) -> None:
        self.values = values
        self.previous: dict[str, str | None] = {}

    def __enter__(self) -> None:
        for key, value in self.values.items():
            self.previous[key] = os.environ.get(key)
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value

    def __exit__(self, exc_type, exc, tb) -> None:
        for key, value in self.previous.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


class TestFederationTransport(unittest.TestCase):
    def test_unknown_transport_raises(self) -> None:
        from cccc.daemon.federation.transports.base import (
            UnknownTransportError,
            get_transport,
        )

        with self.assertRaises(UnknownTransportError):
            get_transport("does-not-exist")

    def test_registry_returns_registered_transport(self) -> None:
        from cccc.daemon.federation.transports.base import get_transport

        t = get_transport("peer_cccc_http")
        self.assertEqual(t.transport, "peer_cccc_http")

        libp2p = get_transport("libp2p_cccc")
        self.assertEqual(libp2p.transport, "libp2p_cccc")

    def _envelope(self, *, attachments=None, refs=None):
        from cccc.contracts.v1.federation import RemoteSendPayload
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget

        return RemoteMessageEnvelope(
            transport="peer_cccc_http",
            src_group_id="g_local",
            source_peer_id="peer-local",
            target=RemoteTarget(url="https://peer.example", remote_group_id="g_remote"),
            payload=RemoteSendPayload(
                text="hi",
                attachments=attachments or [],
                refs=refs or [],
            ),
            idempotency_key="k1",
            credential="secret-token",
        )

    def test_peer_http_includes_source_libp2p_multiaddrs_when_available(self) -> None:
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget
        from cccc.daemon.federation.transports.peer_cccc_http import PeerCcccHttpTransport

        captured = {}

        def fake_post(url, body, credential):
            captured["body"] = dict(body)
            return (200, {"event_id": "remote-123"})

        envelope = self._envelope()
        envelope = RemoteMessageEnvelope(
            transport=envelope.transport,
            src_group_id=envelope.src_group_id,
            source_peer_id=envelope.source_peer_id,
            source_multiaddrs=("/ip4/127.0.0.1/tcp/4001/p2p/peer-local",),
            target=RemoteTarget(
                url=envelope.target.url,
                remote_group_id=envelope.target.remote_group_id,
                remote_peer_id="peer-remote",
            ),
            payload=envelope.payload,
            idempotency_key=envelope.idempotency_key,
            credential=envelope.credential,
        )

        res = PeerCcccHttpTransport(http_post=fake_post).deliver(envelope)

        self.assertTrue(res.ok)
        self.assertEqual(captured["body"].get("source_multiaddrs"), ["/ip4/127.0.0.1/tcp/4001/p2p/peer-local"])

    def test_remote_dispatch_advertises_routable_web_host_instead_of_loopback_source_multiaddr(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.libp2p.supervisor import sidecar_status_path
        from cccc.daemon.federation.transports.base import RemoteSendResult
        from cccc.kernel.federation.registration import upsert_registration
        from cccc.util.fs import atomic_write_json

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            old_home = os.environ.get("CCCC_HOME")
            old_host = os.environ.get("CCCC_WEB_HOST")
            try:
                os.environ["CCCC_HOME"] = str(home)
                os.environ.pop("CCCC_WEB_HOST", None)
                (home / "settings.yaml").write_text(
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
                registration = upsert_registration(
                    "g_local",
                    "https://peer.example",
                    remote_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    transport="peer_cccc_http",
                    status="active",
                    home=home,
                )
                atomic_write_json(
                    sidecar_status_path(home=home),
                    {
                        "status": "running",
                        "pid": 123,
                        "peer_id": "peer_local",
                        "multiaddrs": ["/ip4/127.0.0.1/tcp/4001/p2p/peer_local"],
                        "updated_at": "2026-06-17T00:00:00Z",
                    },
                )
                enqueue_remote_send(
                    src_group_id="g_local",
                    registration_id=registration["registration_id"],
                    idempotency_key="k-advertise",
                    payload={"text": "hi", "to": ["@foreman"]},
                    home=home,
                )
                captured = {}

                class CaptureTransport:
                    transport = "peer_cccc_http"

                    def deliver(self, envelope):
                        captured["source_multiaddrs"] = envelope.source_multiaddrs
                        return RemoteSendResult(ok=True, status="sent", transport=self.transport, remote_event_id="remote-1")

                deliver_enqueued(
                    registration_id=registration["registration_id"],
                    idempotency_key="k-advertise",
                    home=home,
                    transport_factory=lambda _name: CaptureTransport(),
                )

                self.assertEqual(captured["source_multiaddrs"], ("/ip4/172.30.79.171/tcp/4001/p2p/peer_local",))
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home
                if old_host is None:
                    os.environ.pop("CCCC_WEB_HOST", None)
                else:
                    os.environ["CCCC_WEB_HOST"] = old_host

    def test_remote_dispatch_does_not_advertise_loopback_without_routable_host(self) -> None:
        from unittest.mock import patch

        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.libp2p.supervisor import sidecar_status_path
        from cccc.daemon.federation.transports.base import RemoteSendResult
        from cccc.kernel.federation.registration import upsert_registration
        from cccc.util.fs import atomic_write_json

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            with _EnvPatch(
                CCCC_HOME=str(home),
                CCCC_LIBP2P_ADVERTISE_HOST=None,
                CCCC_WEB_PUBLIC_URL=None,
                CCCC_WEB_HOST=None,
            ):
                registration = upsert_registration(
                    "g_local",
                    "https://peer.example",
                    remote_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    transport="peer_cccc_http",
                    status="active",
                    home=home,
                )
                atomic_write_json(
                    sidecar_status_path(home=home),
                    {
                        "status": "running",
                        "pid": 123,
                        "peer_id": "peer_local",
                        "multiaddrs": ["/ip4/127.0.0.1/tcp/4001/p2p/peer_local"],
                        "updated_at": "2026-06-17T00:00:00Z",
                    },
                )
                enqueue_remote_send(
                    src_group_id="g_local",
                    registration_id=registration["registration_id"],
                    idempotency_key="k-no-loopback",
                    payload={"text": "hi", "to": ["@foreman"]},
                    home=home,
                )
                captured = {}

                class CaptureTransport:
                    transport = "peer_cccc_http"

                    def deliver(self, envelope):
                        captured["source_multiaddrs"] = envelope.source_multiaddrs
                        return RemoteSendResult(ok=True, status="sent", transport=self.transport, remote_event_id="remote-1")

                with patch("cccc.daemon.federation.libp2p.advertise._auto_detect_advertise_host", return_value=""):
                    deliver_enqueued(
                        registration_id=registration["registration_id"],
                        idempotency_key="k-no-loopback",
                        home=home,
                        transport_factory=lambda _name: CaptureTransport(),
                    )

                self.assertEqual(captured["source_multiaddrs"], ())

    def test_unsupported_attachments_and_refs_are_permanent(self) -> None:
        from cccc.daemon.federation.transports.peer_cccc_http import PeerCcccHttpTransport

        t = PeerCcccHttpTransport(http_post=lambda *a, **k: (200, {"event_id": "e1"}))

        res_att = t.deliver(self._envelope(attachments=[{"path": "x"}]))
        self.assertFalse(res_att.ok)
        self.assertEqual(res_att.status, "failed")
        self.assertFalse(res_att.retriable)
        self.assertEqual(res_att.error_code, "unsupported_attachments")

        res_refs = t.deliver(self._envelope(refs=[{"kind": "url"}]))
        self.assertFalse(res_refs.ok)
        self.assertFalse(res_refs.retriable)
        self.assertEqual(res_refs.error_code, "unsupported_refs")

    def test_success_maps_remote_event_id(self) -> None:
        from cccc.daemon.federation.transports.peer_cccc_http import PeerCcccHttpTransport

        captured = {}

        def fake_post(url, body, credential):
            captured["url"] = url
            captured["credential"] = credential
            return (200, {"event_id": "remote-123"})

        t = PeerCcccHttpTransport(http_post=fake_post)
        res = t.deliver(self._envelope())
        self.assertTrue(res.ok)
        self.assertEqual(res.status, "sent")
        self.assertEqual(res.remote_event_id, "remote-123")
        self.assertEqual(captured["url"], "https://peer.example/api/v1/groups/g_remote/send")
        self.assertEqual(captured["credential"], "secret-token")

    def test_peer_http_sends_provenance_for_remote_reply_relay(self) -> None:
        from cccc.daemon.federation.transports.peer_cccc_http import PeerCcccHttpTransport

        captured = {}

        def fake_post(url, body, credential):
            captured["body"] = dict(body)
            return (200, {"event_id": "remote-123"})

        envelope = self._envelope()
        envelope = type(envelope)(
            transport=envelope.transport,
            src_group_id=envelope.src_group_id,
            source_peer_id=envelope.source_peer_id,
            target=type(envelope.target)(
                url=envelope.target.url,
                remote_group_id=envelope.target.remote_group_id,
                remote_peer_id="peer-remote",
                multiaddrs=envelope.target.multiaddrs,
            ),
            payload=envelope.payload,
            idempotency_key=envelope.idempotency_key,
            credential=envelope.credential,
        )

        res = PeerCcccHttpTransport(http_post=fake_post).deliver(envelope)

        self.assertTrue(res.ok)
        self.assertEqual(captured["body"].get("source_platform"), "peer_cccc_http")
        self.assertEqual(captured["body"].get("src_group_id"), "g_local")
        self.assertEqual(captured["body"].get("src_event_id"), "k1")
        self.assertEqual(captured["body"].get("source_user_id"), "peer-local")

    def test_success_reads_event_id_from_result_envelope(self) -> None:
        from cccc.daemon.federation.transports.peer_cccc_http import PeerCcccHttpTransport

        # CCCC Web /send returns an envelope {ok, result: {event: {id}}}.
        t = PeerCcccHttpTransport(http_post=lambda *a, **k: (200, {"ok": True, "result": {"event": {"id": "remote-123"}}}))
        res = t.deliver(self._envelope())
        self.assertTrue(res.ok)
        self.assertEqual(res.remote_event_id, "remote-123")

    def test_success_reads_event_id_from_legacy_result_envelope(self) -> None:
        from cccc.daemon.federation.transports.peer_cccc_http import PeerCcccHttpTransport

        t = PeerCcccHttpTransport(http_post=lambda *a, **k: (200, {"ok": True, "result": {"event_id": "legacy-123"}}))
        res = t.deliver(self._envelope())
        self.assertTrue(res.ok)
        self.assertEqual(res.remote_event_id, "legacy-123")

    def test_2xx_error_envelope_is_permanent_failure(self) -> None:
        from cccc.daemon.federation.transports.peer_cccc_http import PeerCcccHttpTransport

        t = PeerCcccHttpTransport(
            http_post=lambda *a, **k: (
                200,
                {"ok": False, "error": {"code": "invalid_recipient", "message": "unknown recipient"}},
            )
        )
        res = t.deliver(self._envelope())
        self.assertFalse(res.ok)
        self.assertEqual(res.status, "failed")
        self.assertFalse(res.retriable)
        self.assertEqual(res.remote_event_id or "", "")
        self.assertEqual(res.error_code, "invalid_recipient")
        self.assertEqual(res.error_message, "unknown recipient")

    def test_4xx_is_permanent_5xx_is_transient(self) -> None:
        from cccc.daemon.federation.transports.peer_cccc_http import PeerCcccHttpTransport

        perm = PeerCcccHttpTransport(http_post=lambda *a, **k: (403, {"error": "forbidden"}))
        rperm = perm.deliver(self._envelope())
        self.assertFalse(rperm.ok)
        self.assertFalse(rperm.retriable)

        trans = PeerCcccHttpTransport(http_post=lambda *a, **k: (503, {"error": "unavailable"}))
        rtrans = trans.deliver(self._envelope())
        self.assertFalse(rtrans.ok)
        self.assertTrue(rtrans.retriable)

    def test_connection_exception_is_transient(self) -> None:
        from cccc.daemon.federation.transports.peer_cccc_http import PeerCcccHttpTransport

        def boom(*a, **k):
            raise ConnectionError("refused")

        t = PeerCcccHttpTransport(http_post=boom)
        res = t.deliver(self._envelope())
        self.assertFalse(res.ok)
        self.assertTrue(res.retriable)

    def _libp2p_envelope(self, *, attachments=None, refs=None):
        from cccc.contracts.v1.federation import RemoteSendPayload
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget

        return RemoteMessageEnvelope(
            transport="libp2p_cccc",
            src_group_id="g_local",
            source_peer_id="peer-local",
            target=RemoteTarget(
                url="libp2p://peer-remote",
                remote_group_id="g_remote",
                remote_peer_id="peer-remote",
                multiaddrs=("/ip4/127.0.0.1/tcp/4001/p2p/peer-remote",),
            ),
            payload=RemoteSendPayload(
                text="hi",
                to=["@foreman"],
                attachments=attachments or [],
                refs=refs or [],
            ),
            idempotency_key="k1",
            credential="secret-token",
        )

    def test_libp2p_fake_client_receives_target_and_payload(self) -> None:
        from cccc.daemon.federation.transports.libp2p_cccc import Libp2pCcccTransport

        captured = {}

        def fake_send(request):
            captured["request"] = request
            return {"ok": True, "event_id": "remote-123"}

        res = Libp2pCcccTransport(client=fake_send).deliver(self._libp2p_envelope())
        self.assertTrue(res.ok)
        self.assertEqual(res.status, "sent")
        self.assertEqual(res.remote_event_id, "remote-123")
        req = captured["request"]
        self.assertEqual(req.remote_peer_id, "peer-remote")
        self.assertEqual(req.src_group_id, "g_local")
        self.assertEqual(req.multiaddrs, ("/ip4/127.0.0.1/tcp/4001/p2p/peer-remote",))
        self.assertEqual(req.remote_group_id, "g_remote")
        self.assertEqual(req.payload["text"], "hi")
        self.assertEqual(req.payload["source_platform"], "libp2p_cccc")
        self.assertEqual(req.payload["src_group_id"], "g_local")
        self.assertEqual(req.idempotency_key, "k1")
        self.assertEqual(req.credential, "secret-token")

    def test_libp2p_resolves_missing_multiaddr_from_address_book(self) -> None:
        import tempfile
        from pathlib import Path

        from cccc.daemon.federation.peer_address_book import record_peer_addresses
        from cccc.daemon.federation.transports.libp2p_cccc import Libp2pCcccTransport

        captured = {}
        resolved_addr = "/ip4/127.0.0.1/tcp/4101/p2p/peer-remote"

        def fake_send(request):
            captured["request"] = request
            return {"ok": True, "event_id": "remote-123"}

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            record_peer_addresses(
                "peer-remote",
                [resolved_addr],
                remote_group_id="g_remote",
                home=home,
            )
            envelope = self._libp2p_envelope()
            envelope = type(envelope)(
                transport=envelope.transport,
                src_group_id=envelope.src_group_id,
                source_peer_id=envelope.source_peer_id,
                target=type(envelope.target)(
                    url=envelope.target.url,
                    remote_group_id=envelope.target.remote_group_id,
                    remote_peer_id=envelope.target.remote_peer_id,
                    multiaddrs=(),
                ),
                payload=envelope.payload,
                idempotency_key=envelope.idempotency_key,
                credential=envelope.credential,
            )
            res = Libp2pCcccTransport(client=fake_send, address_book_home=home).deliver(envelope)

        self.assertTrue(res.ok)
        self.assertEqual(res.remote_event_id, "remote-123")
        self.assertEqual(captured["request"].multiaddrs, (resolved_addr,))

    def test_libp2p_missing_multiaddr_is_transient_address_unresolved(self) -> None:
        from cccc.daemon.federation.transports.libp2p_cccc import Libp2pCcccTransport

        envelope = self._libp2p_envelope()
        envelope = type(envelope)(
            transport=envelope.transport,
            src_group_id=envelope.src_group_id,
            source_peer_id=envelope.source_peer_id,
            target=type(envelope.target)(
                url=envelope.target.url,
                remote_group_id=envelope.target.remote_group_id,
                remote_peer_id=envelope.target.remote_peer_id,
                multiaddrs=(),
            ),
            payload=envelope.payload,
            idempotency_key=envelope.idempotency_key,
            credential=envelope.credential,
        )
        res = Libp2pCcccTransport(client=lambda request: {"ok": True, "event_id": "remote-123"}).deliver(envelope)

        self.assertFalse(res.ok)
        self.assertTrue(res.retriable)
        self.assertEqual(res.error_code, "address_unresolved")

    def test_libp2p_default_client_without_reachable_node_is_transient(self) -> None:
        from cccc.daemon.federation.transports.libp2p_cccc import Libp2pCcccTransport

        res = Libp2pCcccTransport().deliver(self._libp2p_envelope())
        self.assertFalse(res.ok)
        self.assertEqual(res.status, "failed")
        self.assertTrue(res.retriable)
        self.assertIn(res.error_code, {"libp2p_delivery_failed", "libp2p_dial_failed"})

    def test_libp2p_error_envelope_is_permanent_failure(self) -> None:
        from cccc.daemon.federation.transports.libp2p_cccc import Libp2pCcccTransport

        t = Libp2pCcccTransport(
            client=lambda request: {
                "ok": False,
                "error": {"code": "invalid_recipient", "message": "unknown recipient"},
            }
        )
        res = t.deliver(self._libp2p_envelope())
        self.assertFalse(res.ok)
        self.assertFalse(res.retriable)
        self.assertEqual(res.error_code, "invalid_recipient")
        self.assertEqual(res.error_message, "unknown recipient")

    def test_libp2p_connection_exception_is_transient(self) -> None:
        from cccc.daemon.federation.transports.libp2p_cccc import Libp2pCcccTransport

        def boom(request):
            raise ConnectionError("peer offline")

        res = Libp2pCcccTransport(client=boom).deliver(self._libp2p_envelope())
        self.assertFalse(res.ok)
        self.assertTrue(res.retriable)
        self.assertEqual(res.error_code, "libp2p_delivery_failed")

    def test_libp2p_unsupported_attachments_and_refs_are_permanent(self) -> None:
        from cccc.daemon.federation.transports.libp2p_cccc import Libp2pCcccTransport

        t = Libp2pCcccTransport(client=lambda request: {"ok": True, "event_id": "remote-123"})

        res_att = t.deliver(self._libp2p_envelope(attachments=[{"path": "x"}]))
        self.assertFalse(res_att.ok)
        self.assertFalse(res_att.retriable)
        self.assertEqual(res_att.error_code, "unsupported_attachments")

        res_refs = t.deliver(self._libp2p_envelope(refs=[{"kind": "url"}]))
        self.assertFalse(res_refs.ok)
        self.assertFalse(res_refs.retriable)
        self.assertEqual(res_refs.error_code, "unsupported_refs")


if __name__ == "__main__":
    unittest.main()
