import unittest

from cccc.ports.im.adapters.feishu.webhook import normalize_webhook_event


class TestFeishuWebhookNormalizer(unittest.TestCase):
    def test_returns_challenge_response(self) -> None:
        result = normalize_webhook_event({"challenge": "abc123"})

        self.assertEqual(result.challenge_response, {"challenge": "abc123"})
        self.assertIsNone(result.message_event)

    def test_keeps_v2_event_shape_for_enqueue(self) -> None:
        event = {
            "schema": "2.0",
            "header": {"event_type": "im.message.receive_v1"},
            "event": {"message": {"message_id": "om_1"}},
        }

        result = normalize_webhook_event(event)

        self.assertIsNone(result.challenge_response)
        self.assertEqual(result.message_event, event)

    def test_wraps_legacy_v1_event_for_enqueue(self) -> None:
        result = normalize_webhook_event(
            {
                "event": {
                    "type": "im.message.receive_v1",
                    "message": {"message_id": "om_legacy"},
                }
            }
        )

        self.assertIsNone(result.challenge_response)
        self.assertEqual(
            result.message_event,
            {
                "header": {"event_type": "im.message.receive_v1"},
                "event": {
                    "type": "im.message.receive_v1",
                    "message": {"message_id": "om_legacy"},
                },
            },
        )
