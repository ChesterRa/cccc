import unittest


class TestMcpFederationTools(unittest.TestCase):
    def test_remote_send_delegates_to_daemon(self) -> None:
        from cccc.ports.mcp.handlers import cccc_federation

        captured = {}

        def fake_call(req, **kwargs):
            captured["req"] = req
            return {"queued": False, "receipt": {"status": "sent", "remote_event_id": "remote-1"}}

        original = cccc_federation._call_daemon_or_raise
        cccc_federation._call_daemon_or_raise = fake_call  # type: ignore[assignment]
        try:
            out = cccc_federation.remote_send(
                group_id="g_local",
                actor_id="actor-a",
                registration_id="reg_1",
                text="hi",
                to=["@all"],
                idempotency_key="k1",
            )
        finally:
            cccc_federation._call_daemon_or_raise = original  # type: ignore[assignment]

        self.assertEqual(out["queued"], False)
        self.assertEqual(out["receipt"]["status"], "sent")
        req = captured["req"]
        self.assertEqual(req["op"], "remote_send")
        args = req["args"]
        self.assertEqual(args["registration_id"], "reg_1")
        self.assertEqual(args["idempotency_key"], "k1")
        self.assertEqual(args["payload"]["text"], "hi")
        self.assertEqual(args["payload"]["to"], ["@all"])

    def test_remote_send_requires_idempotency_key(self) -> None:
        from cccc.ports.mcp.common import MCPError
        from cccc.ports.mcp.handlers import cccc_federation

        with self.assertRaises(MCPError):
            cccc_federation.remote_send(
                group_id="g_local",
                actor_id="actor-a",
                registration_id="reg_1",
                text="hi",
                idempotency_key="",
            )

    def test_remote_send_requires_explicit_recipient(self) -> None:
        from cccc.ports.mcp.common import MCPError
        from cccc.ports.mcp.handlers import cccc_federation

        with self.assertRaises(MCPError) as raised:
            cccc_federation.remote_send(
                group_id="g_local",
                actor_id="actor-a",
                registration_id="reg_1",
                text="hi",
                to=[],
                idempotency_key="k1",
            )

        self.assertEqual(raised.exception.code, "missing_remote_recipient")

    def test_delivery_status_delegates_to_daemon(self) -> None:
        from cccc.ports.mcp.handlers import cccc_federation

        captured = {}

        def fake_call(req, **kwargs):
            captured["req"] = req
            return {"receipt": {"status": "queued"}}

        original = cccc_federation._call_daemon_or_raise
        cccc_federation._call_daemon_or_raise = fake_call  # type: ignore[assignment]
        try:
            cccc_federation.remote_delivery_status(
                group_id="g_local", registration_id="reg_1", idempotency_key="k1"
            )
        finally:
            cccc_federation._call_daemon_or_raise = original  # type: ignore[assignment]

        self.assertEqual(captured["req"]["op"], "remote_delivery_status")
        self.assertEqual(captured["req"]["args"]["idempotency_key"], "k1")

    def test_handler_does_not_import_http_transport(self) -> None:
        # MCP handler must reach the daemon, never speak HTTP itself.
        import inspect

        from cccc.ports.mcp.handlers import cccc_federation

        src = inspect.getsource(cccc_federation)
        self.assertNotIn("transports", src)
        self.assertNotIn("urllib", src)
        self.assertNotIn("requests", src)


if __name__ == "__main__":
    unittest.main()
