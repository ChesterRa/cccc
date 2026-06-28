import unittest

from cccc.ports.im.adapters.feishu.events import normalize_message_event


class _Obj:
    def __init__(self, **kwargs):
        self.__dict__.update(kwargs)


class TestFeishuEventNormalizer(unittest.TestCase):
    def test_normalizes_sdk_message_event_with_mentions_and_sender(self) -> None:
        data = _Obj(
            event=_Obj(
                message=_Obj(
                    message_id="om_1",
                    chat_id="oc_group",
                    chat_type="group",
                    message_type="text",
                    content='{"text":"@_user_1 hi"}',
                    root_id="om_root",
                    create_time="1710000000000",
                    mentions=[
                        _Obj(
                            key="@_user_1",
                            name="cccc",
                            id=_Obj(open_id="ou_bot", user_id="u_bot"),
                        )
                    ],
                ),
                sender=_Obj(
                    sender_type="user",
                    sender_id=_Obj(open_id="ou_sender", user_id="u_sender"),
                ),
            )
        )

        normalized = normalize_message_event(data)

        self.assertEqual(
            normalized,
            {
                "message": {
                    "message_id": "om_1",
                    "chat_id": "oc_group",
                    "chat_type": "group",
                    "message_type": "text",
                    "content": '{"text":"@_user_1 hi"}',
                    "root_id": "om_root",
                    "create_time": "1710000000000",
                    "mentions": [
                        {
                            "key": "@_user_1",
                            "name": "cccc",
                            "id": {"open_id": "ou_bot", "user_id": "u_bot"},
                        }
                    ],
                },
                "sender": {
                    "sender_id": {"open_id": "ou_sender", "user_id": "u_sender"},
                    "sender_type": "user",
                },
            },
        )

    def test_normalizes_missing_event_to_empty_message_shape(self) -> None:
        normalized = normalize_message_event(_Obj())

        self.assertEqual(normalized, {"message": {}, "sender": {"sender_id": {}}})


if __name__ == "__main__":
    unittest.main()
