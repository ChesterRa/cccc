import unittest
from unittest.mock import patch

from cccc.ports.im.adapters.feishu.ws import FeishuWsListener


class _FakeBuilder:
    def __init__(self) -> None:
        self.handler = None

    def register_p2_im_message_receive_v1(self, handler):
        self.handler = handler
        return self

    def build(self):
        return {"handler": self.handler}


class _FakeEventDispatcherHandler:
    builder_instance = _FakeBuilder()

    @classmethod
    def builder(cls, *_args):
        cls.builder_instance = _FakeBuilder()
        return cls.builder_instance


class _FakeLogLevel:
    INFO = "INFO"


class _FakeLark:
    EventDispatcherHandler = _FakeEventDispatcherHandler
    LogLevel = _FakeLogLevel


class _FakeWsClient:
    instances = []

    def __init__(self, **kwargs):
        self.kwargs = kwargs
        self.started = False
        self.stopped = False
        _FakeWsClient.instances.append(self)

    def start(self):
        self.started = True

    def stop(self):
        self.stopped = True


class _RaisingWsClient(_FakeWsClient):
    def start(self):
        raise RuntimeError("ws failed")


class TestFeishuWsListener(unittest.TestCase):
    def setUp(self) -> None:
        _FakeWsClient.instances.clear()

    def test_starts_client_and_forwards_normalized_message_event(self) -> None:
        logs = []
        enqueued = []
        listener = FeishuWsListener(
            app_id="cli_test",
            app_secret="secret",
            domain="https://open.feishu.cn",
            lark=_FakeLark,
            ws_client_cls=_FakeWsClient,
            log_fn=logs.append,
            enqueue_fn=enqueued.append,
        )

        with patch("cccc.ports.im.adapters.feishu.ws.threading.Thread") as thread_cls:
            thread_cls.side_effect = lambda target, daemon: type(
                "InlineThread",
                (),
                {
                    "start": lambda self: target(),
                    "is_alive": lambda self: True,
                    "join": lambda self, timeout=0: None,
                },
            )()

            listener.start()

        self.assertTrue(listener.started.wait(timeout=0))
        self.assertIsNone(listener.connect_error)
        self.assertEqual(len(_FakeWsClient.instances), 1)
        self.assertTrue(_FakeWsClient.instances[0].started)

        handler = _FakeWsClient.instances[0].kwargs["event_handler"]["handler"]
        event_obj = type(
            "Event",
            (),
            {
                "message": type(
                    "Message",
                    (),
                    {
                        "message_id": "om_1",
                        "chat_id": "oc_group",
                        "chat_type": "group",
                        "message_type": "text",
                        "content": "{\"text\":\"hello\"}",
                    },
                )(),
                "sender": type(
                    "Sender",
                    (),
                    {
                        "sender_type": "user",
                        "sender_id": type("SenderId", (), {"open_id": "ou_sender"})(),
                    },
                )(),
            },
        )()
        handler(type("Data", (), {"event": event_obj})())

        self.assertEqual(enqueued[0]["header"], {"event_type": "im.message.receive_v1"})
        self.assertEqual(enqueued[0]["event"]["message"]["chat_id"], "oc_group")
        self.assertIn("[ws] Message enqueued", logs)

        listener.stop()
        self.assertTrue(_FakeWsClient.instances[0].stopped)

    def test_records_start_error_and_signals_started(self) -> None:
        logs = []
        listener = FeishuWsListener(
            app_id="cli_test",
            app_secret="secret",
            domain="https://open.feishu.cn",
            lark=_FakeLark,
            ws_client_cls=_RaisingWsClient,
            log_fn=logs.append,
            enqueue_fn=lambda _event: None,
        )

        with patch("cccc.ports.im.adapters.feishu.ws.threading.Thread") as thread_cls:
            thread_cls.side_effect = lambda target, daemon: type(
                "InlineThread",
                (),
                {
                    "start": lambda self: target(),
                    "is_alive": lambda self: True,
                    "join": lambda self, timeout=0: None,
                },
            )()

            listener.start()

        self.assertTrue(listener.started.wait(timeout=0))
        self.assertEqual(listener.connect_error, "ws failed")
        self.assertTrue(any("[ws] SDK error: ws failed" in item for item in logs))


if __name__ == "__main__":
    unittest.main()
