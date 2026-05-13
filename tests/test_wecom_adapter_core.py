"""Tests for WecomAdapter core functionality (Steps 7-13)."""

import json
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch

from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes


def _encrypt_wecom_media_for_test(raw: bytes, aes_key: str) -> bytes:
    key = aes_key.encode("utf-8")
    pad = 32 - (len(raw) % 32)
    padded = raw + bytes([pad]) * pad
    encryptor = Cipher(algorithms.AES(key), modes.CBC(key[:16])).encryptor()
    return encryptor.update(padded) + encryptor.finalize()


class TestWecomAuthFrames(unittest.TestCase):
    """WeCom AI Bot auth now uses bot_id + secret over WebSocket."""

    def _make_adapter(self, **kw):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        return WecomAdapter(
            bot_id=kw.get("bot_id", "corp123"),
            secret=kw.get("secret", "sec456"),
        )

    def test_build_subscribe_frame_uses_bot_id_and_secret(self):
        adapter = self._make_adapter()
        frame = adapter._build_subscribe_frame()
        self.assertEqual(frame["cmd"], "aibot_subscribe")
        self.assertEqual(frame["body"]["bot_id"], "corp123")
        self.assertEqual(frame["body"]["secret"], "sec456")
        self.assertTrue(str(frame["headers"]["req_id"]).startswith("aibot_subscribe_"))


class TestWecomDedup(unittest.TestCase):
    """Step 9: _should_enqueue_message deduplication tests."""

    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        return WecomAdapter(bot_id="corp", secret="sec")

    def test_first_message_passes(self):
        adapter = self._make_adapter()
        self.assertTrue(adapter._should_enqueue_message("conv1", "msg1"))

    def test_duplicate_blocked(self):
        adapter = self._make_adapter()
        adapter._should_enqueue_message("conv1", "msg1")
        self.assertFalse(adapter._should_enqueue_message("conv1", "msg1"))

    def test_empty_msg_id_always_passes(self):
        adapter = self._make_adapter()
        self.assertTrue(adapter._should_enqueue_message("conv1", ""))
        self.assertTrue(adapter._should_enqueue_message("conv1", ""))

    def test_pruning_at_threshold(self):
        adapter = self._make_adapter()
        # Fill with 2049 entries to trigger pruning
        for i in range(2049):
            adapter._seen_msg_ids[f"conv:msg{i}"] = time.time()
        # Next call should trigger pruning (no crash)
        adapter._should_enqueue_message("conv1", "new_msg")
        self.assertIn("conv1:new_msg", adapter._seen_msg_ids)


class TestWecomEnqueueMessage(unittest.TestCase):
    """Step 9: _enqueue_message normalization tests."""

    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        return WecomAdapter(bot_id="corp", secret="sec")

    def test_text_message_normalized(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "conv_123",
                "msg_id": "msg_456",
                "chat_type": "single",
                "msg_type": "text",
                "content": {"text": "hello world"},
                "sender": {"user_id": "user_789", "name": "Test User"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        m = msgs[0]
        self.assertEqual(m["chat_id"], "conv_123")
        self.assertEqual(m["chat_type"], "p2p")
        self.assertEqual(m["text"], "hello world")
        self.assertEqual(m["from_user"], "user_789")
        self.assertEqual(m["message_id"], "msg_456")
        self.assertTrue(m["routed"])  # p2p is always routed

    def test_group_message_not_routed_without_mention(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "grp_1",
                "msg_id": "msg_1",
                "chat_type": "group",
                "msg_type": "text",
                "content": {"text": "hi"},
                "sender": {"user_id": "u1"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        self.assertEqual(msgs[0]["chat_type"], "group")
        self.assertFalse(msgs[0]["routed"])

    def test_group_message_routed_when_at_bot(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "grp_1",
                "msg_id": "msg_2",
                "chat_type": "group",
                "msg_type": "text",
                "content": {"text": "@bot hi"},
                "sender": {"user_id": "u1"},
                "is_at_bot": True,
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertTrue(msgs[0]["routed"])

    def test_image_message_has_attachment(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "conv_1",
                "msg_id": "msg_img",
                "chat_type": "single",
                "msg_type": "image",
                "content": {"media_id": "media_abc"},
                "sender": {"user_id": "u1"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        self.assertEqual(msgs[0]["text"], "[image]")
        self.assertEqual(msgs[0]["attachments"][0]["kind"], "image")
        self.assertEqual(msgs[0]["attachments"][0]["media_id"], "media_abc")

    def test_file_message_has_attachment(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "conv_1",
                "msg_id": "msg_file",
                "chat_type": "single",
                "msg_type": "file",
                "content": {"media_id": "media_file", "file_name": "report.pdf"},
                "sender": {"user_id": "u1"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        self.assertEqual(msgs[0]["text"], "[file: report.pdf]")
        self.assertEqual(msgs[0]["attachments"][0]["kind"], "file")
        self.assertEqual(msgs[0]["attachments"][0]["media_id"], "media_file")
        self.assertEqual(msgs[0]["attachments"][0]["file_name"], "report.pdf")

    def test_image_sent_as_file_stays_file_without_filename_signal(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "conv_1",
                "msg_id": "msg_file_img",
                "chat_type": "single",
                "msg_type": "file",
                "content": {"media_id": "media_file_img"},
                "sender": {"user_id": "u1"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        self.assertEqual(msgs[0]["text"], "[file]")
        self.assertEqual(msgs[0]["attachments"][0]["kind"], "file")
        self.assertEqual(msgs[0]["attachments"][0]["media_id"], "media_file_img")
        self.assertEqual(msgs[0]["attachments"][0]["file_name"], "file")

    def test_video_message_has_attachment(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "conv_1",
                "msg_id": "msg_video",
                "chat_type": "single",
                "msg_type": "video",
                "content": {"media_id": "media_video", "file_name": "clip.mp4"},
                "sender": {"user_id": "u1"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        self.assertEqual(msgs[0]["text"], "[video]")
        self.assertEqual(msgs[0]["attachments"][0]["kind"], "video")
        self.assertEqual(msgs[0]["attachments"][0]["media_id"], "media_video")
        self.assertEqual(msgs[0]["attachments"][0]["file_name"], "clip.mp4")

    def test_voice_message_has_attachment(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "conv_1",
                "msg_id": "msg_voice",
                "chat_type": "single",
                "msg_type": "voice",
                "content": {"media_id": "media_voice", "file_name": "voice.amr"},
                "sender": {"user_id": "u1"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        self.assertEqual(msgs[0]["text"], "[voice]")
        self.assertEqual(msgs[0]["attachments"][0]["kind"], "voice")
        self.assertEqual(msgs[0]["attachments"][0]["media_id"], "media_voice")
        self.assertEqual(msgs[0]["attachments"][0]["file_name"], "voice.amr")

    def test_mixed_text_and_image_preserves_image_attachment(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "conv_mixed",
                "msg_id": "msg_mixed_img",
                "chat_type": "single",
                "msg_type": "mixed",
                "mixed": {
                    "msg_item": [
                        {"msgtype": "text", "text": {"content": "看下这张图"}},
                        {"msgtype": "image", "image": {"media_id": "media_mix_img", "filename": "demo.png"}},
                    ]
                },
                "sender": {"user_id": "u1"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        self.assertEqual(msgs[0]["text"], "看下这张图")
        self.assertEqual(len(msgs[0]["attachments"]), 1)
        self.assertEqual(msgs[0]["attachments"][0]["kind"], "image")
        self.assertEqual(msgs[0]["attachments"][0]["media_id"], "media_mix_img")
        self.assertEqual(msgs[0]["attachments"][0]["file_name"], "demo.png")

    def test_mixed_image_only_uses_image_placeholder(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "conv_mixed",
                "msg_id": "msg_mixed_only_img",
                "chat_type": "single",
                "msg_type": "mixed",
                "mixed": {
                    "msg_item": [
                        {
                            "msgtype": "image",
                            "image": {
                                "media_id": "media_only_img",
                                "url": "https://example.test/media.png",
                                "aeskey": "12345678901234567890123456789012",
                            },
                        }
                    ]
                },
                "sender": {"user_id": "u1"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        self.assertEqual(msgs[0]["text"], "[image]")
        self.assertEqual(len(msgs[0]["attachments"]), 1)
        att = msgs[0]["attachments"][0]
        self.assertEqual(att["kind"], "image")
        self.assertEqual(att["media_id"], "media_only_img")
        self.assertEqual(att["download_url"], "https://example.test/media.png")
        self.assertEqual(att["decryption_key"], "12345678901234567890123456789012")

    def test_ws_file_message_preserves_download_url_and_aeskey(self):
        adapter = self._make_adapter()
        data = {
            "cmd": "aibot_msg_callback",
            "headers": {"req_id": "req_media"},
            "body": {
                "chatid": "conv_1",
                "msgid": "msg_file_ws",
                "chattype": "single",
                "msgtype": "file",
                "file": {
                    "url": "https://example.test/file.bin",
                    "filename": "report.pdf",
                    "aeskey": "12345678901234567890123456789012",
                },
                "from": {"userid": "u1"},
            },
        }
        adapter._enqueue_message(data)
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)
        att = msgs[0]["attachments"][0]
        self.assertEqual(msgs[0]["text"], "[file: report.pdf]")
        self.assertEqual(att["download_url"], "https://example.test/file.bin")
        self.assertEqual(att["aeskey"], "12345678901234567890123456789012")
        self.assertEqual(att["decryption_key"], "12345678901234567890123456789012")
        self.assertEqual(att["file_name"], "report.pdf")

    def test_empty_data_ignored(self):
        adapter = self._make_adapter()
        adapter._enqueue_message({"action": "aibot_msg_callback"})
        self.assertEqual(len(adapter.poll()), 0)

    def test_duplicate_message_not_enqueued(self):
        adapter = self._make_adapter()
        data = {
            "action": "aibot_msg_callback",
            "data": {
                "conversation_id": "conv_1",
                "msg_id": "dup_msg",
                "chat_type": "single",
                "msg_type": "text",
                "content": {"text": "hi"},
                "sender": {"user_id": "u1"},
            },
        }
        adapter._enqueue_message(data)
        adapter._enqueue_message(data)  # duplicate
        msgs = adapter.poll()
        self.assertEqual(len(msgs), 1)


class TestWecomConnect(unittest.TestCase):
    """Step 8: connect() early failure detection tests."""

    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        return WecomAdapter(bot_id="corp", secret="sec")

    def test_connect_disables_proxies_before_start(self):
        adapter = self._make_adapter()
        calls: list[str] = []

        def fake_disable() -> None:
            calls.append("disable")

        def fake_start() -> None:
            calls.append("start")
            adapter._ws_started.set()
            adapter._ws_running = False
            adapter._ws_thread = None

        with patch.object(adapter, "_disable_proxies", side_effect=fake_disable):
            with patch.object(adapter, "_start_ws_listener", side_effect=fake_start):
                result = adapter.connect()

        self.assertTrue(result)
        self.assertEqual(calls, ["disable", "start"])

    def test_connect_fails_on_first_websocket_error(self):
        adapter = self._make_adapter()

        def fake_start() -> None:
            adapter._ws_connect_error = "boom"
            adapter._ws_started.set()

        with patch.object(adapter, "_start_ws_listener", side_effect=fake_start):
            result = adapter.connect()

        self.assertFalse(result)


class TestWecomWebSocketReconnect(unittest.TestCase):
    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        return WecomAdapter(bot_id="corp", secret="sec")

    def test_subsequent_websocket_failure_does_not_become_first_connect_fatal(self):
        adapter = self._make_adapter()

        attempts = {"count": 0}
        sleep_calls = {"count": 0}

        class FakeWebSocketApp:
            def __init__(self, url, on_open, on_message, on_error, on_close):
                self.url = url
                self._on_open = on_open
                self._on_message = on_message
                self._on_error = on_error
                self._on_close = on_close

            def send(self, payload: str) -> None:
                data = __import__("json").loads(payload)
                cmd = str(data.get("cmd") or "")
                req_id = str((data.get("headers") or {}).get("req_id") or "")
                if cmd == "aibot_subscribe":
                    self._on_message(self, __import__("json").dumps({
                        "headers": {"req_id": req_id},
                        "errcode": 0,
                        "errmsg": "ok",
                    }))

            def close(self) -> None:
                return None

            def run_forever(self, ping_interval: int = 0) -> None:
                _ = ping_interval
                attempts["count"] += 1
                if attempts["count"] == 1:
                    self._on_open(self)
                    return
                self._on_error(self, RuntimeError("reconnect failed"))

        class ImmediateThread:
            def __init__(self, target=None, kwargs=None, daemon=None):
                self._target = target
                self._kwargs = kwargs or {}
                self._alive = False

            def start(self) -> None:
                self._alive = True
                try:
                    if self._target:
                        self._target(**self._kwargs)
                finally:
                    self._alive = False

            def is_alive(self) -> bool:
                return self._alive

        fake_websocket_module = type("FakeWebSocketModule", (), {"WebSocketApp": FakeWebSocketApp})

        def fake_sleep(seconds: float) -> None:
            if seconds >= 1.0:
                sleep_calls["count"] += 1
                if sleep_calls["count"] >= 2:
                    adapter._ws_running = False

        with patch.dict(sys.modules, {"websocket": fake_websocket_module}):
            with patch("cccc.ports.im.adapters.wecom.threading.Thread", ImmediateThread):
                with patch("cccc.ports.im.adapters.wecom.random.uniform", return_value=0.0):
                    with patch("cccc.ports.im.adapters.wecom.time.sleep", side_effect=fake_sleep):
                        adapter._start_ws_listener()

        self.assertTrue(adapter._ws_started.is_set())
        self.assertIsNone(adapter._ws_connect_error)
        self.assertGreaterEqual(attempts["count"], 2)

    def test_missing_heartbeat_ack_triggers_reconnect_backoff(self):
        from cccc.ports.im.adapters.wecom import WS_HEARTBEAT_INTERVAL

        adapter = self._make_adapter()
        sends: list[str] = []
        sleep_calls: list[float] = []

        class FakeWebSocketApp:
            def __init__(self, url, on_open, on_message, on_error, on_close):
                self._on_open = on_open
                self._on_message = on_message
                self._on_error = on_error
                self._on_close = on_close

            def send(self, payload: str) -> None:
                data = __import__("json").loads(payload)
                cmd = str(data.get("cmd") or "")
                sends.append(cmd)
                if cmd == "aibot_subscribe":
                    self._on_message(
                        self,
                        __import__("json").dumps(
                            {
                                "headers": {"req_id": str((data.get("headers") or {}).get("req_id") or "")},
                                "errcode": 0,
                                "errmsg": "ok",
                            }
                        ),
                    )

            def close(self) -> None:
                return None

            def run_forever(self, ping_interval: int = 0) -> None:
                _ = ping_interval
                self._on_open(self)

        class FakeThread:
            def __init__(self, target=None, kwargs=None, daemon=None):
                self._target = target
                self._kwargs = kwargs or {}
                self._alive = False
                self._keep_alive_after_start = getattr(target, "__name__", "") == "run_forever"

            def start(self) -> None:
                self._alive = True
                if self._target:
                    self._target(**self._kwargs)
                if not self._keep_alive_after_start:
                    self._alive = False

            def is_alive(self) -> bool:
                return self._alive

        fake_websocket_module = type("FakeWebSocketModule", (), {"WebSocketApp": FakeWebSocketApp})

        def fake_sleep(seconds: float) -> None:
            sleep_calls.append(seconds)
            if 1.0 <= seconds < WS_HEARTBEAT_INTERVAL:
                adapter._ws_running = False

        with patch.dict(sys.modules, {"websocket": fake_websocket_module}):
            with patch("cccc.ports.im.adapters.wecom.threading.Thread", FakeThread):
                with patch("cccc.ports.im.adapters.wecom.random.uniform", return_value=0.0):
                    with patch("cccc.ports.im.adapters.wecom.time.sleep", side_effect=fake_sleep):
                        adapter._start_ws_listener()

        self.assertTrue(adapter._ws_started.is_set())
        self.assertIsNone(adapter._ws_connect_error)
        self.assertEqual(sends.count("ping"), 3)
        self.assertIn(WS_HEARTBEAT_INTERVAL, sleep_calls)
        self.assertIn(1.0, sleep_calls)


class TestWecomRespondHandles(unittest.TestCase):
    """Step 10-11: callback req_id capture and cache behavior."""

    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        return WecomAdapter(bot_id="corp", secret="sec")

    def test_enqueue_captures_reply_req_id(self):
        adapter = self._make_adapter()
        data = {
            "cmd": "aibot_msg_callback",
            "headers": {"req_id": "req_abc123"},
            "body": {
                "chatid": "conv_1",
                "msgid": "msg_1",
                "chattype": "single",
                "msgtype": "text",
                "text": {"content": "hello"},
                "from": {"userid": "u1"},
            },
        }
        adapter._enqueue_message(data)
        self.assertEqual(adapter._get_reply_req_id("conv_1"), "req_abc123")

    def test_enqueue_captures_response_url(self):
        adapter = self._make_adapter()
        data = {
            "cmd": "aibot_msg_callback",
            "headers": {"req_id": "req_abc123"},
            "body": {
                "chatid": "conv_1",
                "msgid": "msg_1",
                "chattype": "single",
                "msgtype": "text",
                "text": {"content": "hello"},
                "response_url": "https://bot.example.test/response",
                "from": {"userid": "u1"},
            },
        }
        adapter._enqueue_message(data)
        self.assertEqual(adapter._get_reply_req_id("conv_1"), "req_abc123")
        self.assertEqual(adapter._get_response_url("conv_1"), "https://bot.example.test/response")

    def test_reply_req_id_does_not_artificially_expire(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", "req_old")
        adapter._reply_refs["conv_1"]["ts"] = 1.0
        self.assertEqual(adapter._get_reply_req_id("conv_1"), "req_old")

    def test_reply_ref_cache_prunes_oldest_entries(self):
        from cccc.ports.im.adapters.wecom import REPLY_REF_MAX_ENTRIES

        adapter = self._make_adapter()
        for idx in range(REPLY_REF_MAX_ENTRIES + 8):
            adapter._store_reply_ref(f"conv_{idx}", f"req_{idx}")
            adapter._reply_refs[f"conv_{idx}"]["ts"] = float(idx)

        adapter._store_reply_ref("conv_latest", "req_latest")

        self.assertLessEqual(len(adapter._reply_refs), REPLY_REF_MAX_ENTRIES)
        self.assertEqual(adapter._get_reply_req_id("conv_latest"), "req_latest")
        self.assertEqual(adapter._get_reply_req_id("conv_0"), "")

    def test_reply_req_id_missing_returns_empty(self):
        adapter = self._make_adapter()
        self.assertEqual(adapter._get_reply_req_id("nonexistent"), "")

    def test_reply_req_id_concurrent_reads_are_stable(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", "req_shared")
        results: list[str] = []
        result_lock = threading.Lock()

        def reader() -> None:
            local = [adapter._get_reply_req_id("conv_1") for _ in range(20)]
            with result_lock:
                results.extend(local)

        threads = [threading.Thread(target=reader) for _ in range(8)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()

        self.assertEqual(len(results), 160)
        self.assertTrue(all(value == "req_shared" for value in results))


class TestWecomRateLimiter(unittest.TestCase):
    def test_wait_and_acquire_sleeps_for_same_chat_burst(self):
        from cccc.ports.im.adapters.wecom import RateLimiter

        limiter = RateLimiter(max_per_second=2.0)
        fake_clock = {"now": 100.0}
        sleep_calls: list[float] = []

        def fake_time() -> float:
            return fake_clock["now"]

        def fake_sleep(seconds: float) -> None:
            sleep_calls.append(seconds)
            fake_clock["now"] += seconds

        with patch("cccc.ports.im.adapters.wecom.time.time", side_effect=fake_time):
            with patch("cccc.ports.im.adapters.wecom.time.sleep", side_effect=fake_sleep):
                limiter.wait_and_acquire("chat-1")
                limiter.wait_and_acquire("chat-1")

        self.assertEqual(len(sleep_calls), 1)
        self.assertAlmostEqual(sleep_calls[0], 0.5, places=3)


class TestWecomSendMessage(unittest.TestCase):
    """Step 10: send_message tests."""

    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        adapter = WecomAdapter(bot_id="corp", secret="sec")
        adapter._connected = True
        return adapter

    def test_send_empty_text_returns_true(self):
        adapter = self._make_adapter()
        self.assertTrue(adapter.send_message("conv_1", ""))

    def test_send_when_disconnected_returns_false(self):
        adapter = self._make_adapter()
        adapter._connected = False
        self.assertFalse(adapter.send_message("conv_1", "hello"))

    def test_send_via_ws_respond(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", "req_test")
        with patch.object(adapter, "_ws_send_and_wait_ack", return_value=(True, {"errcode": 0})) as mock_ws:
            result = adapter.send_message("conv_1", "hello")
            self.assertTrue(result)
            mock_ws.assert_called_once()
            call_payload = mock_ws.call_args[0][0]
            self.assertEqual(call_payload["cmd"], "aibot_respond_msg")
            self.assertEqual(call_payload["headers"]["req_id"], "req_test")
            self.assertTrue(call_payload["body"]["stream"]["finish"])
            self.assertEqual(call_payload["body"]["stream"]["content"], "hello")
            self.assertEqual(call_payload["body"]["msgtype"], "stream")

    def test_send_without_reply_ref_uses_active_markdown_send(self):
        adapter = self._make_adapter()
        with patch.object(adapter, "_ws_send_and_wait_ack", return_value=(True, {"errcode": 0})) as mock_ws:
            result = adapter.send_message("conv_1", "hello")

        self.assertTrue(result)
        payload = mock_ws.call_args[0][0]
        self.assertEqual(payload["cmd"], "aibot_send_msg")
        self.assertEqual(payload["body"]["chatid"], "conv_1")
        self.assertEqual(payload["body"]["msgtype"], "markdown")
        self.assertEqual(payload["body"]["markdown"]["content"], "hello")

    def test_send_returns_false_when_ws_fails(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", "req_test")
        with patch.object(adapter, "_ws_send_and_wait_ack", return_value=(False, None)):
            result = adapter.send_message("conv_1", "hello")
            self.assertFalse(result)

    def test_send_returns_false_when_ack_rejects(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", "req_test")
        with patch.object(
            adapter,
            "_ws_send_and_wait_ack",
            return_value=(False, {"errcode": 40008, "errmsg": "invalid message type"}),
        ):
            result = adapter.send_message("conv_1", "hello")
            self.assertFalse(result)

    def test_send_uses_response_url_when_req_id_missing(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", response_url="https://bot.example.test/response")
        with patch.object(adapter, "_post_json", return_value=True) as mock_post:
            result = adapter.send_message("conv_1", "hello")

        self.assertTrue(result)
        mock_post.assert_called_once()
        self.assertEqual(mock_post.call_args[0][0], "https://bot.example.test/response")
        payload = mock_post.call_args[0][1]
        self.assertEqual(payload["msgtype"], "stream")
        self.assertTrue(payload["stream"]["finish"])
        self.assertEqual(payload["stream"]["content"], "hello")

    def test_compose_safe_truncates(self):
        adapter = self._make_adapter()
        long_text = "x" * 3000
        safe = adapter._compose_safe(long_text)
        self.assertLessEqual(len(safe), adapter.max_chars)
        self.assertIn("truncated", safe)


class _FakeHttpResponse:
    def __init__(self, payload: bytes):
        self._payload = payload

    def read(self) -> bytes:
        return self._payload

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb) -> bool:
        _ = exc_type
        _ = exc
        _ = tb
        return False


class TestWecomAttachments(unittest.TestCase):
    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        adapter = WecomAdapter(bot_id="corp", secret="sec")
        adapter._connected = True
        return adapter

    def test_download_attachment_uses_media_get(self):
        adapter = self._make_adapter()

        def fake_urlopen(req, timeout=0):
            self.assertIn("/media/get?", req.full_url)
            self.assertIn("media_id=media_abc", req.full_url)
            self.assertIn("bot_id=corp", req.full_url)
            self.assertIn("secret=sec", req.full_url)
            self.assertEqual(timeout, 60)
            return _FakeHttpResponse(b"image-bytes")

        with patch("cccc.ports.im.adapters.wecom.urllib.request.urlopen", side_effect=fake_urlopen):
            raw = adapter.download_attachment({"media_id": "media_abc"})

        self.assertEqual(raw, b"image-bytes")

    def test_download_attachment_uses_direct_url_and_decrypts_when_aeskey_present(self):
        adapter = self._make_adapter()

        def fake_urlopen(req, timeout=0):
            self.assertEqual(req.full_url, "https://example.test/media.enc")
            self.assertEqual(timeout, 60)
            return _FakeHttpResponse(b"encrypted-bytes")

        with patch("cccc.ports.im.adapters.wecom.urllib.request.urlopen", side_effect=fake_urlopen):
            with patch.object(adapter, "_decrypt_media_bytes", return_value=b"plain-bytes") as mock_decrypt:
                raw = adapter.download_attachment({
                    "download_url": "https://example.test/media.enc",
                    "aeskey": "12345678901234567890123456789012",
                })

        self.assertEqual(raw, b"plain-bytes")
        mock_decrypt.assert_called_once_with(b"encrypted-bytes", "12345678901234567890123456789012")

    def test_download_attachment_decrypts_direct_url_without_external_openssl(self):
        adapter = self._make_adapter()
        aes_key = "12345678901234567890123456789012"
        encrypted = _encrypt_wecom_media_for_test(b"plain attachment bytes", aes_key)

        def fake_urlopen(req, timeout=0):
            self.assertEqual(req.full_url, "https://example.test/media.enc")
            self.assertEqual(timeout, 60)
            return _FakeHttpResponse(encrypted)

        with patch("cccc.ports.im.adapters.wecom.urllib.request.urlopen", side_effect=fake_urlopen):
            with patch.object(subprocess, "run", side_effect=AssertionError("external openssl should not be used")):
                raw = adapter.download_attachment({
                    "download_url": "https://example.test/media.enc",
                    "aeskey": aes_key,
                })

        self.assertEqual(raw, b"plain attachment bytes")

    def test_download_attachment_uses_direct_url_without_decrypt_when_no_aeskey(self):
        adapter = self._make_adapter()

        def fake_urlopen(req, timeout=0):
            self.assertEqual(req.full_url, "https://example.test/media.bin")
            self.assertEqual(timeout, 60)
            return _FakeHttpResponse(b"raw-bytes")

        with patch("cccc.ports.im.adapters.wecom.urllib.request.urlopen", side_effect=fake_urlopen):
            with patch.object(adapter, "_decrypt_media_bytes") as mock_decrypt:
                raw = adapter.download_attachment({
                    "download_url": "https://example.test/media.bin",
                })

        self.assertEqual(raw, b"raw-bytes")
        mock_decrypt.assert_not_called()

    def test_send_file_uploads_image_and_sends_caption(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", "req_test")

        with tempfile.TemporaryDirectory() as td:
            image_path = Path(td) / "photo.png"
            image_path.write_bytes(b"\x89PNG\r\n\x1a\nfake")

            with patch("cccc.ports.im.adapters.wecom.urllib.request.urlopen") as mock_urlopen:
                with patch.object(adapter, "_ws_send_and_wait_ack", return_value=(True, {"errcode": 0})) as mock_ws:
                    with patch.object(adapter, "send_message", return_value=True) as mock_caption:
                        ok = adapter.send_file(
                            "conv_1",
                            file_path=image_path,
                            filename="photo.png",
                            caption="caption text",
                        )

        self.assertTrue(ok)
        mock_urlopen.assert_not_called()
        payload = mock_ws.call_args[0][0]
        self.assertEqual(payload["cmd"], "aibot_respond_msg")
        self.assertEqual(payload["headers"]["req_id"], "req_test")
        self.assertEqual(payload["body"]["msgtype"], "stream")
        self.assertEqual(payload["body"]["stream"]["content"], "caption text")
        self.assertEqual(payload["body"]["stream"]["msg_item"][0]["msgtype"], "image")
        self.assertTrue(payload["body"]["stream"]["msg_item"][0]["image"]["base64"])
        self.assertEqual(len(payload["body"]["stream"]["msg_item"][0]["image"]["md5"]), 32)
        mock_caption.assert_not_called()

    def test_send_file_uploads_regular_file_via_ws_media_upload(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", req_id="req_test")

        ws_calls: list[dict] = []

        def fake_ws_send_and_wait_ack(payload, *, timeout=5.0):
            _ = timeout
            ws_calls.append(payload)
            cmd = payload.get("cmd", "")
            if cmd == "aibot_upload_media_init":
                return True, {"errcode": 0, "body": {"upload_id": "upload_1"}}
            if cmd == "aibot_upload_media_chunk":
                return True, {"errcode": 0, "body": {}}
            if cmd == "aibot_upload_media_finish":
                return True, {"errcode": 0, "body": {"media_id": "media_file", "type": "file"}}
            if cmd == "aibot_respond_msg":
                return True, {"errcode": 0}
            return True, {"errcode": 0}

        with tempfile.TemporaryDirectory() as td:
            file_path = Path(td) / "report.pdf"
            file_path.write_bytes(b"%PDF-1.4")

            with patch("cccc.ports.im.adapters.wecom.urllib.request.urlopen") as mock_urlopen:
                with patch.object(adapter, "_ws_send_and_wait_ack", side_effect=fake_ws_send_and_wait_ack) as mock_ws:
                    with patch.object(adapter, "send_message", return_value=True) as mock_caption:
                        ok = adapter.send_file(
                            "conv_1",
                            file_path=file_path,
                            filename="report.pdf",
                            caption="caption text",
                        )

        self.assertTrue(ok)
        mock_urlopen.assert_not_called()
        upload_init = [c for c in ws_calls if c.get("cmd") == "aibot_upload_media_init"]
        upload_chunk = [c for c in ws_calls if c.get("cmd") == "aibot_upload_media_chunk"]
        upload_finish = [c for c in ws_calls if c.get("cmd") == "aibot_upload_media_finish"]
        self.assertEqual(len(upload_init), 1)
        self.assertEqual(upload_init[0]["body"]["type"], "file")
        self.assertEqual(upload_init[0]["body"]["filename"], "report.pdf")
        self.assertEqual(upload_init[0]["body"]["total_size"], len(b"%PDF-1.4"))
        self.assertEqual(upload_init[0]["body"]["total_chunks"], 1)
        self.assertEqual(len(upload_init[0]["body"]["md5"]), 32)
        self.assertEqual(len(upload_chunk), 1)
        self.assertEqual(upload_chunk[0]["body"]["upload_id"], "upload_1")
        self.assertEqual(upload_chunk[0]["body"]["chunk_index"], 0)
        self.assertTrue(upload_chunk[0]["body"]["base64_data"])
        self.assertEqual(len(upload_finish), 1)
        self.assertEqual(upload_finish[0]["body"]["upload_id"], "upload_1")
        # WS respond_msg should contain file msgtype with correct media_id
        respond = [c for c in ws_calls if c.get("cmd") == "aibot_respond_msg"]
        self.assertEqual(len(respond), 1)
        self.assertEqual(respond[0]["body"]["msgtype"], "file")
        self.assertEqual(respond[0]["body"]["file"]["media_id"], "media_file")
        self.assertEqual(respond[0]["body"]["file"]["filename"], "report.pdf")
        mock_caption.assert_called_once_with("conv_1", "caption text")

    def test_send_file_without_reply_ref_uses_active_media_send(self):
        adapter = self._make_adapter()

        ws_calls: list[dict] = []

        def fake_ws_send_and_wait_ack(payload, *, timeout=5.0):
            _ = timeout
            ws_calls.append(payload)
            cmd = payload.get("cmd", "")
            if cmd == "aibot_upload_media_init":
                return True, {"errcode": 0, "body": {"upload_id": "upload_1"}}
            if cmd == "aibot_upload_media_chunk":
                return True, {"errcode": 0, "body": {}}
            if cmd == "aibot_upload_media_finish":
                return True, {"errcode": 0, "body": {"media_id": "media_image", "type": "image"}}
            if cmd == "aibot_send_msg":
                return True, {"errcode": 0}
            return False, None

        with tempfile.TemporaryDirectory() as td:
            image_path = Path(td) / "photo.webp"
            image_path.write_bytes(b"RIFFxxxxWEBP")

            with patch.object(adapter, "_ws_send_and_wait_ack", side_effect=fake_ws_send_and_wait_ack):
                ok = adapter.send_file(
                    "conv_1",
                    file_path=image_path,
                    filename="photo.webp",
                    caption="caption text",
                )

        self.assertTrue(ok)
        active_sends = [c for c in ws_calls if c.get("cmd") == "aibot_send_msg"]
        self.assertEqual(len(active_sends), 2)
        media_send = active_sends[0]
        caption_send = active_sends[1]
        self.assertEqual(media_send["body"]["chatid"], "conv_1")
        self.assertEqual(media_send["body"]["msgtype"], "image")
        self.assertEqual(media_send["body"]["image"]["media_id"], "media_image")
        self.assertEqual(caption_send["body"]["chatid"], "conv_1")
        self.assertEqual(caption_send["body"]["msgtype"], "markdown")
        self.assertEqual(caption_send["body"]["markdown"]["content"], "caption text")

class TestWecomStreaming(unittest.TestCase):
    """Step 11: Streaming reply tests."""

    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        adapter = WecomAdapter(bot_id="corp", secret="sec")
        adapter._connected = True
        return adapter

    def test_begin_stream_returns_none_without_handle(self):
        adapter = self._make_adapter()
        result = adapter.begin_stream("conv_no_handle", "stream_1")
        self.assertIsNone(result)

    def test_begin_stream_returns_handle(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", "req_stream")
        with patch.object(adapter, "_ws_send_and_wait_ack", return_value=(True, {"errcode": 0})):
            handle = adapter.begin_stream("conv_1", "s1", text="starting...")
            self.assertIsNotNone(handle)
            self.assertEqual(handle["stream_id"], "s1")
            self.assertEqual(
                handle["platform_handle"],
                {"chat_id": "conv_1", "req_id": "req_stream", "response_url": "", "stream_id": "s1"},
            )

    def test_begin_stream_returns_handle_with_response_url_only(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", response_url="https://bot.example.test/response")
        handle = adapter.begin_stream("conv_1", "s1")

        self.assertIsNotNone(handle)
        self.assertEqual(
            handle["platform_handle"],
            {
                "chat_id": "conv_1",
                "req_id": "",
                "response_url": "https://bot.example.test/response",
                "stream_id": "s1",
            },
        )

    def test_update_stream_sends_intermediate(self):
        adapter = self._make_adapter()
        handle = {"stream_id": "s1", "platform_handle": {"chat_id": "conv_1", "req_id": "req_test", "response_url": "", "stream_id": "s1"}}
        with patch.object(adapter, "_ws_send_and_wait_ack", return_value=(True, {"errcode": 0})) as mock_ws:
            result = adapter.update_stream(handle, text="chunk 1")
            self.assertTrue(result)
            payload = mock_ws.call_args[0][0]
            self.assertFalse(payload["body"]["stream"]["finish"])
            self.assertEqual(payload["body"]["stream"]["content"], "chunk 1")

    def test_end_stream_sends_final(self):
        adapter = self._make_adapter()
        handle = {"stream_id": "s1", "platform_handle": {"chat_id": "conv_1", "req_id": "req_test", "response_url": "", "stream_id": "s1"}}
        with patch.object(adapter, "_ws_send_and_wait_ack", return_value=(True, {"errcode": 0})) as mock_ws:
            result = adapter.end_stream(handle, text="final text")
            self.assertTrue(result)
            payload = mock_ws.call_args[0][0]
            self.assertTrue(payload["body"]["stream"]["finish"])
            self.assertEqual(payload["body"]["stream"]["content"], "final text")

    def test_update_stream_uses_response_url_when_req_id_missing(self):
        adapter = self._make_adapter()
        handle = {
            "stream_id": "s1",
            "platform_handle": {
                "chat_id": "conv_1",
                "req_id": "",
                "response_url": "https://bot.example.test/response",
                "stream_id": "s1",
            },
        }
        with patch.object(adapter, "_post_json", return_value=True) as mock_post:
            result = adapter.update_stream(handle, text="chunk 1")

        self.assertTrue(result)
        mock_post.assert_called_once()
        self.assertEqual(mock_post.call_args[0][0], "https://bot.example.test/response")
        payload = mock_post.call_args[0][1]
        self.assertFalse(payload["stream"]["finish"])
        self.assertEqual(payload["stream"]["content"], "chunk 1")

    def test_end_stream_uses_response_url_when_req_id_missing(self):
        adapter = self._make_adapter()
        handle = {
            "stream_id": "s1",
            "platform_handle": {
                "chat_id": "conv_1",
                "req_id": "",
                "response_url": "https://bot.example.test/response",
                "stream_id": "s1",
            },
        }
        with patch.object(adapter, "_post_json", return_value=True) as mock_post:
            result = adapter.end_stream(handle, text="")

        self.assertTrue(result)
        mock_post.assert_called_once()
        self.assertEqual(mock_post.call_args[0][0], "https://bot.example.test/response")
        payload = mock_post.call_args[0][1]
        self.assertTrue(payload["stream"]["finish"])
        self.assertEqual(payload["stream"]["content"], "")

    def test_update_stream_fails_with_empty_handle(self):
        adapter = self._make_adapter()
        handle = {"stream_id": "s1", "platform_handle": {}}
        self.assertFalse(adapter.update_stream(handle, text="x"))

    def test_end_stream_fails_with_empty_handle(self):
        adapter = self._make_adapter()
        handle = {"stream_id": "s1", "platform_handle": {}}
        self.assertFalse(adapter.end_stream(handle, text="x"))


class _AckingWebSocket:
    def __init__(self, adapter, ack_frame):
        self.adapter = adapter
        self.ack_frame = ack_frame

    def send(self, payload: str) -> None:
        _ = payload
        threading.Thread(
            target=lambda: self.adapter._resolve_reply_ack(self.ack_frame),
            daemon=True,
        ).start()


class TestWecomReplyAck(unittest.TestCase):
    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        adapter = WecomAdapter(bot_id="corp", secret="sec")
        adapter._connected = True
        return adapter

    def test_ws_send_and_wait_ack_returns_true_on_success(self):
        adapter = self._make_adapter()
        frame = {
            "cmd": "aibot_respond_msg",
            "headers": {"req_id": "req_ok"},
            "body": {"msgtype": "stream", "stream": {"id": "s1", "finish": True, "content": "ok"}},
        }
        adapter._ws_app = _AckingWebSocket(adapter, {"headers": {"req_id": "req_ok"}, "errcode": 0})

        ok, ack = adapter._ws_send_and_wait_ack(frame, timeout=0.5)

        self.assertTrue(ok)
        self.assertEqual(ack["errcode"], 0)

    def test_ws_send_and_wait_ack_returns_false_on_rejection(self):
        adapter = self._make_adapter()
        frame = {
            "cmd": "aibot_respond_msg",
            "headers": {"req_id": "req_bad"},
            "body": {"msgtype": "stream", "stream": {"id": "s1", "finish": True, "content": "bad"}},
        }
        adapter._ws_app = _AckingWebSocket(
            adapter,
            {"headers": {"req_id": "req_bad"}, "errcode": 40008, "errmsg": "invalid message type"},
        )

        ok, ack = adapter._ws_send_and_wait_ack(frame, timeout=0.5)

        self.assertFalse(ok)
        self.assertEqual(ack["errcode"], 40008)


class TestWecomDisconnect(unittest.TestCase):
    """Step 13: Enhanced disconnect tests."""

    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        adapter = WecomAdapter(bot_id="corp", secret="sec")
        adapter._connected = True
        return adapter

    def test_disconnect_clears_state(self):
        adapter = self._make_adapter()
        adapter._store_reply_ref("conv_1", "req_1")
        adapter._seen_msg_ids["k"] = time.time()
        with adapter._queue_lock:
            adapter._message_queue.append({"text": "leftover"})

        adapter.disconnect()

        self.assertFalse(adapter._connected)
        self.assertEqual(len(adapter._reply_refs), 0)
        self.assertEqual(len(adapter._seen_msg_ids), 0)
        self.assertEqual(len(adapter._message_queue), 0)


class TestWecomGetChatTitle(unittest.TestCase):
    """Step 13: get_chat_title tests."""

    def _make_adapter(self):
        from cccc.ports.im.adapters.wecom import WecomAdapter
        return WecomAdapter(bot_id="corp", secret="sec")

    def test_returns_chat_id(self):
        adapter = self._make_adapter()
        self.assertEqual(adapter.get_chat_title("chat_1"), "chat_1")


if __name__ == "__main__":
    unittest.main()
