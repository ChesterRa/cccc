"""Chat message delivery operations.

This module owns post-ledger delivery to actor runtimes. Callers append the
canonical chat event first, then schedule this work outside the request path.
"""

from __future__ import annotations

import logging
from typing import Any, Callable, Optional

from ...contracts.v1 import SystemNotifyData
from ...kernel.actors import find_actor, list_actors
from .actor_delivery_planner import (
    TRANSPORT_CLAUDE_HEADLESS,
    TRANSPORT_CODEX_APP_SERVER,
    TRANSPORT_CODEX_HEADLESS,
    TRANSPORT_PTY,
    TRANSPORT_WEB_MODEL_BROWSER,
    event_with_effective_to,
    plan_actor_chat_delivery,
)
from ..actors.web_model_browser_delivery import schedule_web_model_browser_delivery, web_model_browser_delivery_enabled
from .chat_support_ops import schedule_headless_post_wake_delivery
from .delivery import emit_system_notify, get_headless_targets_for_message, queue_chat_message, request_flush_pending_messages


def notify_headless_targets(
    *,
    group: Any,
    by: str,
    event_id: str,
    priority: str,
    reply_required: bool,
    event: dict[str, Any],
    skip_actor_ids: Optional[set[str]] = None,
) -> None:
    try:
        headless_targets = get_headless_targets_for_message(group, event=event, by=by)
        skip_ids = {str(item).strip() for item in (skip_actor_ids or set()) if str(item).strip()}
        if reply_required:
            notify_title = "Need reply"
            notify_priority = "urgent" if priority == "attention" else "high"
        else:
            notify_title = "Needs acknowledgement" if priority == "attention" else "New message"
            notify_priority = "urgent" if priority == "attention" else "high"
        for actor_id in headless_targets:
            if actor_id in skip_ids:
                continue
            actor = find_actor(group, actor_id)
            if isinstance(actor, dict) and str(actor.get("runtime") or "").strip().lower() == "web_model":
                continue
            emit_system_notify(
                group,
                by="system",
                notify=SystemNotifyData(
                    kind="info",
                    priority=notify_priority,
                    title=notify_title,
                    message=f"New message from {by}. Check your inbox.",
                    target_actor_id=actor_id,
                    requires_ack=False,
                    context={"event_id": event_id, "from": by},
                ),
            )
    except Exception:
        pass


def deliver_chat_message(
    *,
    group: Any,
    event: dict[str, Any],
    by: str,
    effective_to: list[str],
    delivery_text: str,
    headless_delivery_text: str,
    event_id: str,
    event_ts: str,
    priority: str,
    reply_required: bool,
    effective_runner_kind: Callable[[str], str],
    codex_actor_running: Callable[[str, str], bool],
    claude_actor_running: Callable[[str, str], bool],
    codex_submit_user_message: Callable[..., bool],
    claude_submit_user_message: Callable[..., bool],
    woken: set[str],
    logger: logging.Logger,
    attachments: Optional[list[dict[str, Any]]] = None,
    reply_to: str = "",
    quote_text: str = "",
    source_platform: str = "",
    source_user_name: str = "",
    source_user_id: str = "",
) -> None:
    skip_headless_notify_actor_ids: set[str] = set()
    clean_reply_to = str(reply_to or "").strip()
    clean_attachments = [item for item in (attachments or []) if isinstance(item, dict)]
    for actor in list_actors(group):
        if not isinstance(actor, dict):
            continue
        decision = plan_actor_chat_delivery(
            group=group,
            actor=actor,
            event=event,
            by=by,
            effective_to=effective_to,
            effective_runner_kind=effective_runner_kind,
            codex_headless_running=codex_actor_running,
            claude_headless_running=claude_actor_running,
            web_model_browser_delivery_enabled=web_model_browser_delivery_enabled,
        )
        actor_id = decision.actor_id
        if decision.transport in {TRANSPORT_CODEX_HEADLESS, TRANSPORT_CODEX_APP_SERVER}:
            delivered = bool(
                codex_submit_user_message(
                    group_id=group.group_id,
                    actor_id=actor_id,
                    text=headless_delivery_text,
                    event_id=event_id,
                    ts=event_ts,
                    reply_to=clean_reply_to or None,
                    attachments=clean_attachments,
                )
            )
            if delivered:
                skip_headless_notify_actor_ids.add(actor_id)
        elif decision.transport == TRANSPORT_CLAUDE_HEADLESS:
            delivered = bool(
                claude_submit_user_message(
                    group_id=group.group_id,
                    actor_id=actor_id,
                    text=headless_delivery_text,
                    event_id=event_id,
                    ts=event_ts,
                    reply_to=clean_reply_to or None,
                    attachments=clean_attachments,
                )
            )
            if delivered:
                skip_headless_notify_actor_ids.add(actor_id)
        elif decision.transport == TRANSPORT_PTY:
            kwargs: dict[str, Any] = {
                "actor_id": actor_id,
                "event_id": event_id,
                "by": by,
                "to": effective_to,
                "text": delivery_text,
                "ts": event_ts,
            }
            if clean_reply_to:
                kwargs["reply_to"] = clean_reply_to
                kwargs["quote_text"] = quote_text
            else:
                kwargs["source_platform"] = source_platform or None
                kwargs["source_user_name"] = source_user_name or None
                kwargs["source_user_id"] = source_user_id or None
            queue_chat_message(group, **kwargs)
            request_flush_pending_messages(group, actor_id=actor_id)
        elif decision.transport == TRANSPORT_WEB_MODEL_BROWSER:
            if schedule_web_model_browser_delivery(
                group_id=group.group_id,
                actor_id=actor_id,
                trigger_event_id=event_id,
                logger=logger,
            ):
                skip_headless_notify_actor_ids.add(actor_id)
        elif actor_id in woken and decision.reason in {"codex_headless_not_running", "claude_headless_not_running"}:
            if schedule_headless_post_wake_delivery(
                group_id=group.group_id,
                actor_id=actor_id,
                runtime=decision.runtime,
                text=headless_delivery_text,
                event_id=event_id,
                ts=event_ts,
                reply_to=clean_reply_to or None,
                attachments=clean_attachments,
                codex_actor_running=codex_actor_running,
                claude_actor_running=claude_actor_running,
                codex_submit_user_message=codex_submit_user_message,
                claude_submit_user_message=claude_submit_user_message,
                logger=logger,
            ):
                skip_headless_notify_actor_ids.add(actor_id)
        else:
            logger.debug("[chat-delivery] skip actor=%s (%s)", actor_id, decision.reason)

    notify_headless_targets(
        group=group,
        by=by,
        event_id=event_id,
        priority=priority,
        reply_required=reply_required,
        event=event_with_effective_to(event, effective_to),
        skip_actor_ids=skip_headless_notify_actor_ids,
    )
