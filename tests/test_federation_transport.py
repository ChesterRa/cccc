import unittest
import os
import tempfile
import asyncio
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
        from cccc.daemon.federation.transports.base import UnknownTransportError, get_transport

        t = get_transport("peer_cccc_http")
        self.assertEqual(t.transport, "peer_cccc_http")

        session = get_transport("federation_session")
        self.assertEqual(session.transport, "federation_session")

        with self.assertRaises(UnknownTransportError):
            get_transport("libp2p_cccc")

    def test_federation_session_transport_uses_active_websocket_session(self) -> None:
        from cccc.contracts.v1.federation import RemoteSendPayload
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget
        from cccc.daemon.federation.transports.federation_session import FederationSessionTransport
        from cccc.daemon.federation.ws_session import FederationWsSession, clear_sessions, register_session

        captured = {}

        async def send_request(request, timeout):
            captured["request"] = dict(request)
            captured["timeout"] = timeout
            return {"ok": True, "event_id": "remote-via-ws"}

        clear_sessions()
        asyncio.run(
            register_session(
                FederationWsSession(
                    target_group_id="g_local",
                    src_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    send_request=send_request,
                )
            )
        )
        try:
            result = FederationSessionTransport().deliver(
                RemoteMessageEnvelope(
                    transport="federation_session",
                    src_group_id="g_local",
                    source_peer_id="peer_local",
                    target=RemoteTarget(
                        url="session://peer_remote",
                        remote_group_id="g_remote",
                        remote_peer_id="peer_remote",
                        multiaddrs=("/ip4/127.0.0.1/tcp/4001/p2p/peer_remote",),
                    ),
                    payload=RemoteSendPayload(text="hi", to=["@foreman"]),
                    idempotency_key="k-ws",
                )
            )
        finally:
            clear_sessions()

        self.assertTrue(result.ok)
        self.assertEqual(result.remote_event_id, "remote-via-ws")
        self.assertEqual(captured["request"]["op"], "remote_send")
        self.assertEqual(captured["request"]["payload"]["text"], "hi")

    def test_federation_session_transport_uses_inbound_session_for_reverse_send(self) -> None:
        from cccc.contracts.v1.federation import RemoteSendPayload
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget
        from cccc.daemon.federation.transports.federation_session import FederationSessionTransport
        from cccc.daemon.federation.ws_session import FederationWsSession, clear_sessions, register_session

        captured = {}

        async def send_request(request, timeout):
            captured["request"] = dict(request)
            return {"ok": True, "event_id": "remote-reverse"}

        clear_sessions()
        asyncio.run(
            register_session(
                FederationWsSession(
                    # Peer A connected to local B, so B stores the session under
                    # target=B/local and src=A/remote. B can later send back to A
                    # without dialing A.
                    target_group_id="g_b",
                    src_group_id="g_a",
                    remote_peer_id="peer_a",
                    send_request=send_request,
                )
            )
        )
        try:
            result = FederationSessionTransport().deliver(
                RemoteMessageEnvelope(
                    transport="federation_session",
                    src_group_id="g_b",
                    source_peer_id="peer_b",
                    target=RemoteTarget(
                        url="session://peer_a",
                        remote_group_id="g_a",
                        remote_peer_id="peer_a",
                        multiaddrs=("/ip4/127.0.0.1/tcp/4001/p2p/peer_a",),
                    ),
                    payload=RemoteSendPayload(text="reply", to=["@foreman"]),
                    idempotency_key="k-reverse",
                )
            )
        finally:
            clear_sessions()

        self.assertTrue(result.ok)
        self.assertEqual(result.remote_event_id, "remote-reverse")
        self.assertEqual(captured["request"]["src_group_id"], "g_b")
        self.assertEqual(captured["request"]["target_group_id"], "g_a")

    def test_federation_session_transport_uses_websocket_session_without_multiaddr(self) -> None:
        from cccc.contracts.v1.federation import RemoteSendPayload
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget
        from cccc.daemon.federation.transports.federation_session import FederationSessionTransport
        from cccc.daemon.federation.ws_session import FederationWsSession, clear_sessions, register_session

        async def send_request(request, timeout):
            return {"ok": True, "event_id": "remote-private"}

        clear_sessions()
        asyncio.run(
            register_session(
                FederationWsSession(
                    target_group_id="g_b",
                    src_group_id="g_a",
                    remote_peer_id="peer_a",
                    send_request=send_request,
                )
            )
        )
        try:
            result = FederationSessionTransport().deliver(
                RemoteMessageEnvelope(
                    transport="federation_session",
                    src_group_id="g_b",
                    source_peer_id="peer_b",
                    target=RemoteTarget(
                        url="session://peer_a",
                        remote_group_id="g_a",
                        remote_peer_id="peer_a",
                        multiaddrs=(),
                    ),
                    payload=RemoteSendPayload(text="reply", to=["@foreman"]),
                    idempotency_key="k-private",
                )
            )
        finally:
            clear_sessions()

        self.assertTrue(result.ok)
        self.assertEqual(result.remote_event_id, "remote-private")

    def test_federation_session_transport_does_not_fallback_to_direct_multiaddr(self) -> None:
        from cccc.contracts.v1.federation import RemoteSendPayload
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget
        from cccc.daemon.federation.transports.federation_session import FederationSessionTransport

        result = FederationSessionTransport().deliver(
            RemoteMessageEnvelope(
                transport="federation_session",
                src_group_id="g_local",
                source_peer_id="peer_local",
                target=RemoteTarget(
                    url="session://peer_remote",
                    remote_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    multiaddrs=("/ip4/127.0.0.1/tcp/4001/p2p/peer_remote",),
                ),
                payload=RemoteSendPayload(text="hi", to=["@foreman"]),
                idempotency_key="k-session-only",
            )
        )

        self.assertFalse(result.ok)
        self.assertTrue(result.retriable)
        self.assertEqual(result.error_code, "peer_session_unavailable")

    def test_websocket_session_sync_send_uses_session_event_loop(self) -> None:
        import threading

        from cccc.daemon.federation.ws_session import FederationWsSession, clear_sessions, register_session, send_via_session_sync

        loop_ready = threading.Event()
        stop = threading.Event()
        holder = {}

        def loop_thread() -> None:
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)

            async def send_request(request, timeout):
                holder["request"] = dict(request)
                holder["thread"] = threading.current_thread().name
                return {"ok": True, "event_id": "remote-loop"}

            async def setup() -> None:
                await register_session(
                    FederationWsSession(
                        target_group_id="g_local",
                        src_group_id="g_remote",
                        remote_peer_id="peer_remote",
                        send_request=send_request,
                        loop=loop,
                    )
                )
                loop_ready.set()
                while not stop.is_set():
                    await asyncio.sleep(0.01)

            loop.run_until_complete(setup())
            loop.close()

        clear_sessions()
        thread = threading.Thread(target=loop_thread, name="ws-session-loop", daemon=True)
        thread.start()
        self.assertTrue(loop_ready.wait(2.0))
        try:
            result = send_via_session_sync(
                target_group_id="g_local",
                src_group_id="g_remote",
                remote_peer_id="peer_remote",
                request={"op": "remote_send"},
                timeout=1.0,
            )
        finally:
            stop.set()
            thread.join(timeout=2.0)
            clear_sessions()

        self.assertTrue(result["ok"])
        self.assertEqual(result["event_id"], "remote-loop")
        self.assertEqual(holder["thread"], "ws-session-loop")

    def test_federation_session_client_uses_remote_ws_url_and_handles_requests(self) -> None:
        import threading

        from cccc.daemon.federation.ws_client import connect_federation_session_once, federation_session_ws_url
        from cccc.daemon.federation.ws_session import clear_sessions, send_via_session_sync

        class FakeWs:
            def __init__(self) -> None:
                self.sent = []
                self.frames = [
                    {"ok": True, "type": "ready"},
                    {"type": "request", "request_id": "req-1", "op": "remote_send", "payload": {"text": "hi"}},
                ]
                self.outbound_request_sent = threading.Event()
                self.outbound_response_sent = False

            def send(self, raw):
                import json

                payload = json.loads(raw)
                self.sent.append(payload)
                if payload.get("type") == "request" and (payload.get("payload") or {}).get("text") == "outbound":
                    self.outbound_request_sent.set()

            def recv(self):
                import json

                if not self.frames:
                    self.outbound_request_sent.wait(1.0)
                if not self.frames and not self.outbound_response_sent:
                    for sent in self.sent:
                        if sent.get("type") == "request" and (sent.get("payload") or {}).get("text") == "outbound":
                            self.outbound_response_sent = True
                            self.frames.append(
                                {
                                    "type": "response",
                                    "response_to": sent.get("request_id"),
                                    "result": {"ok": True, "event_id": "remote-over-outbound"},
                                }
                            )
                            break
                if not self.frames:
                    raise RuntimeError("closed")
                return json.dumps(self.frames.pop(0))

            def close(self):
                self.closed = True

        fake_ws = FakeWs()
        captured = {}

        def connect(url, timeout):
            captured["url"] = url
            captured["timeout"] = timeout
            return fake_ws

        outbound_result = {}
        outbound_done = threading.Event()

        def send_outbound() -> None:
            try:
                outbound_result.update(
                    send_via_session_sync(
                        target_group_id="g_local",
                        src_group_id="g_remote",
                        remote_peer_id="peer_remote",
                        request={"op": "remote_send", "payload": {"text": "outbound"}},
                        timeout=2.0,
                    )
                )
            finally:
                outbound_done.set()

        self.assertEqual(federation_session_ws_url("https://peer.example/base"), "wss://peer.example/api/federation/session/ws")
        clear_sessions()
        try:
            result = connect_federation_session_once(
                remote_base_url="http://peer.example:8848",
                local_group_id="g_local",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
                connect=connect,
                handle_request=lambda frame: {"ok": True, "event_id": "handled"},
                on_ready=lambda: threading.Thread(target=send_outbound, daemon=True).start(),
                timeout=2.0,
            )
        finally:
            outbound_done.wait(2.0)
            clear_sessions()

        self.assertFalse(result["ok"])
        self.assertEqual(result["error"]["code"], "session_closed")
        self.assertEqual(captured["url"], "ws://peer.example:8848/api/federation/session/ws")
        self.assertEqual(fake_ws.sent[0]["target_group_id"], "g_remote")
        self.assertEqual(fake_ws.sent[0]["src_group_id"], "g_local")
        self.assertTrue(fake_ws.sent[0]["remote_peer_id"].startswith("12D3Koo"))
        self.assertTrue(fake_ws.sent[0]["public_key"])
        self.assertTrue(fake_ws.sent[0]["signature"])
        self.assertEqual(fake_ws.sent[1]["type"], "response")
        self.assertEqual(fake_ws.sent[1]["response_to"], "req-1")
        self.assertEqual(fake_ws.sent[1]["result"]["event_id"], "handled")
        self.assertEqual(fake_ws.sent[2]["type"], "request")
        self.assertEqual(fake_ws.sent[2]["payload"]["text"], "outbound")
        self.assertEqual(outbound_result["event_id"], "remote-over-outbound")

    def test_federation_session_client_default_handler_uses_connected_remote_peer_id(self) -> None:
        from unittest.mock import patch

        from cccc.daemon.federation.ws_client import connect_federation_session_once

        class FakeWs:
            def __init__(self) -> None:
                self.sent = []
                self.frames = [
                    {"ok": True, "type": "ready"},
                    {
                        "type": "request",
                        "request_id": "req-1",
                        "op": "remote_send",
                        "target_group_id": "g_local",
                        "src_group_id": "g_remote",
                        "remote_peer_id": "peer_local",
                        "payload": {"text": "hi"},
                    },
                ]

            def send(self, raw):
                import json

                self.sent.append(json.loads(raw))

            def recv(self):
                import json

                if not self.frames:
                    raise RuntimeError("closed")
                return json.dumps(self.frames.pop(0))

            def close(self):
                self.closed = True

        captured = {}

        def handle(frame, **kwargs):
            captured["kwargs"] = dict(kwargs)
            return {"ok": True, "event_id": "handled"}

        with (
            patch("cccc.daemon.federation.ws_client.get_local_identity", return_value={"peer_id": "peer_local"}),
            patch("cccc.daemon.federation.ws_endpoint.handle_federation_session_request", side_effect=handle),
        ):
            result = connect_federation_session_once(
                remote_base_url="http://peer.example:8848",
                local_group_id="g_local",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
                connect=lambda url, timeout: FakeWs(),
                timeout=2.0,
            )

        self.assertFalse(result["ok"])
        self.assertEqual(result["error"]["code"], "session_closed")
        self.assertEqual(captured["kwargs"]["remote_peer_id"], "peer_remote")
        self.assertEqual(captured["kwargs"]["target_group_id"], "g_local")
        self.assertEqual(captured["kwargs"]["src_group_id"], "g_remote")

    def test_federation_session_manager_starts_one_client_per_active_trust_with_endpoint(self) -> None:
        import threading
        from pathlib import Path

        from cccc.daemon.federation.ws_manager import tick_federation_session_clients

        started = []

        class AliveThread:
            def is_alive(self) -> bool:
                return True

        def start_client(**kwargs):
            started.append(kwargs)
            return AliveThread()

        trusts = [
            {
                "trust_id": "t1",
                "status": "active",
                "transport": "peer_cccc_http",
                "group_id": "g_local",
                "remote_group_id": "g_remote",
                "remote_peer_id": "peer_remote",
                "remote_endpoint": "http://peer.example:8848",
            },
            {
                "trust_id": "t2",
                "status": "active",
                "transport": "federation_session",
                "group_id": "g_local",
                "remote_group_id": "g_session",
                "remote_peer_id": "peer_session",
                "remote_endpoint": "http://session.example:8848",
            },
            {
                "trust_id": "t3",
                "status": "active",
                "transport": "federation_session",
                "group_id": "g_local",
                "remote_group_id": "g_no_endpoint",
                "remote_peer_id": "peer_no_endpoint",
            },
            {
                "trust_id": "t4",
                "status": "revoked",
                "transport": "federation_session",
                "group_id": "g_local",
                "remote_group_id": "g_revoked",
                "remote_peer_id": "peer_revoked",
                "remote_endpoint": "http://revoked.example",
            },
        ]

        state = {}
        result = tick_federation_session_clients(
            home=Path("/tmp/cccc-test-home"),
            stop_event=threading.Event(),
            state=state,
            list_trusts_fn=lambda home=None: trusts,
            start_client=start_client,
        )
        again = tick_federation_session_clients(
            home=Path("/tmp/cccc-test-home"),
            stop_event=threading.Event(),
            state=state,
            list_trusts_fn=lambda home=None: trusts,
            start_client=start_client,
        )

        self.assertEqual(result, {"started": 1, "active": 1})
        self.assertEqual(again, {"started": 0, "active": 1})
        self.assertEqual(len(started), 1)
        self.assertEqual(started[0]["remote_base_url"], "http://session.example:8848")
        self.assertEqual(started[0]["local_group_id"], "g_local")
        self.assertEqual(started[0]["remote_group_id"], "g_session")
        self.assertEqual(started[0]["remote_peer_id"], "peer_session")

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

    def test_peer_http_includes_explicit_source_multiaddrs_when_available(self) -> None:
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

    def test_remote_dispatch_does_not_advertise_local_multiaddr(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult
        from cccc.kernel.federation.registration import upsert_registration

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            old_home = os.environ.get("CCCC_HOME")
            try:
                os.environ["CCCC_HOME"] = str(home)
                registration = upsert_registration(
                    "g_local",
                    "https://peer.example",
                    remote_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    transport="peer_cccc_http",
                    status="active",
                    home=home,
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

                self.assertEqual(captured["source_multiaddrs"], ())
            finally:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home

    def test_remote_dispatch_does_not_advertise_loopback_without_routable_host(self) -> None:
        from cccc.daemon.federation.remote_dispatch import deliver_enqueued, enqueue_remote_send
        from cccc.daemon.federation.transports.base import RemoteSendResult
        from cccc.kernel.federation.registration import upsert_registration

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            with _EnvPatch(CCCC_HOME=str(home)):
                registration = upsert_registration(
                    "g_local",
                    "https://peer.example",
                    remote_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    transport="peer_cccc_http",
                    status="active",
                    home=home,
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

    def _session_envelope(self, *, attachments=None, refs=None):
        from cccc.contracts.v1.federation import RemoteSendPayload
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget

        return RemoteMessageEnvelope(
            transport="federation_session",
            src_group_id="g_local",
            source_peer_id="peer-local",
            target=RemoteTarget(
                url="session://peer-remote",
                remote_group_id="g_remote",
                remote_peer_id="peer-remote",
                multiaddrs=(),
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

    def test_federation_session_transport_without_session_is_transient(self) -> None:
        from cccc.daemon.federation.transports.federation_session import FederationSessionTransport

        res = FederationSessionTransport().deliver(self._session_envelope())

        self.assertFalse(res.ok)
        self.assertTrue(res.retriable)
        self.assertEqual(res.error_code, "peer_session_unavailable")

    def test_federation_session_unsupported_attachments_and_refs_are_permanent(self) -> None:
        from cccc.daemon.federation.transports.federation_session import FederationSessionTransport

        t = FederationSessionTransport()

        res_att = t.deliver(self._session_envelope(attachments=[{"path": "x"}]))
        self.assertFalse(res_att.ok)
        self.assertFalse(res_att.retriable)
        self.assertEqual(res_att.error_code, "unsupported_attachments")

        res_refs = t.deliver(self._session_envelope(refs=[{"kind": "url"}]))
        self.assertFalse(res_refs.ok)
        self.assertFalse(res_refs.retriable)
        self.assertEqual(res_refs.error_code, "unsupported_refs")


if __name__ == "__main__":
    unittest.main()
