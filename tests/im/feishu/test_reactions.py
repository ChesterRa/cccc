import unittest

from cccc.ports.im.adapters.feishu.reactions import (
    build_add_reaction_request,
    build_remove_reaction_request,
    parse_add_reaction_response,
    reaction_succeeded,
)


class TestFeishuReactions(unittest.TestCase):
    def test_builds_add_reaction_request_with_default_emoji(self) -> None:
        endpoint, body, emoji = build_add_reaction_request("om_123", "")

        self.assertEqual(endpoint, "/im/v1/messages/om_123/reactions")
        self.assertEqual(body, {"reaction_type": {"emoji_type": "OnIt"}})
        self.assertEqual(emoji, "OnIt")

    def test_parses_add_reaction_success(self) -> None:
        reaction_id = parse_add_reaction_response({"code": 0, "data": {"reaction_id": "react_1"}})

        self.assertEqual(reaction_id, "react_1")
        self.assertIsNone(parse_add_reaction_response({"code": 0, "data": {}}))
        self.assertIsNone(parse_add_reaction_response({"code": 403, "msg": "forbidden"}))

    def test_builds_remove_reaction_request_and_success_check(self) -> None:
        endpoint = build_remove_reaction_request("om_123", "react_1")

        self.assertEqual(endpoint, "/im/v1/messages/om_123/reactions/react_1")
        self.assertTrue(reaction_succeeded({"code": 0}))
        self.assertFalse(reaction_succeeded({"code": 400}))
