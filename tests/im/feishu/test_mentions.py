import unittest

from cccc.ports.im.adapters.feishu.mentions import FeishuBotIdentity, FeishuMentionRouter


class TestFeishuMentionRouter(unittest.TestCase):
    def test_routes_when_bot_open_id_matches(self) -> None:
        router = FeishuMentionRouter(FeishuBotIdentity(open_id="ou_bot", user_id="", name="cccc"))

        self.assertTrue(
            router.mentions_bot(
                [
                    {
                        "name": "someone",
                        "id": {"open_id": "ou_bot", "user_id": "u_other"},
                    }
                ]
            )
        )

    def test_routes_by_configured_name_when_bot_id_unknown_even_if_mention_has_id(self) -> None:
        router = FeishuMentionRouter(FeishuBotIdentity(open_id="", user_id="", name="cccc"))

        self.assertTrue(
            router.mentions_bot(
                [
                    {
                        "name": "cccc",
                        "id": {"open_id": "ou_from_event", "user_id": "u_from_event"},
                    }
                ]
            )
        )

    def test_does_not_route_same_name_human_when_bot_id_is_known(self) -> None:
        router = FeishuMentionRouter(FeishuBotIdentity(open_id="ou_bot", user_id="u_bot", name="cccc"))

        self.assertFalse(
            router.mentions_bot(
                [
                    {
                        "name": "cccc",
                        "id": {"open_id": "ou_human", "user_id": "u_human"},
                    }
                ]
            )
        )

    def test_does_not_route_any_mention_without_identity_or_configured_name(self) -> None:
        router = FeishuMentionRouter(FeishuBotIdentity(open_id="", user_id="", name=""))

        self.assertFalse(
            router.mentions_bot(
                [
                    {
                        "name": "cccc",
                        "id": {"open_id": "ou_unknown", "user_id": "u_unknown"},
                    }
                ]
            )
        )


if __name__ == "__main__":
    unittest.main()
