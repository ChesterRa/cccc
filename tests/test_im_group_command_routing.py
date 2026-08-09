import asyncio
import sys
import types
from types import SimpleNamespace
from unittest.mock import patch

from cccc.ports.im.adapters.discord import DiscordAdapter
from cccc.ports.im.adapters.slack import SlackAdapter
from cccc.ports.im.commands import is_recognized_command


def test_only_recognized_cccc_commands_bypass_group_mentions() -> None:
    for text in (
        "/subscribe",
        "/status",
        "/send @all hello",
        "/context",
        "/launch",
        "/quit",
        "/help@cccc_bot",
    ):
        assert is_recognized_command(text), text

    for text in ("/deploy", "/weather tomorrow", "hello", ""):
        assert not is_recognized_command(text), text


def test_slack_channel_prefilter_keeps_known_commands_only() -> None:
    response_module = types.ModuleType("slack_sdk.socket_mode.response")

    class SocketModeResponse:
        def __init__(self, *, envelope_id: str) -> None:
            self.envelope_id = envelope_id

    response_module.SocketModeResponse = SocketModeResponse  # type: ignore[attr-defined]
    socket_mode_module = types.ModuleType("slack_sdk.socket_mode")
    socket_mode_module.response = response_module  # type: ignore[attr-defined]
    slack_module = types.ModuleType("slack_sdk")
    slack_module.socket_mode = socket_mode_module  # type: ignore[attr-defined]

    adapter = SlackAdapter(bot_token="token")
    adapter._bot_user_id = "U-BOT"
    client = SimpleNamespace(send_socket_mode_response=lambda _response: None)

    def dispatch(text: str) -> None:
        request = SimpleNamespace(
            envelope_id="envelope",
            type="events_api",
            payload={
                "event": {
                    "type": "message",
                    "channel": "C123",
                    "channel_type": "channel",
                    "user": "U123",
                    "text": text,
                    "ts": "1.0",
                }
            },
        )
        adapter._handle_socket_event(client, request)

    with patch.dict(
        sys.modules,
        {
            "slack_sdk": slack_module,
            "slack_sdk.socket_mode": socket_mode_module,
            "slack_sdk.socket_mode.response": response_module,
        },
    ):
        dispatch("/status")
        dispatch("/weather")

    assert [message["text"] for message in adapter._message_queue] == ["/status"]
    assert adapter._message_queue[0]["routed"] is False


def test_discord_guild_prefilter_keeps_known_commands_only() -> None:
    adapter = DiscordAdapter(token="token")
    bot_user = SimpleNamespace(id=999)
    adapter._client = SimpleNamespace(user=bot_user)

    def message(text: str, message_id: int) -> SimpleNamespace:
        return SimpleNamespace(
            author=SimpleNamespace(id=123, name="alice"),
            content=text,
            attachments=[],
            guild=SimpleNamespace(id=1),
            mentions=[],
            channel=SimpleNamespace(id=456, name="general"),
            created_at=SimpleNamespace(timestamp=lambda: 1.0),
            id=message_id,
        )

    asyncio.run(adapter._handle_message(message("/status", 1)))
    asyncio.run(adapter._handle_message(message("/weather", 2)))

    assert [item["text"] for item in adapter._message_queue] == ["/status"]
    assert adapter._message_queue[0]["routed"] is False
