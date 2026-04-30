from cccc.daemon.messaging.chat_ops import _build_headless_delivery_text
from cccc.daemon.messaging.actor_turn_rendering import build_actor_delivery_text
from cccc.daemon.messaging.delivery import PendingMessage, render_single_message
from cccc.daemon.messaging.inbound_rendering import ActorInboundEnvelope, render_actor_inbound_message


def test_inbound_renderer_plain_send_matches_pty_and_headless_wrappers() -> None:
    expected = "[cccc] user → peer1: hello"

    assert render_actor_inbound_message(
        ActorInboundEnvelope(by="user", to=["peer1"], text="hello")
    ) == expected
    assert render_single_message(
        PendingMessage(event_id="evt-1", by="user", to=["peer1"], text="hello")
    ) == expected
    assert _build_headless_delivery_text(by="user", to=["peer1"], body="hello") == expected


def test_inbound_renderer_preserves_reply_quote_semantics() -> None:
    expected = '[cccc] peer2 → peer1 (reply:abcdef12)\n> "外部用户原话": 收到，我来处理。'

    assert render_actor_inbound_message(
        ActorInboundEnvelope(
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
        by="peer2",
        to=["peer1"],
        body="收到，我来处理。",
        reply_to="abcdef123456",
        quote_text="外部用户原话",
    ) == expected


def test_inbound_renderer_preserves_external_source_semantics() -> None:
    expected = "[cccc] user[dingtalk / Alice / 1729] → peer1: 外部消息"

    assert render_actor_inbound_message(
        ActorInboundEnvelope(
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
        by="user",
        to=["peer1"],
        body="外部消息",
        source_platform="dingtalk",
        source_user_name="Alice",
        source_user_id="1729",
    ) == expected


def test_inbound_renderer_preserves_multiline_body() -> None:
    expected = "[cccc] user → peer1:\nline one\nline two"

    assert render_actor_inbound_message(
        ActorInboundEnvelope(by="user", to=["peer1"], text="line one\nline two")
    ) == expected
    assert render_single_message(
        PendingMessage(event_id="evt-4", by="user", to=["peer1"], text="line one\nline two")
    ) == expected
    assert _build_headless_delivery_text(
        by="user",
        to=["peer1"],
        body="line one\nline two",
    ) == expected


def test_actor_delivery_text_points_attachments_to_file_read_tools() -> None:
    text = build_actor_delivery_text(
        text="inspect attachment",
        priority="normal",
        reply_required=False,
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
    assert "- notes.txt (12 bytes) [state/blobs/sha256_notes.txt]" in text
