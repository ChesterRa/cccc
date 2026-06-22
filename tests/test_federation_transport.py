import unittest
import os
import tempfile
import asyncio
import base64
import hashlib
from pathlib import Path
from unittest.mock import patch


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


def _write_local_group_with_federation_trust(home: Path) -> Path:
    from cccc.kernel.federation.pairing import _save_store

    group_dir = home / "groups" / "g_local"
    group_dir.mkdir(parents=True)
    (group_dir / "group.yaml").write_text(
        "\n".join(
            [
                "v: 1",
                "group_id: g_local",
                "title: Local",
                "state: active",
                "active_scope_key: ''",
                "scopes: []",
                "actors:",
                "  - id: peer1",
                "    title: Peer 1",
                "    runtime: codex",
                "    enabled: true",
            ]
        )
        + "\n",
        encoding="utf-8",
    )
    _save_store(
        {
            "invites": {},
            "requests": {},
            "outbounds": {},
            "trusts": {
                "ptrust_1": {
                    "trust_id": "ptrust_1",
                    "group_id": "g_local",
                    "remote_group_id": "g_remote",
                    "remote_peer_id": "peer_remote",
                    "registration_id": "reg_remote",
                    "transport": "federation_session",
                    "remote_group_title": "Remote Group",
                    "status": "active",
                    "created_at": "2026-01-01T00:00:00Z",
                    "updated_at": "2026-01-01T00:00:00Z",
                }
            },
        },
        home=home,
    )
    return group_dir


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

        session = get_transport("federation_session")
        self.assertEqual(session.transport, "federation_session")

        with self.assertRaises(UnknownTransportError):
            get_transport("peer_cccc_http")

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
                        url="federation-session://peer_remote",
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
                        url="federation-session://peer_a",
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
                        url="federation-session://peer_a",
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
                    url="federation-session://peer_remote",
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

    def test_federation_session_client_keeps_session_active_across_idle_recv_timeout(self) -> None:
        import socket
        import threading
        import time

        from cccc.daemon.federation.ws_client import connect_federation_session_once
        from cccc.daemon.federation.ws_session import clear_sessions, send_via_session_sync

        class FakeWs:
            def __init__(self) -> None:
                self.sent = []
                self.frames = [{"ok": True, "type": "ready"}]
                self.outbound_request_sent = threading.Event()
                self.outbound_response_sent = False
                self.closed = False

            def send(self, raw):
                import json

                payload = json.loads(raw)
                self.sent.append(payload)
                if payload.get("type") == "request" and (payload.get("payload") or {}).get("text") == "outbound":
                    self.outbound_request_sent.set()

            def recv(self):
                import json

                if self.frames:
                    return json.dumps(self.frames.pop(0))
                if self.outbound_request_sent.wait(0.01):
                    for sent in self.sent:
                        if sent.get("type") == "request" and (sent.get("payload") or {}).get("text") == "outbound":
                            if self.outbound_response_sent:
                                raise RuntimeError("closed")
                            self.outbound_response_sent = True
                            return json.dumps(
                                {
                                    "type": "response",
                                    "response_to": sent.get("request_id"),
                                    "result": {"ok": True, "event_id": "remote-after-idle"},
                                }
                            )
                raise socket.timeout("idle")

            def close(self):
                self.closed = True

        fake_ws = FakeWs()
        outbound_result = {}
        outbound_done = threading.Event()

        def send_outbound_after_idle() -> None:
            time.sleep(0.05)
            try:
                outbound_result.update(
                    send_via_session_sync(
                        target_group_id="g_local",
                        src_group_id="g_remote",
                        remote_peer_id="peer_remote",
                        request={"op": "remote_send", "payload": {"text": "outbound"}},
                        timeout=1.0,
                    )
                )
            finally:
                outbound_done.set()

        clear_sessions()
        try:
            result = connect_federation_session_once(
                remote_base_url="http://peer.example:8848",
                local_group_id="g_local",
                remote_group_id="g_remote",
                remote_peer_id="peer_remote",
                connect=lambda url, timeout: fake_ws,
                on_ready=lambda: threading.Thread(target=send_outbound_after_idle, daemon=True).start(),
                idle_tick_seconds=0.01,
                timeout=1.0,
            )
        finally:
            outbound_done.wait(2.0)
            clear_sessions()

        self.assertFalse(result["ok"])
        self.assertEqual(result["error"]["code"], "session_closed")
        self.assertEqual(outbound_result["event_id"], "remote-after-idle")
        self.assertTrue(any(sent.get("type") == "ping" for sent in fake_ws.sent))

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

    def test_federation_session_endpoint_routes_remote_send_to_daemon_when_web_supervised(self) -> None:
        from cccc.daemon.federation.ws_endpoint import handle_federation_session_request

        with (
            _EnvPatch(CCCC_WEB_SUPERVISED="1"),
            patch(
                "cccc.daemon.server.call_daemon",
                return_value={"ok": True, "result": {"ok": True, "event_id": "evt-daemon", "duplicate": False}},
            ) as call_daemon,
            patch("cccc.daemon.federation.ws_endpoint.receive_remote_send") as local_receive,
        ):
            result = handle_federation_session_request(
                {
                    "op": "remote_send",
                    "target_group_id": "g_local",
                    "src_group_id": "g_remote",
                    "payload": {"text": "hi", "to": ["@foreman"]},
                    "idempotency_key": "remote-1",
                },
                target_group_id="g_local",
                src_group_id="g_remote",
                remote_peer_id="peer_remote",
            )

        self.assertTrue(result["ok"])
        self.assertEqual(result["event_id"], "evt-daemon")
        local_receive.assert_not_called()
        call_daemon.assert_called_once_with(
            {
                "op": "federation_receive_remote_send",
                "args": {
                    "target_group_id": "g_local",
                    "src_group_id": "g_remote",
                    "remote_peer_id": "peer_remote",
                    "payload": {"text": "hi", "to": ["@foreman"]},
                    "idempotency_key": "remote-1",
                },
            }
        )

    def test_federation_session_client_reports_connect_failure_without_escaping(self) -> None:
        from cccc.daemon.federation.ws_client import connect_federation_session_once

        def connect(url, timeout):
            raise RuntimeError("HTTP error: 401 Unauthorized")

        result = connect_federation_session_once(
            remote_base_url="http://peer.example:8848",
            local_group_id="g_local",
            remote_group_id="g_remote",
            remote_peer_id="peer_remote",
            connect=connect,
            timeout=2.0,
        )

        self.assertFalse(result["ok"])
        self.assertEqual(result["error"]["code"], "session_connect_failed")
        self.assertIn("401 Unauthorized", result["error"]["message"])

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
                "transport": "registry_hub",
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

    def _session_envelope(self, *, attachments=None, refs=None):
        from cccc.contracts.v1.federation import RemoteSendPayload
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget

        return RemoteMessageEnvelope(
            transport="federation_session",
            src_group_id="g_local",
            source_peer_id="peer-local",
            target=RemoteTarget(
                url="federation-session://peer-remote",
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

    def test_federation_session_transport_routes_to_web_owner_when_daemon_has_no_session(self) -> None:
        from cccc.daemon.federation.transports.federation_session import FederationSessionTransport

        with patch(
            "cccc.daemon.federation.transports.federation_session._send_session_request_via_web_owner",
            return_value={"ok": True, "event_id": "remote-via-web-owner"},
        ) as fallback:
            res = FederationSessionTransport().deliver(self._session_envelope())

        self.assertTrue(res.ok)
        self.assertEqual(res.remote_event_id, "remote-via-web-owner")
        fallback.assert_called_once()
        self.assertEqual(fallback.call_args.kwargs["local_group_id"], "g_local")
        self.assertEqual(fallback.call_args.kwargs["remote_group_id"], "g_remote")
        self.assertEqual(fallback.call_args.kwargs["remote_peer_id"], "peer-remote")

    def test_federation_session_transport_sends_attachment_payloads(self) -> None:
        from cccc.daemon.federation.transports.federation_session import FederationSessionTransport
        from cccc.daemon.federation.ws_session import FederationWsSession, clear_sessions, register_session
        from cccc.kernel.blobs import store_blob_bytes
        from cccc.kernel.group import load_group

        captured = {}

        async def send_request(request, timeout):
            _ = timeout
            captured["request"] = dict(request)
            return {"ok": True, "event_id": "remote-with-attachment"}

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            _write_local_group_with_federation_trust(home)
            with _EnvPatch(CCCC_HOME=str(home)):
                group = load_group("g_local")
                self.assertIsNotNone(group)
                attachment = store_blob_bytes(group, data=b"image-bytes", filename="shot.png", mime_type="image/png")
                clear_sessions()
                asyncio.run(
                    register_session(
                        FederationWsSession(
                            target_group_id="g_local",
                            src_group_id="g_remote",
                            remote_peer_id="peer-remote",
                            send_request=send_request,
                        )
                    )
                )
                try:
                    result = FederationSessionTransport().deliver(self._session_envelope(attachments=[attachment]))
                finally:
                    clear_sessions()

        self.assertTrue(result.ok, result)
        payload = captured["request"]["payload"]
        attachments = payload["attachments"]
        self.assertEqual(len(attachments), 1)
        self.assertEqual(attachments[0]["title"], "shot.png")
        self.assertEqual(attachments[0]["mime_type"], "image/png")
        self.assertEqual(attachments[0]["sha256"], hashlib.sha256(b"image-bytes").hexdigest())
        self.assertEqual(base64.b64decode(attachments[0]["content_base64"].encode("ascii")), b"image-bytes")

    def test_federation_session_unsupported_refs_are_permanent(self) -> None:
        from cccc.daemon.federation.transports.federation_session import FederationSessionTransport

        res_refs = FederationSessionTransport().deliver(self._session_envelope(refs=[{"kind": "url"}]))
        self.assertFalse(res_refs.ok)
        self.assertFalse(res_refs.retriable)
        self.assertEqual(res_refs.error_code, "unsupported_refs")

    def test_receive_remote_send_delivers_once_and_deduplicates(self) -> None:
        from cccc.contracts.v1.message import ChatMessageData
        from cccc.daemon.federation.receiver import receive_remote_send
        from cccc.kernel.ledger import append_event

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            group_dir = _write_local_group_with_federation_trust(home)

            deliveries = []

            def capture_delivery(**kwargs):
                deliveries.append(kwargs)

            with (
                _EnvPatch(CCCC_CHAT_POST_COMMIT_MODE="inline"),
                patch("cccc.daemon.federation.receiver.deliver_appended_chat_message", side_effect=capture_delivery),
            ):
                first = receive_remote_send(
                    target_group_id="g_local",
                    src_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    payload={"text": "hello from remote", "to": ["peer1"], "priority": "attention"},
                    idempotency_key="remote-client-1",
                    home=home,
                )
                for idx in range(1005):
                    append_event(
                        group_dir / "ledger.jsonl",
                        kind="chat.message",
                        group_id="g_local",
                        scope_key="",
                        by="user",
                        data=ChatMessageData(text=f"filler {idx}", client_id=f"filler-{idx}").model_dump(),
                    )
                duplicate = receive_remote_send(
                    target_group_id="g_local",
                    src_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    payload={"text": "hello from remote", "to": ["peer1"], "priority": "attention"},
                    idempotency_key="remote-client-1",
                    home=home,
                )

        self.assertTrue(first["ok"], first)
        self.assertFalse(first["duplicate"])
        self.assertTrue(duplicate["ok"], duplicate)
        self.assertTrue(duplicate["duplicate"])
        self.assertEqual(duplicate["event_id"], first["event_id"])
        self.assertEqual(len(deliveries), 1)
        delivery = deliveries[0]
        self.assertEqual(delivery["by"], "federation:peer_remote")
        self.assertEqual(delivery["effective_to"], ["peer1"])
        self.assertEqual(delivery["priority"], "attention")
        self.assertFalse(delivery["reply_required"])
        self.assertEqual(str(delivery["event"].get("id") or ""), first["event_id"])
        self.assertEqual(delivery["text"], "hello from remote")
        self.assertEqual(delivery["source_user_name"], "Remote Group")

    def test_receive_remote_send_stores_attachments_as_local_blobs(self) -> None:
        from cccc.daemon.federation.receiver import receive_remote_send
        from cccc.kernel.blobs import resolve_blob_attachment_path
        from cccc.kernel.group import load_group
        from cccc.kernel.inbox import iter_events

        raw = b"remote image bytes"
        digest = hashlib.sha256(raw).hexdigest()

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            group_dir = _write_local_group_with_federation_trust(home)
            deliveries = []

            def capture_delivery(**kwargs):
                deliveries.append(kwargs)

            with (
                _EnvPatch(CCCC_CHAT_POST_COMMIT_MODE="inline", CCCC_HOME=str(home)),
                patch("cccc.daemon.federation.receiver.deliver_appended_chat_message", side_effect=capture_delivery),
            ):
                result = receive_remote_send(
                    target_group_id="g_local",
                    src_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    payload={
                        "text": "see attachment",
                        "to": ["peer1"],
                        "attachments": [
                            {
                                "kind": "image",
                                "title": "remote.png",
                                "mime_type": "image/png",
                                "bytes": len(raw),
                                "sha256": digest,
                                "content_base64": base64.b64encode(raw).decode("ascii"),
                            }
                        ],
                    },
                    idempotency_key="remote-client-attachment-1",
                    home=home,
                )
                group = load_group("g_local")

            self.assertTrue(result["ok"], result)
            events = [event for event in iter_events(group_dir / "ledger.jsonl") if event.get("kind") == "chat.message"]
            self.assertEqual(len(events), 1)
            attachments = ((events[0].get("data") or {}).get("attachments") or [])
            self.assertEqual(len(attachments), 1)
            self.assertEqual(attachments[0]["title"], "remote.png")
            self.assertEqual(attachments[0]["sha256"], digest)
            self.assertIsNotNone(group)
            stored = resolve_blob_attachment_path(group, rel_path=attachments[0]["path"])
            self.assertEqual(stored.read_bytes(), raw)
            self.assertEqual(len(deliveries), 1)
            self.assertEqual(deliveries[0]["attachments"], attachments)

    def test_receive_remote_send_requires_explicit_recipient(self) -> None:
        from cccc.daemon.federation.receiver import receive_remote_send
        from cccc.kernel.inbox import iter_events

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            group_dir = _write_local_group_with_federation_trust(home)

            with (
                _EnvPatch(CCCC_CHAT_POST_COMMIT_MODE="inline"),
                patch("cccc.daemon.federation.receiver.deliver_appended_chat_message") as deliver,
            ):
                result = receive_remote_send(
                    target_group_id="g_local",
                    src_group_id="g_remote",
                    remote_peer_id="peer_remote",
                    payload={"text": "hello from remote"},
                    idempotency_key="remote-client-1",
                    home=home,
                )

            self.assertFalse(result["ok"], result)
            self.assertEqual(result["error"]["code"], "missing_remote_recipient")
            deliver.assert_not_called()
            events = [event for event in iter_events(group_dir / "ledger.jsonl") if event.get("kind") == "chat.message"]
            self.assertEqual(events, [])


if __name__ == "__main__":
    unittest.main()
