"""Canonical pending runtime-delivery recovery."""

from __future__ import annotations

from typing import Any

from ...contracts.v1 import SystemNotifyData
from ...kernel.actors import find_actor
from ...kernel.runtime_state_source import actor_uses_codex_app_server_state
from .actor_turn_rendering import (
    build_actor_delivery_text,
    render_actor_event_for_delivery,
    render_mail_pending_hint,
)
from .runtime_delivery import append_delivery_state, pending_runtime_delivery_events


def _deliver_headless_app_messages(
    group: Any,
    *,
    actor: dict[str, Any],
    actor_id: str,
    runtime: str,
    limit: int,
) -> int:
    from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
    from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
    supervisor = codex_app_supervisor if runtime == "codex" else claude_app_supervisor
    if not supervisor.actor_running(group.group_id, actor_id):
        return 0

    actor_created_at = str(actor.get("created_at") or "").strip()
    transport = f"{runtime}_headless"
    delivered_count = 0
    mail_hint = render_mail_pending_hint(group=group, actor_id=actor_id)
    for _ in range(max(1, int(limit or 1))):
        events = pending_runtime_delivery_events(
            group,
            actor_id=actor_id,
            actor_created_at=actor_created_at,
            transport=transport,
            limit=1,
            claim_unclaimed_chat=True,
        )
        if not events:
            break
        event = events[0]
        event_id = str(event.get("id") or "").strip()
        event_data = event.get("data") if isinstance(event.get("data"), dict) else {}
        delivery_text = render_actor_event_for_delivery(event, actor_id=actor_id)
        if mail_hint and str(event.get("kind") or "") == "chat.message":
            delivery_text = f"{delivery_text.rstrip()}\n\n{mail_hint}"
        try:
            accepted = bool(
                supervisor.submit_user_message(
                    group_id=group.group_id,
                    actor_id=actor_id,
                    text=delivery_text,
                    event_id=event_id,
                    ts=str(event.get("ts") or "").strip(),
                    reply_to=str(event_data.get("reply_to") or "").strip() or None,
                    attachments=[
                        item
                        for item in event_data.get("attachments", [])
                        if isinstance(item, dict)
                    ]
                    if isinstance(event_data.get("attachments"), list)
                    else [],
                )
            )
            reason = "" if accepted else "runtime rejected payload"
        except Exception as exc:
            accepted = False
            reason = str(exc)
        append_delivery_state(
            group,
            actor_id=actor_id,
            actor_created_at=actor_created_at,
            source_event_id=event_id,
            state="accepted" if accepted else "failed",
            transport=transport,
            reason=reason,
        )
        if not accepted:
            break
        delivered_count += 1
        mail_hint = ""
    return delivered_count


def refill_unread_runtime_messages(
    group: Any, *, actor_id: str, limit: int = 256
) -> int:
    """Recover pending direct delivery without changing the Mail cursor."""
    from .delivery import (
        PendingMessage,
        THROTTLE,
        _render_system_notify_message_for_delivery,
        should_deliver_message,
    )

    aid = str(actor_id or "").strip()
    actor = find_actor(group, aid)
    if not aid or not isinstance(actor, dict) or not bool(actor.get("enabled", True)):
        return 0
    if not should_deliver_message(group, "chat.message"):
        return 0
    runner = str(actor.get("runner") or "pty").strip()
    runtime = str(actor.get("runtime") or "").strip().lower()
    if runner == "headless" and runtime in {"codex", "claude"}:
        return _deliver_headless_app_messages(
            group,
            actor=actor,
            actor_id=aid,
            runtime=runtime,
            limit=limit,
        )
    if runner != "pty" and not (runner == "headless" and runtime == "deepseek"):
        return 0
    if actor_uses_codex_app_server_state(actor):
        return 0

    recovered: list[PendingMessage] = []
    actor_created_at = str(actor.get("created_at") or "").strip()
    transport = "deepseek_headless" if runner == "headless" else "pty"
    events = pending_runtime_delivery_events(
        group,
        actor_id=aid,
        actor_created_at=actor_created_at,
        transport=transport,
        limit=max(1, int(limit or 256)),
        claim_unclaimed_chat=True,
    )
    mail_hint = render_mail_pending_hint(group=group, actor_id=aid)
    direct_event_ids = [
        str(event.get("id") or "").strip()
        for event in events
        if str(event.get("kind") or "") == "chat.message"
    ]
    hint_event_id = direct_event_ids[-1] if direct_event_ids else ""
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
            delivery_text = build_actor_delivery_text(
                text=str(data.get("text") or ""),
                insight=data.get("insight"),
                message_mode=str(data.get("message_mode") or "send"),
                event_id=event_id,
                refs=refs,
                attachments=attachments,
                src_group_id=str(data.get("src_group_id") or ""),
                src_event_id=str(data.get("src_event_id") or ""),
                remote_reply_to=remote_reply_to,
            )
            if mail_hint and event_id == hint_event_id:
                delivery_text = f"{delivery_text.rstrip()}\n\n{mail_hint}"
            recovered.append(
                PendingMessage(
                    event_id=event_id,
                    by=str(event.get("by") or "user").strip() or "user",
                    to=recipients or [aid],
                    text=delivery_text,
                    message_mode=str(data.get("message_mode") or "send"),
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
    recovered_count = THROTTLE.recover_front(group.group_id, aid, recovered)
    return recovered_count
