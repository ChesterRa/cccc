from unittest.mock import patch

from cccc.daemon.messaging.chat_ops import _build_headless_delivery_text
from cccc.daemon.messaging.actor_turn_rendering import (
    build_actor_delivery_text,
    render_actor_event_batch_for_delivery,
    render_actor_event_for_delivery,
    render_group_bridge_route_ref,
)
from cccc.daemon.messaging.delivery import PendingMessage, render_single_message
from cccc.daemon.messaging.inbound_rendering import ActorInboundEnvelope, render_actor_inbound_message


def test_inbound_renderer_plain_send_matches_pty_and_headless_wrappers() -> None:
    expected = "[cccc] user → peer1 [event_id=evt-1 message_mode=send]: hello"

    assert render_actor_inbound_message(
        ActorInboundEnvelope(event_id="evt-1", by="user", to=["peer1"], text="hello")
    ) == expected
    assert render_single_message(
        PendingMessage(event_id="evt-1", by="user", to=["peer1"], text="hello")
    ) == expected
    assert _build_headless_delivery_text(
        event_id="evt-1", message_mode="send", by="user", to=["peer1"], body="hello"
    ) == expected


def test_direct_delivery_adds_mail_count_without_consuming_mail() -> None:
    direct = {
        "id": "direct-1",
        "kind": "chat.message",
        "by": "user",
        "data": {
            "to": ["peer1"],
            "text": "look now",
            "message_mode": "send",
        },
    }
    mail = {
        "id": "mail-1",
        "kind": "chat.message",
        "by": "user",
        "data": {
            "to": ["peer1"],
            "text": "read later",
            "message_mode": "mail",
        },
    }
    with patch(
        "cccc.daemon.messaging.actor_turn_rendering.mail_pending_summary",
        return_value={"count": 2},
    ) as pending:
        rendered = render_actor_event_batch_for_delivery(
            [direct],
            actor_id="peer1",
            group=object(),
        )
        mail_rendered = render_actor_event_batch_for_delivery(
            [mail],
            actor_id="peer1",
            group=object(),
        )

    assert "MAIL PENDING: 2 items" in rendered
    assert "MAIL PENDING" not in mail_rendered
    pending.assert_called_once()


def test_inbound_renderer_preserves_reply_quote_semantics() -> None:
    expected = (
        "[cccc] peer2 → peer1 (reply:abcdef12) "
        '[event_id=evt-2 message_mode=send reply_to=abcdef123456]\n> "外部用户原话": 收到，我来处理。'
    )

    assert render_actor_inbound_message(
        ActorInboundEnvelope(
            event_id="evt-2",
            by="peer2",
            to=["peer1"],
            text="收到，我来处理。",
            reply_to="abcdef123456",
            quote_text="外部用户原话",
        )
    ) == expected
    assert render_single_message(
        PendingMessage(
            event_id="evt-2",
            by="peer2",
            to=["peer1"],
            text="收到，我来处理。",
            reply_to="abcdef123456",
            quote_text="外部用户原话",
        )
    ) == expected
    assert _build_headless_delivery_text(
        event_id="evt-2",
        message_mode="send",
        by="peer2",
        to=["peer1"],
        body="收到，我来处理。",
        reply_to="abcdef123456",
        quote_text="外部用户原话",
    ) == expected


def test_inbound_renderer_preserves_external_source_semantics() -> None:
    expected = (
        "[cccc] user[dingtalk / Alice / 1729] → peer1 "
        "[event_id=evt-3 message_mode=send]: 外部消息"
    )

    assert render_actor_inbound_message(
        ActorInboundEnvelope(
            event_id="evt-3",
            by="user",
            to=["peer1"],
            text="外部消息",
            source_platform="dingtalk",
            source_user_name="Alice",
            source_user_id="1729",
        )
    ) == expected
    assert render_single_message(
        PendingMessage(
            event_id="evt-3",
            by="user",
            to=["peer1"],
            text="外部消息",
            source_platform="dingtalk",
            source_user_name="Alice",
            source_user_id="1729",
        )
    ) == expected
    assert _build_headless_delivery_text(
        event_id="evt-3",
        message_mode="send",
        by="user",
        to=["peer1"],
        body="外部消息",
        source_platform="dingtalk",
        source_user_name="Alice",
        source_user_id="1729",
    ) == expected


def test_inbound_renderer_preserves_multiline_body() -> None:
    expected = (
        "[cccc] user → peer1 [event_id=evt-4 message_mode=send]:\nline one\nline two"
    )

    assert render_actor_inbound_message(
        ActorInboundEnvelope(event_id="evt-4", by="user", to=["peer1"], text="line one\nline two")
    ) == expected
    assert render_single_message(
        PendingMessage(event_id="evt-4", by="user", to=["peer1"], text="line one\nline two")
    ) == expected
    assert _build_headless_delivery_text(
        event_id="evt-4",
        message_mode="send",
        by="user",
        to=["peer1"],
        body="line one\nline two",
    ) == expected


def test_request_reply_delivery_exposes_current_event_identity_and_requirement() -> None:
    rendered = render_actor_event_for_delivery(
        {
            "id": "evt-reply-required",
            "kind": "chat.message",
            "by": "user",
            "data": {
                "to": ["peer1"],
                "text": "please answer",
                "message_mode": "request_reply",
            },
        },
        actor_id="peer1",
    )

    assert rendered.startswith(
        "[cccc] user → peer1 "
        "[event_id=evt-reply-required message_mode=request_reply]:\n"
    )
    assert "REPLY REQUIRED (event_id=evt-reply-required): reply via cccc_message_reply." in rendered


def test_actor_delivery_text_points_attachments_to_file_read_tools() -> None:
    text = build_actor_delivery_text(
        text="inspect attachment",
        message_mode="send",
        event_id="evt-1",
        refs=[],
        attachments=[
            {
                "title": "notes.txt",
                "bytes": 12,
                "path": "state/blobs/sha256_notes.txt",
            }
        ],
    )

    assert 'cccc_file(action="read", rel_path=...)' in text
    assert 'action="blob_path"' in text
    assert "binary/local tools" in text
    assert "- notes.txt (12 bytes) [state/blobs/sha256_notes.txt]" in text


def test_actor_delivery_text_renders_group_bridge_route_refs() -> None:
    text = build_actor_delivery_text(
        text="please send to #Remote Product",
        message_mode="send",
        event_id="evt-1",
        refs=[
            {
                "kind": "group_bridge_route",
                "remote_group_id": "g_remote",
                "remote_group_title": "Remote Product",
                "remote_endpoint": "https://remote.example",
                "remote_peer_id": "peer_remote",
                "trust_id": "ptrust_1",
                "access_level": "read",
                "recipient_identifier": "Remote Product (g_remote remote/read)",
                "token": "#Remote Product",
            }
        ],
        attachments=[],
    )

    assert "- Group Bridge route Remote Product (g_remote remote/read)" in text
    assert "endpoint: https://remote.example" not in text
    assert "peer_id: peer_remote" not in text
    assert "trust_id: ptrust_1" not in text


def test_actor_delivery_text_renders_local_group_route_as_ai_owned_context() -> None:
    text = build_actor_delivery_text(
        text="请 #Self Agent 主动打个招呼",
        message_mode="send",
        event_id="evt-1",
        refs=[
            {
                "kind": "local_group_route",
                "group_id": "g_self_agent",
                "group_title": "Self Agent",
                "token": "#Self Agent",
            }
        ],
        attachments=[],
    )

    assert "Local group route Self Agent (group_id=g_self_agent)" in text
    assert "this is context, not an automatic send" in text
    assert "your own natural message" in text
    assert "Do not forward the user's text or a template" in text


def test_actor_delivery_text_does_not_render_hidden_slash_control_refs() -> None:
    text = build_actor_delivery_text(
        text="[CCCC] INTERNAL CONTROL: CCCC capability skill dispatch",
        message_mode="send",
        event_id="evt-1",
        refs=[
            {
                "kind": "text",
                "title": "slash_skill_dispatch",
                "hidden": True,
                "control_kind": "slash_skill_dispatch",
                "command": "/using-superpowers",
                "capability_id": "skill:agent_self_proposed:using-superpowers",
                "task_text": "开始执行",
            }
        ],
        attachments=[],
    )

    assert "[CCCC] INTERNAL CONTROL" in text
    assert "[cccc] References:" not in text
    assert "slash_skill_dispatch" not in text


def test_group_bridge_route_ref_renderer_preserves_route_id_with_long_label() -> None:
    long_label = "Remote Product " + ("Very Long " * 12)

    lines = render_group_bridge_route_ref(
        {
            "kind": "group_bridge_route",
            "remote_group_id": "g_remote_stable",
            "remote_group_title": long_label,
            "access_level": "full",
            "recipient_identifier": f"{long_label} (g_remote_stable remote/full)",
        }
    )

    assert len(lines) == 1
    assert lines[0].endswith("(g_remote_stable remote/full)")
    assert "…" in lines[0]


def test_group_bridge_route_ref_renderer_ignores_refs_without_group_id() -> None:
    assert render_group_bridge_route_ref({"kind": "group_bridge_route", "remote_group_title": "Remote"}) == []
