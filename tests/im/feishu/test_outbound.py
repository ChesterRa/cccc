import json
import unittest

from cccc.ports.im.adapters.feishu.outbound import build_media_message_request, build_text_message_request


class TestFeishuOutboundRequests(unittest.TestCase):
    def test_builds_text_message_request(self) -> None:
        endpoint, body = build_text_message_request("oc_group", "hello", thread_id=123)

        self.assertEqual(endpoint, "/im/v1/messages?receive_id_type=chat_id")
        self.assertEqual(
            body,
            {
                "receive_id": "oc_group",
                "msg_type": "text",
                "content": json.dumps({"text": "hello"}, ensure_ascii=False),
                "root_id": "123",
            },
        )

    def test_builds_media_message_request(self) -> None:
        endpoint, body = build_media_message_request(
            "oc_group",
            msg_type="image",
            media_key_name="image_key",
            media_key="img_123",
            thread_id=None,
        )

        self.assertEqual(endpoint, "/im/v1/messages?receive_id_type=chat_id")
        self.assertEqual(
            body,
            {
                "receive_id": "oc_group",
                "msg_type": "image",
                "content": json.dumps({"image_key": "img_123"}, ensure_ascii=False),
            },
        )
