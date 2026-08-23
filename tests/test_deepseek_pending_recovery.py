from __future__ import annotations

from unittest.mock import patch

from cccc.daemon.messaging import delivery
from cccc.daemon.messaging.deepseek_pending_recovery import recover_pending_messages
from cccc.kernel.group import Group
from cccc.kernel.headless_events import append_headless_event
from cccc.kernel.ledger import append_event


def _deepseek_group(tmp_path):
    actor = {
        "id": "deepseek",
        "runtime": "deepseek",
        "runner": "headless",
        "enabled": True,
    }
    group = Group(
        group_id="deepseek-restart",
        path=tmp_path,
        doc={"group_id": "deepseek-restart", "actors": [actor], "automation": {}},
    )
    append_event(
        group.ledger_path,
        kind="actor.add",
        group_id=group.group_id,
        scope_key="",
        by="user",
        data={"actor": actor},
    )
    return group


def _group_with_unread(tmp_path):
    group = _deepseek_group(tmp_path)
    events = [
        append_event(
            group.ledger_path,
            kind="chat.message",
            group_id=group.group_id,
            scope_key="",
            by="user",
            data={"to": ["deepseek"], "text": text, "message_mode": "send"},
        )
        for text in ("first", "second")
    ]
    return group, events


def test_restart_rebuilds_pending_direct_delivery_queue(tmp_path) -> None:
    group, events = _group_with_unread(tmp_path)
    throttle = delivery.DeliveryThrottle()
    with (
        patch.object(delivery, "THROTTLE", throttle),
        patch.object(
            delivery, "request_flush_pending_messages", return_value=True
        ) as request_flush,
    ):
        assert recover_pending_messages(group, actor_id="deepseek") == 2
        pending = throttle.take_pending(group.group_id, "deepseek")

    assert [item.event_id for item in pending] == [event["id"] for event in events]
    request_flush.assert_called_once_with(group, actor_id="deepseek")


def test_restart_skips_durably_completed_prefix_before_requeue(tmp_path) -> None:
    group, events = _group_with_unread(tmp_path)
    append_headless_event(
        group.path,
        group_id=group.group_id,
        actor_id="deepseek",
        event_type="headless.turn.completed",
        data={"event_id": events[0]["id"]},
        dedupe_key=f"deepseek.turn:headless.turn.completed:{events[0]['id']}",
    )
    throttle = delivery.DeliveryThrottle()
    with (
        patch.object(delivery, "THROTTLE", throttle),
        patch.object(delivery, "request_flush_pending_messages", return_value=True),
    ):
        assert recover_pending_messages(group, actor_id="deepseek") == 2
        pending = throttle.take_pending(group.group_id, "deepseek")

    assert [item.event_id for item in pending] == [events[1]["id"]]


def test_restart_applies_legacy_read_watermark_to_notification_prefix(tmp_path) -> None:
    group = _deepseek_group(tmp_path)
    old_message = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key="",
        by="user",
        data={"to": ["deepseek"], "text": "legacy"},
    )
    linked_notice = append_event(
        group.ledger_path,
        kind="system.notify",
        group_id=group.group_id,
        scope_key="",
        by="system",
        data={
            "kind": "info",
            "target_actor_id": "deepseek",
            "context": {"event_id": old_message["id"]},
        },
    )
    watermark_notice = append_event(
        group.ledger_path,
        kind="system.notify",
        group_id=group.group_id,
        scope_key="",
        by="system",
        data={"target_actor_id": "deepseek", "kind": "info"},
    )
    append_event(
        group.ledger_path,
        kind="chat.read",
        group_id=group.group_id,
        scope_key="",
        by="deepseek",
        data={"actor_id": "deepseek", "event_id": watermark_notice["id"]},
    )
    late_linked_notice = append_event(
        group.ledger_path,
        kind="system.notify",
        group_id=group.group_id,
        scope_key="",
        by="system",
        data={
            "kind": "info",
            "target_actor_id": "deepseek",
            "context": {"event_id": old_message["id"]},
        },
    )
    current_notice = append_event(
        group.ledger_path,
        kind="system.notify",
        group_id=group.group_id,
        scope_key="",
        by="system",
        data={"target_actor_id": "deepseek", "kind": "info"},
    )
    current = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key="",
        by="user",
        data={"to": ["deepseek"], "text": "current", "message_mode": "send"},
    )
    throttle = delivery.DeliveryThrottle()

    with (
        patch.object(delivery, "THROTTLE", throttle),
        patch.object(delivery, "request_flush_pending_messages", return_value=True),
    ):
        assert recover_pending_messages(group, actor_id="deepseek") == 2
        pending = throttle.take_pending(group.group_id, "deepseek")

    assert [item.event_id for item in pending] == [current_notice["id"], current["id"]]
    assert linked_notice["id"] not in {item.event_id for item in pending}
    assert watermark_notice["id"] not in {item.event_id for item in pending}
    assert late_linked_notice["id"] not in {item.event_id for item in pending}


def test_failed_batch_requeues_only_unfinished_suffix(tmp_path) -> None:
    group, events = _group_with_unread(tmp_path)
    throttle = delivery.DeliveryThrottle()
    for event in events:
        delivery_text = str(event.get("data", {}).get("text") or "")
        throttle.queue_message(
            group.group_id,
            "deepseek",
            event_id=event["id"],
            by="user",
            to=["deepseek"],
            text=delivery_text,
            ts=event["ts"],
        )

    with (
        patch.object(delivery, "THROTTLE", throttle),
        patch.object(delivery, "get_group_state", return_value="active"),
        patch.object(
            delivery.pty_runner.SUPERVISOR, "startup_times", return_value=(None, None)
        ),
        patch(
            "cccc.daemon.messaging.deepseek_delivery.recover_durable_terminals",
            return_value=0,
        ),
        patch(
            "cccc.daemon.messaging.deepseek_delivery.deliver_messages",
            side_effect=[True, False],
        ) as deliver_messages,
    ):
        assert delivery.flush_pending_messages(group, actor_id="deepseek") is False
        pending = throttle.take_pending(group.group_id, "deepseek")

    assert [item.event_id for item in pending] == [events[1]["id"]]
    assert [
        call.kwargs["messages"][0].event_id for call in deliver_messages.call_args_list
    ] == [
        events[0]["id"],
        events[1]["id"],
    ]
