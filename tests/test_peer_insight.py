from __future__ import annotations

from contextlib import contextmanager
from pathlib import Path
from unittest.mock import patch

import pytest


@pytest.fixture()
def peer_group(tmp_path: Path, monkeypatch: pytest.MonkeyPatch):
    monkeypatch.setenv("CCCC_HOME", str(tmp_path / "home"))
    from cccc.kernel.actors import add_actor
    from cccc.kernel.group import create_group
    from cccc.kernel.registry import load_registry

    group = create_group(load_registry(), title="peer-insight")
    add_actor(group, actor_id="peer1", title="Peer 1", runner="headless", enabled=True)
    add_actor(group, actor_id="peer2", title="Peer 2", runner="headless", enabled=True)
    return group


@contextmanager
def _quiet_chat_commit():
    with (
        patch("cccc.daemon.messaging.chat_ops.run_group_chat_post_commit"),
        patch("cccc.daemon.messaging.chat_ops.schedule_chat_side_effects"),
    ):
        yield


def _handle_send(group, args: dict, *, wakes: list[tuple[list[str], str]] | None = None):
    from cccc.daemon.messaging.chat_ops import handle_send
    from cccc.util.conv import coerce_bool

    def auto_wake(_group, to, by):
        if wakes is not None:
            wakes.append((list(to), str(by)))
        return []

    with _quiet_chat_commit():
        return handle_send(
            {"group_id": group.group_id, **args},
            coerce_bool=coerce_bool,
            normalize_attachments=lambda _group, raw: list(raw or []),
            effective_runner_kind=lambda value: value,
            auto_wake_recipients=auto_wake,
            automation_on_resume=lambda _group: None,
            automation_on_new_message=lambda _group: None,
            clear_pending_system_notifies=lambda _group_id, _kinds: None,
        )


def _handle_reply(group, args: dict):
    from cccc.daemon.messaging.chat_ops import handle_reply
    from cccc.util.conv import coerce_bool

    with _quiet_chat_commit():
        return handle_reply(
            {"group_id": group.group_id, **args},
            coerce_bool=coerce_bool,
            normalize_attachments=lambda _group, raw: list(raw or []),
            effective_runner_kind=lambda value: value,
            auto_wake_recipients=lambda _group, _to, _by: [],
            automation_on_resume=lambda _group: None,
            automation_on_new_message=lambda _group: None,
            clear_pending_system_notifies=lambda _group_id, _kinds: None,
        )


def _configure_file_scope(group, tmp_path: Path) -> str:
    scope_root = tmp_path / "scope"
    scope_root.mkdir()
    source = scope_root / "report.txt"
    source.write_text("report", encoding="utf-8")
    group.doc["active_scope_key"] = "scope-1"
    group.doc["scopes"] = [{"scope_key": "scope-1", "url": str(scope_root)}]
    group.save()
    return source.name


def test_insight_contract_normalizes_and_bounds_plain_text() -> None:
    from pydantic import ValidationError

    from cccc.contracts.v1 import INSIGHT_MAX_CHARS, ChatMessageData, normalize_insight

    assert normalize_insight(None) is None
    assert normalize_insight("  provisional view  ") == "provisional view"
    assert normalize_insight("   ") is None
    assert ChatMessageData(text="body", insight="  view  ").insight == "view"
    with pytest.raises(ValueError, match="string"):
        normalize_insight(42)
    with pytest.raises(ValidationError):
        ChatMessageData(text="body", insight="x" * (INSIGHT_MAX_CHARS + 1))


@pytest.mark.parametrize(
    ("by", "to"),
    [
        ("peer1", ["peer2"]),
        ("peer1", ["user", "peer2"]),
        ("peer1", ["@all"]),
        ("peer2", []),
    ],
)
def test_strict_peer_send_requires_insight_for_real_peer_audiences(peer_group, by: str, to: list[str]) -> None:
    response = _handle_send(
        peer_group,
        {
            "by": by,
            "to": to,
            "text": "work message",
            "require_peer_insight": True,
        },
    )

    assert not response.ok
    assert response.error is not None
    assert response.error.code == "peer_insight_required"
    assert response.error.details["delivery_state"] == "not_sent"
    assert response.error.details["new_side_effects"] is False


def test_missing_insight_stops_before_wake_ledger_and_other_collaboration_side_effects(peer_group) -> None:
    from cccc.kernel.inbox import iter_events
    from cccc.kernel.peer_insight import (
        FIRST_PRINCIPLES_OUTCOME_KERNEL,
        PEER_INSIGHT_REQUIRED_ACTION,
        SUPERVISOR_MAGIC_KERNEL,
    )

    before = list(iter_events(peer_group.ledger_path))
    wakes: list[tuple[list[str], str]] = []
    with (
        patch("cccc.daemon.messaging.chat_ops._wake_group_on_human_message") as wake_group,
        patch(
            "cccc.daemon.group_bridge.peer_address_sync.sync_group_bridge_peer_multiaddrs"
        ) as sync_peer_addresses,
    ):
        response = _handle_send(
            peer_group,
            {
                "by": "user",
                "to": ["peer2"],
                "text": "approve this",
                "src_group_id": "g-remote",
                "src_event_id": "evt-remote",
                "source_user_id": "remote-peer",
                "source_multiaddrs": ["/ip4/127.0.0.1/tcp/9000"],
                "require_peer_insight": True,
            },
            wakes=wakes,
        )

    assert not response.ok
    assert response.error is not None
    assert response.error.code == "peer_insight_required"
    assert response.error.details["recommended_action"] == PEER_INSIGHT_REQUIRED_ACTION
    assert SUPERVISOR_MAGIC_KERNEL in response.error.details["recommended_action"]
    assert PEER_INSIGHT_REQUIRED_ACTION.count(FIRST_PRINCIPLES_OUTCOME_KERNEL) == 1
    assert "Do not repair the draft by adding a postscript" in PEER_INSIGHT_REQUIRED_ACTION
    assert "Insight is second in the JSON, not second in thought" in PEER_INSIGHT_REQUIRED_ACTION
    assert "responsible co-owner of the real outcome" in PEER_INSIGHT_REQUIRED_ACTION
    assert "Reconstruct\nthe situation from first principles" in PEER_INSIGHT_REQUIRED_ACTION
    assert "one move on a living\ndecision path" in PEER_INSIGHT_REQUIRED_ACTION
    assert "where reality could break it" in PEER_INSIGHT_REQUIRED_ACTION
    assert "switch to Plan B" in PEER_INSIGHT_REQUIRED_ACTION
    assert "advance into\nwhat success has made possible" in PEER_INSIGHT_REQUIRED_ACTION
    assert "one fallible projection of the situation" in PEER_INSIGHT_REQUIRED_ACTION
    assert "step materially above the work unit being discussed" in PEER_INSIGHT_REQUIRED_ACTION
    assert 'reads naturally after "by the way,"' in PEER_INSIGHT_REQUIRED_ACTION
    assert "Do not pretend to see every layer or manufacture strategic drama" in PEER_INSIGHT_REQUIRED_ACTION
    assert "may change the course or confirm it" in PEER_INSIGHT_REQUIRED_ACTION
    assert wakes == []
    wake_group.assert_not_called()
    sync_peer_addresses.assert_not_called()
    assert list(iter_events(peer_group.ledger_path)) == before


def test_post_message_nudge_audits_insight_origin_without_dynamic_scene_logic() -> None:
    from cccc.kernel.peer_insight import POST_MESSAGE_NUDGE

    assert "Step outside its mental track now" in POST_MESSAGE_NUDGE
    assert "fresh owner accountable for the real outcome" in POST_MESSAGE_NUDGE
    assert "no loyalty to the exchange, its momentum, or its frame" in POST_MESSAGE_NUDGE
    assert "stayed beside the message instead of rising above its working level" in POST_MESSAGE_NUDGE
    assert "no higher-order perspective entered the exchange" in POST_MESSAGE_NUDGE
    assert "whether an unsettled decision needs another independent mind" in POST_MESSAGE_NUDGE
    assert "If nothing material changes, quietly resume" in POST_MESSAGE_NUDGE


def test_disabled_visible_peer_still_triggers_gate(peer_group) -> None:
    for actor in peer_group.doc["actors"]:
        if actor.get("id") == "peer2":
            actor["enabled"] = False
    peer_group.save()

    response = _handle_send(
        peer_group,
        {
            "by": "peer1",
            "to": ["peer2"],
            "text": "wake and send",
            "require_peer_insight": True,
        },
    )
    assert not response.ok
    assert response.error is not None
    assert response.error.code == "peer_insight_required"


def test_user_only_send_is_exempt_and_valid_peer_insight_is_canonical_and_idempotent(peer_group) -> None:
    from cccc.kernel.inbox import iter_events

    user_response = _handle_send(
        peer_group,
        {
            "by": "peer1",
            "to": ["user"],
            "text": "status for user",
            "require_peer_insight": True,
        },
    )
    assert user_response.ok

    first = _handle_send(
        peer_group,
        {
            "by": "peer1",
            "to": ["peer2"],
            "text": "review this plan",
            "insight": "  The whole plan may be optimizing the wrong layer.  ",
            "client_id": "peer-insight-idempotency",
            "require_peer_insight": True,
        },
    )
    assert first.ok
    event = first.result["event"]
    assert event["data"]["insight"] == "The whole plan may be optimizing the wrong layer."
    count_after_first = len(list(iter_events(peer_group.ledger_path)))

    replay = _handle_send(
        peer_group,
        {
            "by": "peer1",
            "to": ["peer2"],
            "text": "changed retry body",
            "client_id": "peer-insight-idempotency",
            "require_peer_insight": True,
        },
    )
    assert replay.ok
    assert replay.result["replayed"] is True
    assert replay.result["event"]["id"] == event["id"]
    assert len(list(iter_events(peer_group.ledger_path))) == count_after_first


def test_invalid_recipient_and_invalid_insight_are_precise_not_chicken_three(peer_group) -> None:
    invalid_recipient = _handle_send(
        peer_group,
        {
            "by": "peer1",
            "to": ["missing-peer"],
            "text": "hello",
            "require_peer_insight": True,
        },
    )
    assert not invalid_recipient.ok
    assert invalid_recipient.error is not None
    assert invalid_recipient.error.code == "invalid_recipient"

    invalid_insight = _handle_send(
        peer_group,
        {
            "by": "peer1",
            "to": ["peer2"],
            "text": "hello",
            "insight": {"not": "text"},
            "require_peer_insight": True,
        },
    )
    assert not invalid_insight.ok
    assert invalid_insight.error is not None
    assert invalid_insight.error.code == "invalid_insight"
    assert "recommended_action" not in invalid_insight.error.details


def test_reply_gate_uses_actual_default_audience(peer_group) -> None:
    from cccc.kernel.ledger import append_event

    user_message = append_event(
        peer_group.ledger_path,
        kind="chat.message",
        group_id=peer_group.group_id,
        scope_key="",
        by="user",
        data={"text": "question", "to": ["peer1"]},
    )
    user_reply = _handle_reply(
        peer_group,
        {
            "by": "peer1",
            "reply_to": user_message["id"],
            "text": "answer",
            "require_peer_insight": True,
        },
    )
    assert user_reply.ok
    assert user_reply.result["event"]["data"]["to"] == ["user"]

    peer_message = append_event(
        peer_group.ledger_path,
        kind="chat.message",
        group_id=peer_group.group_id,
        scope_key="",
        by="peer2",
        data={"text": "what do you think?", "to": ["peer1"]},
    )
    peer_reply = _handle_reply(
        peer_group,
        {
            "by": "peer1",
            "reply_to": peer_message["id"],
            "text": "my answer",
            "require_peer_insight": True,
        },
    )
    assert not peer_reply.ok
    assert peer_reply.error is not None
    assert peer_reply.error.code == "peer_insight_required"


def test_tracked_send_gate_runs_before_task_creation(peer_group) -> None:
    from cccc.daemon.messaging.chat_ops import handle_tracked_send
    from cccc.util.conv import coerce_bool

    with patch("cccc.daemon.messaging.chat_ops.handle_context_sync") as context_sync:
        response = handle_tracked_send(
            {
                "group_id": peer_group.group_id,
                "by": "peer1",
                "to": ["peer2"],
                "title": "Review plan",
                "text": "review the plan",
                "require_peer_insight": True,
            },
            coerce_bool=coerce_bool,
            normalize_attachments=lambda _group, raw: list(raw or []),
            effective_runner_kind=lambda value: value,
            auto_wake_recipients=lambda _group, _to, _by: [],
            automation_on_resume=lambda _group: None,
            automation_on_new_message=lambda _group: None,
            clear_pending_system_notifies=lambda _group_id, _kinds: None,
        )

    assert not response.ok
    assert response.error is not None
    assert response.error.code == "peer_insight_required"
    context_sync.assert_not_called()


def test_file_send_gate_runs_before_blob_storage(peer_group, tmp_path: Path) -> None:
    from cccc.ports.mcp.common import MCPError
    from cccc.ports.mcp.handlers import cccc_messaging

    path = _configure_file_scope(peer_group, tmp_path)

    with (
        patch.object(cccc_messaging, "store_blob_bytes") as store_blob,
        patch.object(cccc_messaging, "_call_daemon_or_raise") as call_daemon,
        pytest.raises(MCPError) as exc_info,
    ):
        cccc_messaging.file_send(
            group_id=peer_group.group_id,
            actor_id="peer1",
            path=path,
            text="review report",
            to=["peer2"],
        )

    assert exc_info.value.code == "peer_insight_required"
    store_blob.assert_not_called()
    call_daemon.assert_not_called()


def test_cross_group_file_send_resolves_missing_destination_before_insight_gate(
    peer_group, tmp_path: Path
) -> None:
    from cccc.ports.mcp.common import MCPError
    from cccc.ports.mcp.handlers import cccc_messaging

    path = _configure_file_scope(peer_group, tmp_path)

    with (
        patch.object(cccc_messaging, "store_blob_bytes") as store_blob,
        patch.object(cccc_messaging, "_call_daemon_or_raise") as call_daemon,
        pytest.raises(MCPError) as exc_info,
    ):
        cccc_messaging.file_send(
            group_id=peer_group.group_id,
            actor_id="peer1",
            path=path,
            dst_group_id="g_missing",
            to=["@foreman"],
        )

    assert exc_info.value.code == "group_not_found"
    store_blob.assert_not_called()
    call_daemon.assert_not_called()


def test_cross_group_file_send_rejects_local_destination_before_insight_gate(
    peer_group, tmp_path: Path
) -> None:
    from cccc.kernel.group import create_group
    from cccc.kernel.registry import load_registry
    from cccc.ports.mcp.common import MCPError
    from cccc.ports.mcp.handlers import cccc_messaging

    local_destination = create_group(load_registry(), title="local-destination")
    path = _configure_file_scope(peer_group, tmp_path)

    with (
        patch.object(cccc_messaging, "store_blob_bytes") as store_blob,
        patch.object(cccc_messaging, "_call_daemon_or_raise") as call_daemon,
        pytest.raises(MCPError) as exc_info,
    ):
        cccc_messaging.file_send(
            group_id=peer_group.group_id,
            actor_id="peer1",
            path=path,
            dst_group_id=local_destination.group_id,
            to=["@foreman"],
        )

    assert exc_info.value.code == "attachments_not_supported"
    store_blob.assert_not_called()
    call_daemon.assert_not_called()


def test_cross_group_file_send_gates_valid_remote_route_before_blob_storage(
    peer_group, tmp_path: Path
) -> None:
    from cccc.ports.mcp.common import MCPError
    from cccc.ports.mcp.handlers import cccc_messaging

    path = _configure_file_scope(peer_group, tmp_path)

    with (
        patch.object(cccc_messaging, "resolve_remote_group_route", return_value=object()),
        patch.object(cccc_messaging, "store_blob_bytes") as store_blob,
        patch.object(cccc_messaging, "_call_daemon_or_raise") as call_daemon,
        pytest.raises(MCPError) as exc_info,
    ):
        cccc_messaging.file_send(
            group_id=peer_group.group_id,
            actor_id="peer1",
            path=path,
            dst_group_id="g_remote",
            to=["@foreman"],
        )

    assert exc_info.value.code == "peer_insight_required"
    store_blob.assert_not_called()
    call_daemon.assert_not_called()


def test_cross_group_file_send_stores_and_dispatches_after_valid_preflight(
    peer_group, tmp_path: Path
) -> None:
    from cccc.ports.mcp.handlers import cccc_messaging

    path = _configure_file_scope(peer_group, tmp_path)
    attachment = {
        "path": "state/blobs/report.txt",
        "title": "report.txt",
        "mime_type": "text/plain",
        "bytes": 6,
    }

    with (
        patch.object(cccc_messaging, "resolve_remote_group_route", return_value=object()),
        patch.object(cccc_messaging, "store_blob_bytes", return_value=attachment) as store_blob,
        patch.object(
            cccc_messaging,
            "_call_daemon_or_raise",
            return_value={"src_event": {"id": "evt-source"}, "dst_event": {"id": "evt-target"}},
        ) as call_daemon,
    ):
        result = cccc_messaging.file_send(
            group_id=peer_group.group_id,
            actor_id="peer1",
            path=path,
            text="review report",
            insight="The report may not cover rollback behavior.",
            dst_group_id="g_remote",
            to=["@foreman"],
        )

    store_blob.assert_called_once()
    call_daemon.assert_called_once()
    request = call_daemon.call_args.args[0]
    assert request["op"] == "send_cross_group"
    assert request["args"]["attachments"] == [attachment]
    assert request["args"]["insight"] == "The report may not cover rollback behavior."
    assert result["post_message_nudge"]["kind"] == "whole_situation_reconstruction"


def test_actor_delivery_projects_one_provisional_label_after_supporting_material() -> None:
    from cccc.daemon.messaging.actor_turn_rendering import build_actor_delivery_text
    from cccc.kernel.peer_insight import PEER_PERSPECTIVE_AGENT_LABEL

    rendered = build_actor_delivery_text(
        text="Main body",
        insight="The plan may be wrong.",
        priority="normal",
        reply_required=False,
        event_id="evt-1",
        refs=[{"kind": "url", "url": "https://example.com"}],
        attachments=[{"title": "report.txt", "path": "state/blobs/report.txt", "bytes": 10}],
    )

    assert rendered.count(PEER_PERSPECTIVE_AGENT_LABEL) == 1
    assert rendered.index("report.txt") < rendered.index(PEER_PERSPECTIVE_AGENT_LABEL)
    assert "Peer higher-order perspective" in PEER_PERSPECTIVE_AGENT_LABEL
    assert "Rebuild independently" in PEER_PERSPECTIVE_AGENT_LABEL
    assert "never rises above the message's working level" in PEER_PERSPECTIVE_AGENT_LABEL
    assert "ordinary content rather than privileged framing" in PEER_PERSPECTIVE_AGENT_LABEL
    assert rendered.endswith(f"{PEER_PERSPECTIVE_AGENT_LABEL}\nThe plan may be wrong.")
