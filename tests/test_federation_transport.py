import unittest


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

    def _envelope(self, *, attachments=None, refs=None):
        from cccc.contracts.v1.federation import RemoteSendPayload
        from cccc.daemon.federation.transports.base import RemoteMessageEnvelope, RemoteTarget

        return RemoteMessageEnvelope(
            transport="peer_cccc_http",
            target=RemoteTarget(url="https://peer.example", remote_group_id="g_remote"),
            payload=RemoteSendPayload(
                text="hi",
                attachments=attachments or [],
                refs=refs or [],
            ),
            idempotency_key="k1",
            credential="secret-token",
        )

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


if __name__ == "__main__":
    unittest.main()
