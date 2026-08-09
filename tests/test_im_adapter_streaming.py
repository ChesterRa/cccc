from __future__ import annotations

import asyncio
import json
import threading
from typing import Any

from cccc.ports.im.adapters.discord import DiscordAdapter
from cccc.ports.im.adapters.feishu import FeishuAdapter
from cccc.ports.im.adapters.slack import SlackAdapter
from cccc.ports.im.adapters.telegram import TelegramAdapter


class _SlackClient:
    def __init__(self) -> None:
        self.posts: list[str] = []
        self.updates: list[str] = []

    def chat_postMessage(self, *, channel: str, text: str) -> dict[str, Any]:
        self.posts.append(text)
        return {"ok": True, "ts": f"{channel}-{len(self.posts)}"}

    def chat_update(self, *, channel: str, ts: str, text: str) -> dict[str, Any]:
        _ = (channel, ts)
        self.updates.append(text)
        return {"ok": True}


def test_slack_final_delivery_is_lossless_and_streams_with_updates() -> None:
    adapter = SlackAdapter(bot_token="token", max_chars=5)
    client = _SlackClient()
    adapter._connected = True
    adapter._web_client = client

    text = "你好🙂abcdef"
    assert adapter.send_message("channel", text) is True
    assert "".join(client.posts) == text

    handle = adapter.begin_stream("channel", "stream-1", text="a")
    assert handle is not None
    assert adapter.update_stream(handle, text="abcd", seq=1) is True
    assert adapter.end_stream(handle, text="abcd") is True
    assert client.updates[-1] == "abcd"

    long_handle = adapter.begin_stream("channel", "stream-2", text="a")
    assert long_handle is not None
    assert adapter.end_stream(long_handle, text="abcdef") is False
    assert client.updates[-1] == "abcd…"


def test_telegram_final_delivery_is_lossless_and_streams_with_edits() -> None:
    adapter = TelegramAdapter(token="token", max_chars=5)
    adapter._connected = True
    adapter._rate_limiter.wait_and_acquire = lambda _chat_id: None  # type: ignore[method-assign]
    sent: list[str] = []
    edits: list[str] = []

    def fake_api(
        method: str,
        params: dict[str, Any] | None = None,
        timeout: int = 35,
    ) -> dict[str, Any]:
        _ = timeout
        payload = params or {}
        if method == "sendMessage":
            sent.append(str(payload.get("text") or ""))
            return {"ok": True, "result": {"message_id": len(sent)}}
        if method == "editMessageText":
            edits.append(str(payload.get("text") or ""))
            return {"ok": True, "result": {}}
        raise AssertionError(method)

    adapter._api = fake_api  # type: ignore[method-assign]

    text = "你好🙂abcdef"
    assert adapter.send_message("chat", text) is True
    assert "".join(sent) == text

    handle = adapter.begin_stream("chat", "stream-1", text="a", thread_id=7)
    assert handle is not None
    assert adapter.update_stream(handle, text="abcd", seq=1) is True
    assert adapter.end_stream(handle, text="abcd") is True
    assert edits[-1] == "abcd"

    long_handle = adapter.begin_stream("chat", "stream-2", text="a")
    assert long_handle is not None
    assert adapter.end_stream(long_handle, text="abcdef") is False
    assert edits[-1] == "abcd…"


def test_feishu_final_delivery_is_lossless_and_streams_with_updates() -> None:
    adapter = FeishuAdapter(app_id="app", app_secret="secret", max_chars=5)
    adapter._connected = True
    adapter._rate_limiter.wait_and_acquire = lambda _chat_id: None  # type: ignore[method-assign]
    posts: list[str] = []
    patches: list[str] = []

    def fake_api(
        method: str,
        endpoint: str,
        body: dict[str, Any] | None = None,
        timeout: int = 15,
    ) -> dict[str, Any]:
        _ = timeout
        payload = body or {}
        content = json.loads(str(payload.get("content") or "{}"))
        if method == "POST":
            posts.append(str(content.get("text") or ""))
            return {"code": 0, "data": {"message_id": f"m-{len(posts)}"}}
        if method == "PUT":
            assert endpoint.startswith("/im/v1/messages/")
            patches.append(str(content.get("text") or ""))
            return {"code": 0}
        raise AssertionError(method)

    adapter._api = fake_api  # type: ignore[method-assign]

    text = "你好🙂abcdef"
    assert adapter.send_message("chat", text) is True
    assert "".join(posts) == text

    handle = adapter.begin_stream("chat", "stream-1", text="a", thread_id=7)
    assert handle is not None
    assert adapter.update_stream(handle, text="abcd", seq=1) is True
    assert adapter.end_stream(handle, text="abcd") is True
    assert patches[-1] == "abcd"

    long_handle = adapter.begin_stream("chat", "stream-2", text="a")
    assert long_handle is not None
    assert adapter.end_stream(long_handle, text="abcdef") is False
    assert patches[-1] == "abcd…"


class _DiscordMessage:
    def __init__(self, content: str) -> None:
        self.content = content
        self.edits: list[str] = []

    async def edit(self, *, content: str) -> "_DiscordMessage":
        self.content = content
        self.edits.append(content)
        return self


class _DiscordChannel:
    def __init__(self) -> None:
        self.messages: list[_DiscordMessage] = []

    async def send(self, content: str) -> _DiscordMessage:
        message = _DiscordMessage(content)
        self.messages.append(message)
        return message


class _DiscordClient:
    def __init__(self, channel: _DiscordChannel) -> None:
        self.channel = channel

    def get_channel(self, _channel_id: int) -> _DiscordChannel:
        return self.channel


def test_discord_final_delivery_is_lossless_and_streams_with_edits() -> None:
    adapter = DiscordAdapter(token="token", max_chars=5)
    channel = _DiscordChannel()
    adapter._connected = True
    adapter._client = _DiscordClient(channel)
    loop = asyncio.new_event_loop()
    adapter._loop = loop
    loop_thread = threading.Thread(target=loop.run_forever, daemon=True)
    loop_thread.start()

    try:
        text = "你好🙂abcdef"
        assert adapter.send_message("123", text) is True
        assert "".join(message.content for message in channel.messages) == text

        handle = adapter.begin_stream("123", "stream-1", text="a")
        assert handle is not None
        assert adapter.update_stream(handle, text="abcd", seq=1) is True
        assert adapter.end_stream(handle, text="abcd") is True

        long_handle = adapter.begin_stream("123", "stream-2", text="a")
        assert long_handle is not None
        assert adapter.end_stream(long_handle, text="abcdef") is False
        platform_handle = long_handle["platform_handle"]
        assert isinstance(platform_handle, dict)
        assert platform_handle["message"].content == "abcd…"
    finally:
        loop.call_soon_threadsafe(loop.stop)
        loop_thread.join(timeout=2)
        loop.close()
