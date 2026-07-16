import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from cccc.ports.im.adapters.feishu import FeishuAdapter


class _FakeHttpResponse:
    def __init__(self, body: dict):
        self._body = json.dumps(body, ensure_ascii=False).encode("utf-8")

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self) -> bytes:
        return self._body


class _FakeHttpxResponse:
    def __init__(self, body: dict, status_code: int = 200):
        self._body = body
        self.status_code = status_code
        self.text = json.dumps(body, ensure_ascii=False)

    def json(self) -> dict:
        return self._body

    def raise_for_status(self) -> None:
        return None


class TestFeishuInboundRouting(unittest.TestCase):
    def test_enqueue_message_uses_message_mapper(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret")
        adapter._connected = True

        class FakeMapper:
            def __init__(self) -> None:
                self.events: list[dict] = []

            def map_event(self, event: dict) -> dict:
                self.events.append(event)
                return {
                    "chat_id": "oc_group",
                    "chat_title": "Group",
                    "chat_type": "group",
                    "routed": True,
                    "thread_id": 0,
                    "text": "mapped",
                    "attachments": [],
                    "from_user": "ou_sender",
                    "message_id": "om_mapped",
                    "timestamp": 0.0,
                }

        fake_mapper = FakeMapper()
        event = {"header": {"event_type": "im.message.receive_v1"}, "event": {}}
        adapter._message_mapper = lambda: fake_mapper  # type: ignore[method-assign]

        adapter._enqueue_message(event)

        self.assertEqual(fake_mapper.events, [event])
        self.assertEqual(adapter.poll()[0]["text"], "mapped")

    def test_group_message_mentioning_non_bot_user_is_not_routed(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret")
        adapter._connected = True
        adapter._bot_open_id = "ou_bot"
        adapter._bot_user_id = "u_bot"
        adapter._bot_name = "cccc"

        adapter._enqueue_message(
            {
                "header": {"event_type": "im.message.receive_v1"},
                "event": {
                    "message": {
                        "message_id": "om_1",
                        "chat_id": "oc_group",
                        "chat_type": "group",
                        "message_type": "text",
                        "content": json.dumps({"text": "@_user_1 好像没有要求"}),
                        "mentions": [
                            {
                                "key": "@_user_1",
                                "name": "waterbang",
                                "id": {"open_id": "ou_human", "user_id": "u_human"},
                            }
                        ],
                    },
                    "sender": {
                        "sender_type": "user",
                        "sender_id": {"open_id": "ou_sender"},
                    },
                },
            }
        )

        messages = adapter.poll()
        self.assertEqual(len(messages), 1)
        self.assertFalse(messages[0]["routed"])

    def test_group_message_mentioning_named_human_is_not_routed(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret")
        adapter._connected = True
        adapter._bot_open_id = "ou_bot"
        adapter._bot_user_id = "u_bot"
        adapter._bot_name = "cccc"

        adapter._enqueue_message(
            {
                "header": {"event_type": "im.message.receive_v1"},
                "event": {
                    "message": {
                        "message_id": "om_named_human",
                        "chat_id": "oc_group",
                        "chat_type": "group",
                        "message_type": "text",
                        "content": json.dumps({"text": "@_user_1 你的论文写完了"}),
                        "mentions": [
                            {
                                "key": "@_user_1",
                                "name": "黄永金",
                                "id": {"open_id": "ou_human", "user_id": "u_human"},
                            }
                        ],
                    },
                    "sender": {
                        "sender_type": "user",
                        "sender_id": {"open_id": "ou_sender"},
                    },
                },
            }
        )

        messages = adapter.poll()
        self.assertEqual(len(messages), 1)
        self.assertEqual(messages[0]["text"], "@黄永金 你的论文写完了")
        self.assertFalse(messages[0]["routed"])

    def test_group_message_mentioning_same_named_non_bot_user_is_not_routed(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret")
        adapter._connected = True
        adapter._bot_open_id = "ou_bot"
        adapter._bot_user_id = "u_bot"
        adapter._bot_name = "cccc"

        adapter._enqueue_message(
            {
                "header": {"event_type": "im.message.receive_v1"},
                "event": {
                    "message": {
                        "message_id": "om_same_name_human",
                        "chat_id": "oc_group",
                        "chat_type": "group",
                        "message_type": "text",
                        "content": json.dumps({"text": "@_user_1 status"}),
                        "mentions": [
                            {
                                "key": "@_user_1",
                                "name": "cccc",
                                "id": {"open_id": "ou_human", "user_id": "u_human"},
                            }
                        ],
                    },
                    "sender": {
                        "sender_type": "user",
                        "sender_id": {"open_id": "ou_sender"},
                    },
                },
            }
        )

        messages = adapter.poll()
        self.assertEqual(len(messages), 1)
        self.assertFalse(messages[0]["routed"])

    def test_group_message_with_any_mention_does_not_route_when_bot_identity_unknown(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret", bot_name="")
        adapter._connected = True

        adapter._enqueue_message(
            {
                "header": {"event_type": "im.message.receive_v1"},
                "event": {
                    "message": {
                        "message_id": "om_unknown_identity",
                        "chat_id": "oc_group",
                        "chat_type": "group",
                        "message_type": "text",
                        "content": json.dumps({"text": "@_user_1 status"}),
                        "mentions": [
                            {
                                "key": "@_user_1",
                                "name": "cccc",
                                "id": {"open_id": "ou_unknown", "user_id": "u_unknown"},
                            }
                        ],
                    },
                    "sender": {
                        "sender_type": "user",
                        "sender_id": {"open_id": "ou_sender"},
                    },
                },
            }
        )

        messages = adapter.poll()
        self.assertEqual(len(messages), 1)
        self.assertFalse(messages[0]["routed"])

    def test_group_message_mentioning_configured_bot_name_with_id_is_routed_when_bot_id_unknown(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret", bot_name="cccc")
        adapter._connected = True

        adapter._enqueue_message(
            {
                "header": {"event_type": "im.message.receive_v1"},
                "event": {
                    "message": {
                        "message_id": "om_name_fallback_with_id",
                        "chat_id": "oc_group",
                        "chat_type": "group",
                        "message_type": "text",
                        "content": json.dumps({"text": "@_user_1 status"}),
                        "mentions": [
                            {
                                "key": "@_user_1",
                                "name": "cccc",
                                "id": {"open_id": "ou_bot_from_event", "user_id": "u_bot_from_event"},
                            }
                        ],
                    },
                    "sender": {
                        "sender_type": "user",
                        "sender_id": {"open_id": "ou_sender"},
                    },
                },
            }
        )

        messages = adapter.poll()
        self.assertEqual(len(messages), 1)
        self.assertTrue(messages[0]["routed"])

    def test_connect_fails_when_bot_identity_cannot_be_loaded(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret", bot_name="")
        adapter._disable_proxies = lambda: None  # type: ignore[method-assign]
        adapter._refresh_token = lambda: True  # type: ignore[method-assign]

        with (
            patch.dict("sys.modules", {"lark_oapi": object(), "lark_oapi.ws": type("WsModule", (), {"Client": object})()}),
            patch.object(adapter, "_api", return_value={"code": 403, "msg": "forbidden"}),
        ):
            self.assertFalse(adapter.connect())

    def test_connect_allows_configured_bot_name_when_identity_response_is_empty(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret", bot_name="cccc")
        adapter._disable_proxies = lambda: None  # type: ignore[method-assign]
        adapter._refresh_token = lambda: True  # type: ignore[method-assign]
        adapter._start_ws_listener = lambda: adapter._ws_started.set()  # type: ignore[method-assign]

        with (
            patch.dict("sys.modules", {"lark_oapi": object(), "lark_oapi.ws": type("WsModule", (), {"Client": object})()}),
            patch.object(adapter, "_api", return_value={"code": 0, "data": {"bot": {}}}),
        ):
            self.assertTrue(adapter.connect())

    def test_group_message_mentioning_bot_open_id_is_routed(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret")
        adapter._connected = True
        adapter._bot_open_id = "ou_bot"

        adapter._enqueue_message(
            {
                "header": {"event_type": "im.message.receive_v1"},
                "event": {
                    "message": {
                        "message_id": "om_2",
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
                    },
                    "sender": {
                        "sender_type": "user",
                        "sender_id": {"open_id": "ou_sender"},
                    },
                },
            }
        )

        messages = adapter.poll()
        self.assertEqual(len(messages), 1)
        self.assertTrue(messages[0]["routed"])


class TestFeishuOutboundFiles(unittest.TestCase):
    def test_send_file_uploads_and_sends_file_message(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret")
        adapter._connected = True
        adapter._client.token = "tenant-token"
        adapter._client.token_expires = 9_999_999_999
        adapter._rate_limiter.wait_and_acquire = lambda _chat_id: None  # type: ignore[method-assign]

        upload_calls = []
        sent_requests = []

        def fake_post(url, *, headers=None, data=None, files=None, timeout=0):
            upload_calls.append(
                {
                    "url": url,
                    "headers": headers,
                    "data": data,
                    "files": files,
                    "timeout": timeout,
                }
            )
            self.assertEqual(data, {"file_type": "stream", "file_name": "report.md"})
            file_tuple = files["file"]
            self.assertEqual(file_tuple[0], "report.md")
            self.assertEqual(file_tuple[1], b"hello file")
            self.assertEqual(file_tuple[2], "text/markdown")
            return _FakeHttpxResponse({"code": 0, "data": {"file_key": "file_v2_key"}})

        def fake_urlopen(req, timeout=0):
            sent_requests.append((req, timeout))
            return _FakeHttpResponse({"code": 0, "data": {"message_id": "om_file"}})

        with tempfile.TemporaryDirectory() as td:
            file_path = Path(td) / "report.md"
            file_path.write_text("hello file", encoding="utf-8")
            with (
                patch("httpx.post", side_effect=fake_post),
                patch("urllib.request.urlopen", side_effect=fake_urlopen),
            ):
                ok = adapter.send_file("oc_group", file_path=file_path, filename="report.md", caption="")

        self.assertTrue(ok)
        self.assertEqual(len(upload_calls), 1)
        self.assertEqual(len(sent_requests), 1)
        message_req = sent_requests[0][0]
        self.assertIn("/open-apis/im/v1/messages?receive_id_type=chat_id", str(message_req.full_url))
        payload = json.loads(message_req.data.decode("utf-8"))
        self.assertEqual(payload["receive_id"], "oc_group")
        self.assertEqual(payload["msg_type"], "file")
        self.assertEqual(json.loads(payload["content"]), {"file_key": "file_v2_key"})

    def test_send_file_uploads_png_as_image_message(self) -> None:
        adapter = FeishuAdapter(app_id="cli_test", app_secret="secret")
        adapter._connected = True
        adapter._client.token = "tenant-token"
        adapter._client.token_expires = 9_999_999_999
        adapter._rate_limiter.wait_and_acquire = lambda _chat_id: None  # type: ignore[method-assign]

        upload_calls = []
        sent_requests = []

        def fake_post(url, *, headers=None, data=None, files=None, timeout=0):
            upload_calls.append(
                {
                    "url": url,
                    "headers": headers,
                    "data": data,
                    "files": files,
                    "timeout": timeout,
                }
            )
            self.assertIn("/open-apis/im/v1/images", url)
            self.assertEqual(data, {"image_type": "message"})
            image_tuple = files["image"]
            self.assertEqual(image_tuple[0], "figure-1-architecture.png")
            self.assertEqual(image_tuple[1], b"png bytes")
            self.assertEqual(image_tuple[2], "image/png")
            return _FakeHttpxResponse({"code": 0, "data": {"image_key": "img_v2_key"}})

        def fake_urlopen(req, timeout=0):
            sent_requests.append((req, timeout))
            return _FakeHttpResponse({"code": 0, "data": {"message_id": "om_image"}})

        with tempfile.TemporaryDirectory() as td:
            file_path = Path(td) / "figure-1-architecture.png"
            file_path.write_bytes(b"png bytes")
            with (
                patch("httpx.post", side_effect=fake_post),
                patch("urllib.request.urlopen", side_effect=fake_urlopen),
            ):
                ok = adapter.send_file("oc_group", file_path=file_path, filename="figure-1-architecture.png", caption="")

        self.assertTrue(ok)
        self.assertEqual(len(upload_calls), 1)
        self.assertEqual(len(sent_requests), 1)
        payload = json.loads(sent_requests[0][0].data.decode("utf-8"))
        self.assertEqual(payload["receive_id"], "oc_group")
        self.assertEqual(payload["msg_type"], "image")
        self.assertEqual(json.loads(payload["content"]), {"image_key": "img_v2_key"})


if __name__ == "__main__":
    unittest.main()
