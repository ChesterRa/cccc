from __future__ import annotations

import asyncio
import sys
import time
import types
import unittest
from unittest.mock import patch


class _FakeIntents:
    @staticmethod
    def default() -> types.SimpleNamespace:
        return types.SimpleNamespace(message_content=False)


class _FakeDiscordClient:
    def __init__(self, *, intents: object, proxy: str | None = None):
        self.intents = intents
        self.proxy = proxy
        self.user = "fake-bot"
        self._events: dict[str, object] = {}

    def event(self, fn):  # noqa: ANN001
        self._events[getattr(fn, "__name__", "")] = fn
        return fn

    async def start(self, token: str) -> None:
        _ = token
        on_ready = self._events.get("on_ready")
        if callable(on_ready):
            await on_ready()

    async def close(self) -> None:
        return None


class _SyncThread:
    def __init__(self, *, target=None, daemon: bool = False):  # noqa: ANN001
        self._target = target
        self.daemon = daemon

    def start(self) -> None:
        if callable(self._target):
            self._target()


class _LoopSuccess:
    def run_until_complete(self, coro):  # noqa: ANN001
        return asyncio.run(coro)

    def close(self) -> None:
        return None


class _LoopFailure:
    def run_until_complete(self, coro):  # noqa: ANN001
        try:
            raise RuntimeError("boom")
        finally:
            try:
                coro.close()
            except Exception:
                pass

    def close(self) -> None:
        return None


class TestDiscordAdapterConnect(unittest.TestCase):
    def test_connect_passes_resolved_proxy_into_discord_client(self) -> None:
        from cccc.ports.im.adapters.discord import DiscordAdapter

        fake_discord = types.SimpleNamespace(Intents=_FakeIntents, Client=_FakeDiscordClient)
        adapter = DiscordAdapter(token="token")

        with patch.dict(sys.modules, {"discord": fake_discord}), patch(
            "cccc.ports.im.adapters.discord._resolve_proxy",
            return_value="http://user:pass@127.0.0.1:8080",
        ), patch(
            "cccc.ports.im.adapters.discord.threading.Thread",
            _SyncThread,
        ), patch(
            "cccc.ports.im.adapters.discord.asyncio.new_event_loop",
            return_value=_LoopSuccess(),
        ), patch(
            "cccc.ports.im.adapters.discord.asyncio.set_event_loop",
            return_value=None,
        ):
            ok = adapter.connect()

        self.assertTrue(ok)
        self.assertTrue(adapter._connected)
        self.assertIsNotNone(adapter._client)
        self.assertEqual(
            str(getattr(adapter._client, "proxy", "")),
            "http://user:pass@127.0.0.1:8080",
        )

    def test_connect_returns_false_quickly_when_client_start_fails(self) -> None:
        from cccc.ports.im.adapters.discord import DiscordAdapter

        fake_discord = types.SimpleNamespace(Intents=_FakeIntents, Client=_FakeDiscordClient)
        adapter = DiscordAdapter(token="token")

        start = time.monotonic()
        with patch.dict(sys.modules, {"discord": fake_discord}), patch(
            "cccc.ports.im.adapters.discord._resolve_proxy",
            return_value=None,
        ), patch(
            "cccc.ports.im.adapters.discord.threading.Thread",
            _SyncThread,
        ), patch(
            "cccc.ports.im.adapters.discord.asyncio.new_event_loop",
            return_value=_LoopFailure(),
        ), patch(
            "cccc.ports.im.adapters.discord.asyncio.set_event_loop",
            return_value=None,
        ):
            ok = adapter.connect()
        elapsed = time.monotonic() - start

        self.assertFalse(ok)
        self.assertFalse(adapter._connected)
        self.assertEqual(str(getattr(adapter, "_connect_error", "")), "boom")
        self.assertLess(elapsed, 1.0)


if __name__ == "__main__":
    unittest.main()
