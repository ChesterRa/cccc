import json
import unittest

from cccc.ports.im.adapters.feishu.mapper import FeishuMessageMapper
from cccc.ports.im.adapters.feishu.mentions import FeishuBotIdentity


class TestFeishuMessageMapper(unittest.TestCase):
    def test_maps_text_message_and_replaces_mention_names(self) -> None:
        mapper = FeishuMessageMapper(
            bot_identity=FeishuBotIdentity(open_id="ou_bot", name="cccc"),
            chat_title_lookup=lambda chat_id: f"title:{chat_id}",
        )

        message = mapper.map_event(
            {
                "header": {"event_type": "im.message.receive_v1"},
                "event": {
                    "message": {
                        "message_id": "om_1",
                        "chat_id": "oc_group",
                        "chat_type": "group",
                        "message_type": "text",
                        "content": json.dumps({"text": "@_user_1 status"}),
                        "mentions": [
                            {
                                "key": "@_user_1",
                                "name": "cccc",
                                "id": {"open_id": "ou_bot", "user_id": ""},
                            }
                        ],
                        "create_time": "1710000000123",
                    },
                    "sender": {
                        "sender_type": "user",
                        "sender_id": {"open_id": "ou_sender"},
                    },
                },
            }
        )

        self.assertIsNotNone(message)
        assert message is not None
        self.assertEqual(message["chat_title"], "title:oc_group")
        self.assertEqual(message["text"], "@cccc status")
        self.assertTrue(message["routed"])
        self.assertEqual(message["timestamp"], 1710000000.123)

    def test_maps_image_message_attachment(self) -> None:
        mapper = FeishuMessageMapper(
            bot_identity=FeishuBotIdentity(),
            chat_title_lookup=lambda _chat_id: "",
        )

        message = mapper.map_event(
            {
                "header": {"event_type": "im.message.receive_v1"},
                "event": {
                    "message": {
                        "message_id": "om_img",
                        "chat_id": "oc_p2p",
                        "chat_type": "p2p",
                        "message_type": "image",
                        "content": json.dumps({"image_key": "img_123"}),
                    },
                    "sender": {
                        "sender_type": "user",
                        "sender_id": {"open_id": "ou_sender"},
                    },
                },
            }
        )

        self.assertIsNotNone(message)
        assert message is not None
        self.assertEqual(message["text"], "[image]")
        self.assertEqual(
            message["attachments"],
            [
                {
                    "provider": "feishu",
                    "kind": "image",
                    "image_key": "img_123",
                    "file_name": "image.png",
                }
            ],
        )
        self.assertTrue(message["routed"])

    def test_skips_bot_sender_and_unrelated_events(self) -> None:
        mapper = FeishuMessageMapper(
            bot_identity=FeishuBotIdentity(open_id="ou_bot"),
            chat_title_lookup=lambda _chat_id: "",
        )

        self.assertIsNone(mapper.map_event({"header": {"event_type": "other.event"}, "event": {}}))
        self.assertIsNone(
            mapper.map_event(
                {
                    "header": {"event_type": "im.message.receive_v1"},
                    "event": {
                        "message": {
                            "message_id": "om_self",
                            "chat_id": "oc_group",
                            "chat_type": "group",
                            "message_type": "text",
                            "content": json.dumps({"text": "self"}),
                        },
                        "sender": {"sender_type": "app", "sender_id": {"open_id": "ou_bot"}},
                    },
                }
            )
        )
