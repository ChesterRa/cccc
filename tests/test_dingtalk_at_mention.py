"""Tests for DingTalk @mention (real push) in outbound messages."""

from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Any, Dict, List, Optional
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from cccc.ports.im.adapters.dingtalk import DINGTALK_MAX_MESSAGE_LENGTH, DingTalkAdapter


# ── fixtures ─────────────────────────────────────────────────────────


@pytest.fixture
def adapter(tmp_path: Path) -> DingTalkAdapter:
    a = DingTalkAdapter(
        app_key="test_key",
        app_secret="test_secret",
        robot_code="test_robot",
        log_path=tmp_path / "test.log",
    )
    a._connected = True
    return a


def _make_inbound_event(
    conversation_id: str = "cidXXXgroup123",
    sender_staff_id: str = "staff_001",
    sender_id: Optional[str] = None,
    sender_nick: str = "Alice",
    text: str = "hello",
    conversation_type: str = "2",  # group
    msg_id: str = "msg_001",
) -> Dict[str, Any]:
    return {
        "msgtype": "text",
        "conversationId": conversation_id,
        "senderStaffId": sender_staff_id,
        "senderId": sender_id if sender_id is not None else f"fallback_{sender_staff_id}",
        "senderNick": sender_nick,
        "conversationType": conversation_type,
        "conversationTitle": "Test Chat",
        "msgId": msg_id,
        "text": {"content": text},
        "sessionWebhook": f"https://oapi.dingtalk.com/robot/sendBySession/{conversation_id}",
        "sessionWebhookExpiredTime": int((time.time() + 3600) * 1000),
        "createAt": int(time.time() * 1000),
    }


# ── Test: from_user_id populated in normalized message ───────────────


class TestFromUserIdNormalized:
    def test_enqueue_populates_from_user_id(self, adapter: DingTalkAdapter) -> None:
        event = _make_inbound_event(sender_staff_id="staff_abc")
        adapter._enqueue_message(event)

        msgs = adapter.poll()
        assert len(msgs) == 1
        assert msgs[0]["from_user_id"] == "staff_abc"
        assert msgs[0]["mention_user_ids"] == ["staff_abc"]

    def test_enqueue_from_user_id_fallback_to_sender_id(self, adapter: DingTalkAdapter) -> None:
        event = _make_inbound_event(sender_staff_id="", sender_id="union_001")
        # senderId is "fallback_" prefix
        adapter._enqueue_message(event)

        msgs = adapter.poll()
        assert len(msgs) == 1
        assert msgs[0]["from_user_id"] == "union_001"
        assert msgs[0]["mention_user_ids"] == []


# ── Test: _last_sender cache ─────────────────────────────────────────


class TestLastSenderCache:
    def test_cache_populated_on_enqueue(self, adapter: DingTalkAdapter) -> None:
        event = _make_inbound_event(
            conversation_id="cidGroup1",
            sender_staff_id="staff_001",
            sender_nick="Alice",
        )
        adapter._enqueue_message(event)

        assert "cidGroup1" in adapter._last_sender
        staff_id, nick = adapter._last_sender["cidGroup1"]
        assert staff_id == "staff_001"
        assert nick == "Alice"

    def test_cache_updated_on_new_sender(self, adapter: DingTalkAdapter) -> None:
        e1 = _make_inbound_event(
            conversation_id="cidGroup1",
            sender_staff_id="staff_001",
            sender_nick="Alice",
            msg_id="msg_001",
        )
        e2 = _make_inbound_event(
            conversation_id="cidGroup1",
            sender_staff_id="staff_002",
            sender_nick="Bob",
            msg_id="msg_002",
        )
        adapter._enqueue_message(e1)
        adapter._enqueue_message(e2)

        staff_id, nick = adapter._last_sender["cidGroup1"]
        assert staff_id == "staff_002"
        assert nick == "Bob"

    def test_sender_id_only_does_not_pollute_mention_cache(self, adapter: DingTalkAdapter) -> None:
        event = _make_inbound_event(
            conversation_id="cidGroup1",
            sender_staff_id="",
            sender_id="union_only",
            sender_nick="NoStaff",
        )
        adapter._enqueue_message(event)

        assert "cidGroup1" not in adapter._last_sender

    def test_cache_bounded(self, adapter: DingTalkAdapter) -> None:
        # Fill beyond limit
        for i in range(adapter._LAST_SENDER_MAX + 10):
            event = _make_inbound_event(
                conversation_id=f"cidGroup{i}",
                sender_staff_id=f"staff_{i}",
                msg_id=f"msg_{i}",
            )
            adapter._enqueue_message(event)

        assert len(adapter._last_sender) <= adapter._LAST_SENDER_MAX


# ── Test: webhook sends at field for group chats ─────────────────────


class TestWebhookAtMention:
    def test_webhook_includes_at_for_group(self, adapter: DingTalkAdapter) -> None:
        """Webhook body should include at.atUserIds when at_user_ids provided."""
        captured: List[bytes] = []

        def mock_urlopen(req, timeout=None):
            captured.append(req.data)
            resp = MagicMock()
            resp.read.return_value = json.dumps({"errcode": 0}).encode()
            resp.__enter__ = lambda s: resp
            resp.__exit__ = lambda s, *a: None
            return resp

        with patch("cccc.ports.im.adapters.dingtalk.urllib.request.urlopen", side_effect=mock_urlopen):
            ok = adapter._send_via_webhook(
                "https://webhook.example.com",
                "Hello group",
                at_user_ids=["staff_001"],
            )

        assert ok is True
        assert len(captured) == 1
        body = json.loads(captured[0])
        assert "at" in body
        assert body["at"]["atUserIds"] == ["staff_001"]

    def test_webhook_no_at_when_none(self, adapter: DingTalkAdapter) -> None:
        """Webhook body should NOT include at field when at_user_ids is None."""
        captured: List[bytes] = []

        def mock_urlopen(req, timeout=None):
            captured.append(req.data)
            resp = MagicMock()
            resp.read.return_value = json.dumps({"errcode": 0}).encode()
            resp.__enter__ = lambda s: resp
            resp.__exit__ = lambda s, *a: None
            return resp

        with patch("cccc.ports.im.adapters.dingtalk.urllib.request.urlopen", side_effect=mock_urlopen):
            ok = adapter._send_via_webhook(
                "https://webhook.example.com",
                "Hello group",
            )

        assert ok is True
        body = json.loads(captured[0])
        assert "at" not in body


# ── Test: send_message end-to-end at resolution ──────────────────────


class TestSendMessageAtResolution:
    def test_group_message_resolves_at(self, adapter: DingTalkAdapter) -> None:
        """send_message for group chat should resolve cached sender and pass at_user_ids."""
        chat_id = "cidGroupABC"
        # Simulate inbound to populate cache
        event = _make_inbound_event(
            conversation_id=chat_id,
            sender_staff_id="staff_xyz",
            sender_nick="Charlie",
        )
        adapter._enqueue_message(event)
        adapter.poll()  # drain queue

        captured_at: List[Optional[List[str]]] = []
        original_send = adapter._send_via_webhook.__func__  # type: ignore[attr-defined]

        def mock_webhook(self_adapter, url, text, at_user_ids=None):
            captured_at.append(at_user_ids)
            return True

        with patch.object(type(adapter), "_send_via_webhook", mock_webhook):
            adapter.send_message(chat_id, "Reply to Charlie")

        assert len(captured_at) == 1
        assert captured_at[0] == ["staff_xyz"]

    def test_p2p_message_no_at(self, adapter: DingTalkAdapter) -> None:
        """send_message for 1:1 chat should NOT include at_user_ids."""
        chat_id = "user123"  # not starting with "cid"
        # Populate sender cache with a non-group chat_id
        adapter._last_sender[chat_id] = ("staff_999", "Dave")

        captured_at: List[Optional[List[str]]] = []

        def mock_webhook(self_adapter, url, text, at_user_ids=None):
            captured_at.append(at_user_ids)
            return True

        # Set up webhook cache for this chat
        adapter._session_webhook_cache[chat_id] = (
            "https://webhook.example.com",
            time.time() + 3600,
        )

        with patch.object(type(adapter), "_send_via_webhook", mock_webhook):
            adapter.send_message(chat_id, "Reply to Dave")

        assert len(captured_at) == 1
        assert captured_at[0] is None  # no at for 1:1

    def test_group_no_cached_sender_no_at(self, adapter: DingTalkAdapter) -> None:
        """send_message for group without cached sender should send without at."""
        chat_id = "cidGroupNoSender"
        adapter._session_webhook_cache[chat_id] = (
            "https://webhook.example.com",
            time.time() + 3600,
        )

        captured_at: List[Optional[List[str]]] = []

        def mock_webhook(self_adapter, url, text, at_user_ids=None):
            captured_at.append(at_user_ids)
            return True

        with patch.object(type(adapter), "_send_via_webhook", mock_webhook):
            adapter.send_message(chat_id, "Hello group")

        assert len(captured_at) == 1
        assert captured_at[0] is None

    def test_group_sender_id_only_does_not_reuse_for_at(self, adapter: DingTalkAdapter) -> None:
        """senderId fallback is display-only and must not be reused for real @mention."""
        chat_id = "cidGroupUnionOnly"
        event = _make_inbound_event(
            conversation_id=chat_id,
            sender_staff_id="",
            sender_id="union_only",
            sender_nick="UnionUser",
        )
        adapter._enqueue_message(event)
        adapter.poll()

        captured_at: List[Optional[List[str]]] = []

        def mock_webhook(self_adapter, url, text, at_user_ids=None):
            captured_at.append(at_user_ids)
            return True

        with patch.object(type(adapter), "_send_via_webhook", mock_webhook):
            adapter.send_message(chat_id, "Reply to UnionUser")

        assert len(captured_at) == 1
        assert captured_at[0] is None

    def test_new_api_fallback_includes_at(self, adapter: DingTalkAdapter) -> None:
        """Robot API fallback should carry at.atUserIds in msgParam."""
        captured: Dict[str, Any] = {}
        adapter._session_webhook_cache.clear()

        def mock_api_new(method, endpoint, body, timeout=15):
            captured["method"] = method
            captured["endpoint"] = endpoint
            captured["body"] = body
            return {"processQueryKey": "ok"}

        with patch.object(adapter, "_api_new", side_effect=mock_api_new):
            ok = adapter.send_message(
                "cidGroupApi",
                "Hello group",
                mention_user_ids=["staff_001"],
            )

        assert ok is True
        assert captured["endpoint"] == "/v1.0/robot/groupMessages/send"
        payload = json.loads(captured["body"]["msgParam"])
        assert payload["at"]["atUserIds"] == ["staff_001"]

    def test_p2p_cid_conversation_fallback_uses_oto_api(self, adapter: DingTalkAdapter) -> None:
        """DingTalk 1:1 conversationIds can start with cid and must not use group API."""
        captured: Dict[str, Any] = {}
        chat_id = "cidP2PFromDingTalk"
        event = _make_inbound_event(
            conversation_id=chat_id,
            sender_staff_id="staff_p2p",
            sender_nick="Shirley",
            conversation_type="1",
        )
        adapter._enqueue_message(event)
        adapter.poll()
        adapter._session_webhook_cache.clear()

        def mock_api_new(method, endpoint, body, timeout=15):
            captured["method"] = method
            captured["endpoint"] = endpoint
            captured["body"] = body
            return {"processQueryKey": "ok"}

        with patch.object(adapter, "_api_new", side_effect=mock_api_new):
            ok = adapter.send_message(chat_id, "Hello p2p")

        assert ok is True
        assert captured["endpoint"] == "/v1.0/robot/oToMessages/batchSend"
        assert captured["body"]["userIds"] == ["staff_p2p"]
        assert "openConversationId" not in captured["body"]

    def test_group_cid_conversation_fallback_uses_group_api(self, adapter: DingTalkAdapter) -> None:
        """DingTalk group conversations still use groupMessages fallback."""
        captured: Dict[str, Any] = {}
        chat_id = "cidGroupApi"
        event = _make_inbound_event(
            conversation_id=chat_id,
            sender_staff_id="staff_group",
            conversation_type="2",
        )
        adapter._enqueue_message(event)
        adapter.poll()
        adapter._session_webhook_cache.clear()

        def mock_api_new(method, endpoint, body, timeout=15):
            captured["method"] = method
            captured["endpoint"] = endpoint
            captured["body"] = body
            return {"processQueryKey": "ok"}

        with patch.object(adapter, "_api_new", side_effect=mock_api_new):
            ok = adapter.send_message(chat_id, "Hello group")

        assert ok is True
        assert captured["endpoint"] == "/v1.0/robot/groupMessages/send"
        assert captured["body"]["openConversationId"] == chat_id
        assert "userIds" not in captured["body"]

    def test_restart_restores_session_webhook_and_p2p_route(self, tmp_path: Path) -> None:
        """A restarted adapter should keep unexpired webhook and p2p routing state."""
        state_path = tmp_path / "dingtalk_sessions.json"
        chat_id = "cidP2PRestart"
        first = DingTalkAdapter(
            app_key="test_key",
            app_secret="test_secret",
            robot_code="test_robot",
            log_path=tmp_path / "first.log",
            session_state_path=state_path,
        )
        first._connected = True
        event = _make_inbound_event(
            conversation_id=chat_id,
            sender_staff_id="staff_restart",
            conversation_type="1",
        )
        first._enqueue_message(event)
        first.poll()

        second = DingTalkAdapter(
            app_key="test_key",
            app_secret="test_secret",
            robot_code="test_robot",
            log_path=tmp_path / "second.log",
            session_state_path=state_path,
        )
        second._connected = True

        captured_webhook: List[str] = []

        def mock_webhook(self_adapter, url, text, at_user_ids=None):
            _ = self_adapter
            _ = text
            _ = at_user_ids
            captured_webhook.append(url)
            return True

        with patch.object(type(second), "_send_via_webhook", mock_webhook):
            ok = second.send_message(chat_id, "after restart")

        assert ok is True
        assert captured_webhook == [f"https://oapi.dingtalk.com/robot/sendBySession/{chat_id}"]
        assert second._conversation_chat_type(chat_id) == "p2p"
        assert second._conversation_user_id(chat_id) == "staff_restart"

    def test_legacy_api_fallback_includes_at(self, adapter: DingTalkAdapter) -> None:
        """Legacy /chat/send fallback should carry msg.at.atUserIds."""
        captured: Dict[str, Any] = {}
        adapter._session_webhook_cache.clear()
        adapter.robot_code = ""

        def mock_api_old(method, endpoint, body, timeout=15):
            captured["method"] = method
            captured["endpoint"] = endpoint
            captured["body"] = body
            return {"errcode": 0}

        with patch.object(adapter, "_api_old", side_effect=mock_api_old):
            ok = adapter.send_message(
                "cidGroupLegacy",
                "Hello legacy",
                mention_user_ids=["staff_001"],
            )

        assert ok is True
        assert captured["endpoint"] == "/chat/send"
        assert captured["body"]["msg"]["at"]["atUserIds"] == ["staff_001"]

    def test_long_message_is_split_instead_of_truncated(self, adapter: DingTalkAdapter) -> None:
        """Long outbound text should be split into multiple webhook sends."""
        chat_id = "cidGroupSplit"
        adapter._session_webhook_cache[chat_id] = (
            "https://webhook.example.com",
            time.time() + 3600,
        )
        adapter.max_chars = DINGTALK_MAX_MESSAGE_LENGTH
        long_text = ("A" * DINGTALK_MAX_MESSAGE_LENGTH) + ("B" * 32)
        captured_texts: List[str] = []
        captured_at: List[Optional[List[str]]] = []

        def mock_webhook(self_adapter, url, text, at_user_ids=None):
            _ = self_adapter
            _ = url
            captured_texts.append(text)
            captured_at.append(at_user_ids)
            return True

        with patch.object(type(adapter), "_send_via_webhook", mock_webhook):
            ok = adapter.send_message(chat_id, long_text, mention_user_ids=["staff_001"])

        assert ok is True
        assert len(captured_texts) == 2
        assert captured_texts[0] == "A" * DINGTALK_MAX_MESSAGE_LENGTH
        assert captured_texts[1] == "B" * 32
        assert captured_at == [["staff_001"], None]

    def test_format_outbound_keeps_full_text_for_chunking(self, adapter: DingTalkAdapter) -> None:
        """format_outbound must not truncate before send_message can split."""
        long_text = "x" * (DINGTALK_MAX_MESSAGE_LENGTH + 64)

        formatted = adapter.format_outbound("foreman", ["user"], long_text)

        assert formatted.startswith("[foreman] ")
        assert len(formatted) > DINGTALK_MAX_MESSAGE_LENGTH
        assert not formatted.endswith("...")


class TestStreamMessageSafety:
    def test_streaming_paths_reuse_safe_text_preparation(self, adapter: DingTalkAdapter) -> None:
        """AI Card start/update/end should reuse the same safe text envelope."""
        fake_client = MagicMock()
        fake_client.create_card = AsyncMock(return_value="card_123")
        fake_client.update_card = AsyncMock(return_value=None)
        fake_client.finalize_card = AsyncMock(return_value=None)

        with patch.object(adapter, "_get_card_client", return_value=fake_client), patch.object(
            adapter,
            "_prepare_stream_text",
            side_effect=lambda value: f"SAFE::{value}",
        ) as safe_mock:
            handle = adapter.begin_stream("cidGroupStream", "stream-1", text="raw start")
            assert handle is not None

            ok_update = adapter.update_stream(handle, text="raw update", seq=7)
            ok_end = adapter.end_stream(handle, text="raw final")

        assert ok_update is True
        assert ok_end is True
        assert safe_mock.call_args_list[0].args == ("raw start",)
        assert safe_mock.call_args_list[1].args == ("raw update",)
        assert safe_mock.call_args_list[2].args == ("raw final",)
        fake_client.create_card.assert_awaited_once_with("cidGroupStream", "SAFE::raw start")
        fake_client.update_card.assert_awaited_once_with("card_123", "SAFE::raw update", seq=7)
        fake_client.finalize_card.assert_awaited_once_with("card_123", "SAFE::raw final")
