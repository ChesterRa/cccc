import unittest

from cccc.ports.im.adapters.feishu.identity import parse_bot_identity_response


class TestFeishuBotIdentityParsing(unittest.TestCase):
    def test_parses_nested_bot_identity_and_prefers_api_name(self) -> None:
        result = parse_bot_identity_response(
            {
                "code": 0,
                "data": {
                    "bot": {
                        "open_id": " ou_bot ",
                        "bot_user_id": " u_bot ",
                        "app_name": " CCCC Bot ",
                    }
                },
            },
            configured_name="cccc",
        )

        self.assertTrue(result.ok)
        self.assertEqual(result.identity.open_id, "ou_bot")
        self.assertEqual(result.identity.user_id, "u_bot")
        self.assertEqual(result.identity.name, "CCCC Bot")

    def test_parses_flat_bot_identity_response(self) -> None:
        result = parse_bot_identity_response(
            {
                "code": 0,
                "data": {
                    "bot_open_id": "ou_flat",
                    "user_id": "u_flat",
                    "bot_name": "flatbot",
                },
            },
            configured_name="cccc",
        )

        self.assertTrue(result.ok)
        self.assertEqual(result.identity.open_id, "ou_flat")
        self.assertEqual(result.identity.user_id, "u_flat")
        self.assertEqual(result.identity.name, "flatbot")

    def test_allows_configured_name_when_api_returns_empty_bot_data(self) -> None:
        result = parse_bot_identity_response({"code": 0, "data": {"bot": {}}}, configured_name="cccc")

        self.assertTrue(result.ok)
        self.assertEqual(result.identity.open_id, "")
        self.assertEqual(result.identity.user_id, "")
        self.assertEqual(result.identity.name, "cccc")

    def test_rejects_error_response(self) -> None:
        result = parse_bot_identity_response({"code": 403, "msg": "forbidden"}, configured_name="cccc")

        self.assertFalse(result.ok)
        self.assertIn("forbidden", result.error)

    def test_rejects_empty_response_without_configured_name(self) -> None:
        result = parse_bot_identity_response({"code": 0, "data": {"bot": {}}}, configured_name="")

        self.assertFalse(result.ok)
        self.assertIn("usable id or name", result.error)


if __name__ == "__main__":
    unittest.main()
