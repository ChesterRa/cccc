import unittest
from unittest.mock import patch


class TestMcpMessageReplyIdempotency(unittest.TestCase):
    def test_message_reply_does_not_generate_content_based_client_id(self) -> None:
        from cccc.ports.mcp.handlers import cccc_messaging

        captured: list[dict] = []

        def fake_call(req: dict) -> dict:
            captured.append(req)
            return {"ok": True, "result": {}}

        with patch.object(cccc_messaging, "_call_daemon_or_raise", side_effect=fake_call), patch.object(
            cccc_messaging, "_normalize_runtime_escaped_text", side_effect=lambda **kwargs: kwargs["text"]
        ):
            cccc_messaging.message_reply(
                group_id="g1",
                actor_id="peer1",
                reply_to="event1",
                text="same reply",
                to=["user"],
            )
            cccc_messaging.message_reply(
                group_id="g1",
                actor_id="peer1",
                reply_to="event1",
                text="same reply",
                to=["user"],
            )

        self.assertNotIn("client_id", captured[0]["args"])
        self.assertNotIn("client_id", captured[1]["args"])


if __name__ == "__main__":
    unittest.main()
