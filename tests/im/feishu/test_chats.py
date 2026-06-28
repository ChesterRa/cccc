import unittest

from cccc.ports.im.adapters.feishu.chats import FeishuChatTitleResolver


class TestFeishuChatTitleResolver(unittest.TestCase):
    def test_get_fetches_chat_title_and_caches_result(self) -> None:
        calls = []

        def api(method: str, endpoint: str):
            calls.append((method, endpoint))
            return {"code": 0, "data": {"name": "工程群", "chat_id": "oc_group"}}

        resolver = FeishuChatTitleResolver(api)

        self.assertEqual(resolver.get("oc_group"), "工程群")
        self.assertEqual(resolver.get("oc_group"), "工程群")
        self.assertEqual(calls, [("GET", "/im/v1/chats/oc_group")])

    def test_get_falls_back_to_chat_id_when_name_is_missing(self) -> None:
        resolver = FeishuChatTitleResolver(lambda _method, _endpoint: {"code": 0, "data": {"chat_id": "oc_group"}})

        self.assertEqual(resolver.get("oc_group"), "oc_group")

    def test_get_falls_back_to_chat_id_on_api_error(self) -> None:
        resolver = FeishuChatTitleResolver(lambda _method, _endpoint: {"code": 403, "msg": "forbidden"})

        self.assertEqual(resolver.get("oc_group"), "oc_group")


if __name__ == "__main__":
    unittest.main()
