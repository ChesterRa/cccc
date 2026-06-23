import unittest


class TestGroupBridgeRemotePayloads(unittest.TestCase):
    def test_build_remote_chat_payload_separates_provenance_from_idempotency(self) -> None:
        from cccc.contracts.v1.group_bridge import RemoteSendPayload
        from cccc.daemon.group_bridge.remote_payloads import build_remote_chat_payload
        from cccc.daemon.group_bridge.transports.base import RemoteMessageEnvelope, RemoteTarget

        envelope = RemoteMessageEnvelope(
            transport="group_bridge_session",
            src_group_id="g_local",
            source_peer_id="peer_source",
            target=RemoteTarget(
                url="https://peer.example",
                remote_group_id="g_remote",
                remote_peer_id="peer_target",
            ),
            payload=RemoteSendPayload(
                text="hello",
                to=["@foreman"],
                priority="attention",
                reply_required=True,
                source_by="user",
            ),
            idempotency_key="delivery:abc",
            source_event_id="local-event-1",
            reply_to_remote_event_id="remote-event-1",
            group_bridge_thread="thread-1",
            credential="secret",
        )

        body = build_remote_chat_payload(envelope)

        self.assertEqual(body["text"], "hello")
        self.assertEqual(body["to"], ["@foreman"])
        self.assertEqual(body["by"], "group_bridge:peer_source")
        self.assertEqual(body["priority"], "attention")
        self.assertEqual(body["reply_required"], True)
        self.assertEqual(body["idempotency_key"], "delivery:abc")
        self.assertEqual(body["source_platform"], "group_bridge_session")
        self.assertEqual(body["source_user_id"], "peer_source")
        self.assertEqual(body["source_by"], "user")
        self.assertEqual(body["src_group_id"], "g_local")
        self.assertEqual(body["src_event_id"], "local-event-1")
        self.assertEqual(body["reply_to"], "remote-event-1")
        self.assertEqual(body["group_bridge_thread"], "thread-1")


if __name__ == "__main__":
    unittest.main()
