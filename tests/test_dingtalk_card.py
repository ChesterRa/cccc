"""Tests for DingTalk AI Card client (dingtalk_card.py)."""

from __future__ import annotations

import asyncio
import json
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from cccc.ports.im.adapters.dingtalk_card import (
    DingTalkAICardClient,
    DingTalkCardHandle,
    THROTTLE_INTERVAL_S,
)


# ── fixtures ─────────────────────────────────────────────────────────


@pytest.fixture
def token_fn():
    return lambda: "test-token-123"


@pytest.fixture
def client(token_fn):
    return DingTalkAICardClient(
        token_fn,
        robot_code="test_robot",
        throttle_interval=THROTTLE_INTERVAL_S,
    )


# ── helpers ──────────────────────────────────────────────────────────


def _run(coro):
    """Run an async coroutine synchronously."""
    return asyncio.run(coro)


def _mock_response(status: int = 200, body: dict | None = None):
    resp = AsyncMock()
    resp.status = status
    resp.text = AsyncMock(return_value=json.dumps(body or {}))
    return resp


def _async_cm(obj):
    cm = AsyncMock()
    cm.__aenter__ = AsyncMock(return_value=obj)
    cm.__aexit__ = AsyncMock(return_value=False)
    return cm


def _patch_aiohttp(resp_mock):
    session = AsyncMock()
    session.request = MagicMock(return_value=_async_cm(resp_mock))
    session_cm = _async_cm(session)
    return patch(
        "cccc.ports.im.adapters.dingtalk_card.aiohttp.ClientSession",
        return_value=session_cm,
    )


# ── create_card ──────────────────────────────────────────────────────


def test_create_card_returns_instance_id(client):
    """create_card should return a non-empty card_instance_id."""
    resp = _mock_response(200, {"success": True})
    with _patch_aiohttp(resp):
        card_id = _run(client.create_card("cidXXX123", "Hello"))
    assert card_id
    assert isinstance(card_id, str)
    assert len(card_id) == 32  # uuid4 hex


def test_create_card_group_space_id(client):
    """Group conversation should use IM_GROUP space."""
    captured = {}

    async def spy_api(method, endpoint, body, **kw):
        captured["body"] = body
        return {}

    client._api = spy_api
    _run(client.create_card("cidABC", "hi"))

    assert "IM_GROUP.cidABC" in captured["body"]["openSpaceId"]
    assert "imGroupOpenDeliverModel" in captured["body"]


def test_create_card_robot_space_id(client):
    """Non-group conversation should use IM_ROBOT space."""
    captured = {}

    async def spy_api(method, endpoint, body, **kw):
        captured["body"] = body
        return {}

    client._api = spy_api
    _run(client.create_card("user123", "hi"))

    assert "IM_ROBOT.user123" in captured["body"]["openSpaceId"]


def test_create_card_api_failure_returns_none(client):
    """API failure should return None so bridge can fall back to plain text."""
    resp = _mock_response(500, {"error": "fail"})
    with _patch_aiohttp(resp):
        card_id = _run(client.create_card("cidXXX", "text"))
    assert card_id is None


# ── update_card (no throttle) ────────────────────────────────────────


def test_update_card_immediate_send(client):
    """First update should be sent immediately (no throttle delay)."""
    calls = []

    async def spy_put(cid, content, *, is_finalize=False):
        calls.append((cid, content, is_finalize))

    client._put_streaming = spy_put
    _run(client.update_card("card1", "chunk-1"))

    assert len(calls) == 1
    assert calls[0] == ("card1", "chunk-1", False)


# ── update_card (throttle) ──────────────────────────────────────────


def test_update_card_throttle_buffers(client):
    """Rapid updates should be throttled — only first is sent immediately.

    Sync throttle model: buffered content is flushed on the next call that
    passes the throttle window, NOT via an async timer task.
    """
    calls = []

    async def spy_put(cid, content, *, is_finalize=False):
        calls.append(content)

    client._put_streaming = spy_put

    async def scenario():
        # First call: immediate
        await client.update_card("card1", "v1")
        assert len(calls) == 1

        # Second call right after: should be buffered
        await client.update_card("card1", "v2")
        assert len(calls) == 1

        # Third call: overwrites buffer
        await client.update_card("card1", "v3")
        assert len(calls) == 1

        # Wait for throttle window to pass
        await asyncio.sleep(THROTTLE_INTERVAL_S + 0.1)

        # Next call triggers flush of buffered "v3" — since enough time
        # has passed, "v4" is sent immediately (and "v3" was superseded).
        await client.update_card("card1", "v4")
        assert len(calls) == 2
        assert calls[1] == "v4"

    _run(scenario())


# ── finalize_card ────────────────────────────────────────────────────


def test_finalize_card_sends_is_finalize(client):
    """finalize_card should call _put_streaming with is_finalize=True."""
    calls = []

    async def spy_put(cid, content, *, is_finalize=False):
        calls.append({"cid": cid, "content": content, "finalize": is_finalize})

    client._put_streaming = spy_put
    _run(client.finalize_card("card1", "final text"))

    assert len(calls) == 1
    assert calls[0]["finalize"] is True
    assert calls[0]["content"] == "final text"


def test_finalize_cancels_pending_throttle(client):
    """finalize_card should cancel any pending throttled update."""
    calls = []

    async def spy_put(cid, content, *, is_finalize=False):
        calls.append({"content": content, "finalize": is_finalize})

    client._put_streaming = spy_put

    async def scenario():
        await client.update_card("card1", "v1")
        await client.update_card("card1", "v2-buffered")
        await client.finalize_card("card1", "FINAL")
        await asyncio.sleep(THROTTLE_INTERVAL_S + 0.1)

        assert len(calls) == 2
        assert calls[0]["content"] == "v1"
        assert calls[1]["content"] == "FINAL"
        assert calls[1]["finalize"] is True

    _run(scenario())


# ── _put_streaming payload ───────────────────────────────────────────


def test_put_streaming_payload(client):
    """_put_streaming should build correct payload for DingTalk API."""
    captured = {}

    async def spy_api(method, endpoint, body, **kw):
        captured.update({"method": method, "endpoint": endpoint, "body": body})
        return {}

    client._api = spy_api
    _run(client._put_streaming("card123", "hello world", is_finalize=False))

    assert captured["method"] == "PUT"
    assert captured["endpoint"] == "/v1.0/card/streaming"
    body = captured["body"]
    assert body["outTrackId"] == "card123"
    assert body["key"] == "msgContent"
    assert body["content"] == "hello world"
    assert body["isFull"] is True
    assert body["isFinalize"] is False
    assert body["isError"] is False


# ── _api error handling ──────────────────────────────────────────────


def test_api_no_token_returns_none():
    """If get_access_token returns empty string, _api returns None."""
    c = DingTalkAICardClient(lambda: "", robot_code="r")
    result = _run(c._api("PUT", "/test", {}))
    assert result is None


def test_api_http_error_returns_none(client):
    """HTTP errors should log warning and return None."""
    resp = _mock_response(403, {"message": "forbidden"})
    with _patch_aiohttp(resp):
        result = _run(client._api("PUT", "/v1.0/card/streaming", {"x": 1}))
    assert result is None


def test_api_network_error_returns_none(client):
    """Network exceptions should not propagate — return None."""
    with patch(
        "cccc.ports.im.adapters.dingtalk_card.aiohttp.ClientSession",
        side_effect=OSError("connection refused"),
    ):
        result = _run(client._api("PUT", "/v1.0/card/streaming", {"x": 1}))
    assert result is None


# ── DingTalkCardHandle ───────────────────────────────────────────────


def test_handle_update_calls_client(client):
    """DingTalkCardHandle.update should delegate to client.update_card."""
    calls = []

    async def spy_update(cid, content, *, seq=0):
        calls.append({"cid": cid, "content": content, "seq": seq})

    client.update_card = spy_update
    handle = DingTalkCardHandle(client, "card42", stream_id="s1")

    async def scenario():
        await handle.update("chunk A")
        await handle.update("chunk B")

    _run(scenario())

    assert len(calls) == 2
    assert calls[0] == {"cid": "card42", "content": "chunk A", "seq": 1}
    assert calls[1] == {"cid": "card42", "content": "chunk B", "seq": 2}


def test_handle_close_calls_finalize(client):
    """DingTalkCardHandle.close should delegate to client.finalize_card."""
    calls = []

    async def spy_finalize(cid, content):
        calls.append({"cid": cid, "content": content})

    client.finalize_card = spy_finalize
    handle = DingTalkCardHandle(client, "card42")

    _run(handle.close("done!"))
    assert calls == [{"cid": "card42", "content": "done!"}]


def test_handle_as_handle_returns_typed_dict(client):
    """as_handle should return OutboundStreamHandle TypedDict."""
    handle = DingTalkCardHandle(client, "card99", stream_id="stream-1")
    d = handle.as_handle()
    assert d["stream_id"] == "stream-1"
    assert d["platform_handle"] == "card99"


def test_handle_properties(client):
    """card_instance_id and stream_id properties."""
    handle = DingTalkCardHandle(client, "c1", stream_id="s1")
    assert handle.card_instance_id == "c1"
    assert handle.stream_id == "s1"
