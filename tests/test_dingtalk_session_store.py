"""Tests for DingTalk conversation reply state."""

from __future__ import annotations

import time
from pathlib import Path

from cccc.ports.im.adapters.dingtalk_session import DingTalkConversationStore


def test_store_remembers_p2p_route_and_unexpired_webhook(tmp_path: Path) -> None:
    state_path = tmp_path / "sessions.json"
    store = DingTalkConversationStore(state_path)
    expires_at = time.time() + 3600

    store.remember(
        "cidP2P",
        chat_type="p2p",
        user_id="staff_001",
        session_webhook="https://webhook.example/p2p",
        session_expires=expires_at,
    )

    restored = DingTalkConversationStore(state_path)

    assert restored.chat_type("cidP2P") == "p2p"
    assert restored.user_id("cidP2P") == "staff_001"
    assert restored.is_group("cidP2P") is False
    assert restored.live_webhook("cidP2P") == "https://webhook.example/p2p"


def test_store_drops_expired_webhook_but_keeps_route(tmp_path: Path) -> None:
    state_path = tmp_path / "sessions.json"
    store = DingTalkConversationStore(state_path)

    store.remember(
        "cidP2P",
        chat_type="p2p",
        user_id="staff_001",
        session_webhook="https://webhook.example/expired",
        session_expires=time.time() - 1,
    )

    restored = DingTalkConversationStore(state_path)

    assert restored.chat_type("cidP2P") == "p2p"
    assert restored.user_id("cidP2P") == "staff_001"
    assert restored.live_webhook("cidP2P") is None


def test_store_uses_cid_prefix_only_when_route_is_unknown(tmp_path: Path) -> None:
    store = DingTalkConversationStore(tmp_path / "sessions.json")

    assert store.is_group("cidUnknown") is True
    store.remember("cidKnownP2P", chat_type="p2p", user_id="staff_001")

    assert store.is_group("cidKnownP2P") is False


def test_store_does_not_replace_known_route_with_unknown(tmp_path: Path) -> None:
    state_path = tmp_path / "sessions.json"
    store = DingTalkConversationStore(state_path)
    store.remember("cidKnownP2P", chat_type="p2p", user_id="staff_001")

    store.remember("cidKnownP2P", chat_type="unknown", user_id="staff_002")

    restored = DingTalkConversationStore(state_path)
    assert restored.chat_type("cidKnownP2P") == "p2p"
    assert restored.user_id("cidKnownP2P") == "staff_002"
    assert restored.is_group("cidKnownP2P") is False
