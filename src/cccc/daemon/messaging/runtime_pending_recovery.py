"""Canonical unread reconstruction for PTY and DeepSeek delivery queues."""

from __future__ import annotations

from typing import Any

from ...contracts.v1 import SystemNotifyData
from ...kernel.actors import find_actor
from ...kernel.inbox import unread_messages
from ...kernel.runtime_state_source import actor_uses_codex_app_server_state
from .actor_turn_rendering import build_actor_delivery_text


def refill_unread_runtime_messages(
    group: Any, *, actor_id: str, limit: int = 256
) -> int:
    """Rebuild a supported runtime's pending queue from canonical unread events."""
    from .delivery import (
        PendingMessage,
        THROTTLE,
        _render_system_notify_message_for_delivery,
    )

    aid = str(actor_id or "").strip()
    actor = find_actor(group, aid)
    if not aid or not isinstance(actor, dict) or not bool(actor.get("enabled", True)):
        return 0
    runner = str(actor.get("runner") or "pty").strip()
    runtime = str(actor.get("runtime") or "").strip().lower()
    if runner != "pty" and not (runner == "headless" and runtime == "deepseek"):
        return 0
    if actor_uses_codex_app_server_state(actor):
        return 0

    recovered: list[PendingMessage] = []
    events = unread_messages(
        group,
        actor_id=aid,
        limit=max(1, int(limit or 256)),
        kind_filter="all",
    )
    for event in events:
        event_id = str(event.get("id") or "").strip()
        event_ts = str(event.get("ts") or "").strip()
        kind = str(event.get("kind") or "").strip()
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if not event_id:
            continue
        if kind == "chat.message":
            refs = (
                [item for item in data.get("refs", []) if isinstance(item, dict)]
                if isinstance(data.get("refs"), list)
                else []
            )
            attachments = (
                [item for item in data.get("attachments", []) if isinstance(item, dict)]
                if isinstance(data.get("attachments"), list)
                else []
            )
            remote_reply_to = (
                [
                    str(item or "").strip()
                    for item in data.get("remote_reply_to", [])
                    if str(item or "").strip()
                ]
                if isinstance(data.get("remote_reply_to"), list)
                else []
            )
            recipients = (
                [
                    str(item or "").strip()
                    for item in data.get("to", [])
                    if str(item or "").strip()
                ]
                if isinstance(data.get("to"), list)
                else [aid]
            )
            recovered.append(
                PendingMessage(
                    event_id=event_id,
                    by=str(event.get("by") or "user").strip() or "user",
                    to=recipients or [aid],
                    text=build_actor_delivery_text(
                        text=str(data.get("text") or ""),
                        insight=data.get("insight"),
                        priority=str(data.get("priority") or "normal"),
                        reply_required=bool(data.get("reply_required")),
                        event_id=event_id,
                        refs=refs,
                        attachments=attachments,
                        src_group_id=str(data.get("src_group_id") or ""),
                        src_event_id=str(data.get("src_event_id") or ""),
                        remote_reply_to=remote_reply_to,
                    ),
                    reply_to=str(data.get("reply_to") or "") or None,
                    quote_text=str(data.get("quote_text") or "") or None,
                    source_platform=str(data.get("source_platform") or "") or None,
                    source_user_name=str(data.get("source_user_name") or "") or None,
                    source_user_id=str(data.get("source_user_id") or "") or None,
                    ts=event_ts,
                )
            )
            continue
        if kind == "system.notify":
            try:
                notify = SystemNotifyData.model_validate(data)
            except Exception:
                continue
            recovered.append(
                PendingMessage(
                    event_id=event_id,
                    by="system",
                    to=[aid],
                    text="",
                    kind="system.notify",
                    notify_kind=str(notify.kind),
                    notify_title=str(notify.title or ""),
                    notify_message=_render_system_notify_message_for_delivery(
                        notify=notify, group=group
                    ),
                    ts=event_ts,
                )
            )
    return THROTTLE.recover_front(group.group_id, aid, recovered)
