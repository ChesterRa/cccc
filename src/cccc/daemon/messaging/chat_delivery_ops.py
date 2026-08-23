"""Chat message delivery operations.

This module owns post-ledger delivery to actor runtimes. Callers append the
canonical chat event first, then schedule this work outside the request path.
"""

from __future__ import annotations

import logging
from typing import Any, Callable, Optional

from ...kernel.actors import list_actors
from ...kernel.group import load_group
from ..actors import deepseek_runtime
from ..actors.runner_ops import _effective_runner_kind as default_effective_runner_kind
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from .actor_delivery_planner import (
    TRANSPORT_CLAUDE_HEADLESS,
    TRANSPORT_CODEX_APP_SERVER,
    TRANSPORT_CODEX_HEADLESS,
    TRANSPORT_DEEPSEEK_HEADLESS,
    TRANSPORT_PTY,
    TRANSPORT_SKIP,
    TRANSPORT_WEB_MODEL_BROWSER,
    plan_actor_chat_delivery,
)
from .actor_turn_rendering import (
    build_actor_delivery_text,
    build_actor_headless_delivery_text,
    render_mail_pending_hint,
)
from ..actors.web_model_browser_delivery import schedule_web_model_browser_delivery, web_model_browser_delivery_enabled
from .chat_support_ops import schedule_headless_post_wake_delivery
from .chat_queue_payload import build_chat_queue_payload
from .delivery import queue_chat_message, request_flush_pending_messages, should_deliver_message
from .runtime_delivery import append_delivery_state, claim_delivery


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
    message_mode: str,
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
    force_ambiguous: bool = False,
    preclaimed_actors: Optional[dict[str, tuple[str, str]]] = None,
) -> None:
    clean_reply_to = str(reply_to or "").strip()
    clean_attachments = [item for item in (attachments or []) if isinstance(item, dict)]
    remaining_preclaimed = dict(preclaimed_actors or {})
    current_runtime_group = load_group(str(group.group_id or "").strip())
    if current_runtime_group is not None and not should_deliver_message(current_runtime_group, "chat.message"):
        current_runtime_group = None
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
            deepseek_headless_running=deepseek_runtime.running,
            web_model_browser_delivery_enabled=web_model_browser_delivery_enabled,
        )
        actor_id = decision.actor_id
        actor_created_at = str(actor.get("created_at") or "").strip()
        actor_delivery_text = delivery_text
        actor_headless_delivery_text = headless_delivery_text
        if message_mode in {"send", "request_reply"}:
            mail_hint = render_mail_pending_hint(
                group=group,
                actor_id=actor_id,
            )
            if mail_hint:
                actor_delivery_text = f"{actor_delivery_text.rstrip()}\n\n{mail_hint}"
                actor_headless_delivery_text = (
                    f"{actor_headless_delivery_text.rstrip()}\n\n{mail_hint}"
                )
        preclaimed_entry = remaining_preclaimed.pop(actor_id, None)
        preclaimed = preclaimed_entry is not None
        preclaimed_created_at, preclaimed_transport = preclaimed_entry or ("", "")
        if preclaimed and preclaimed_created_at != actor_created_at:
            append_delivery_state(
                group,
                actor_id=actor_id,
                actor_created_at=preclaimed_created_at,
                source_event_id=event_id,
                state="failed",
                transport=preclaimed_transport,
                reason="recipient actor generation changed before delivery",
            )
            continue
        queue_after_deepseek_wake = actor_id in woken and decision.reason == "deepseek_headless_not_running"
        transport = decision.transport
        if queue_after_deepseek_wake:
            transport = TRANSPORT_DEEPSEEK_HEADLESS
        elif actor_id in woken and decision.reason in {
            "codex_headless_not_running",
            "claude_headless_not_running",
        }:
            transport = f"{decision.runtime}_headless_post_wake"

        def finish(
            state: str,
            reason: str = "",
            *,
            delivery_transport: str = transport,
            target_actor_id: str = actor_id,
            target_created_at: str = actor_created_at,
        ) -> None:
            append_delivery_state(
                group,
                actor_id=target_actor_id,
                actor_created_at=target_created_at,
                source_event_id=event_id,
                state=state,
                transport=delivery_transport,
                reason=reason,
            )

        if decision.transport in {
            TRANSPORT_CODEX_HEADLESS,
            TRANSPORT_CODEX_APP_SERVER,
            TRANSPORT_CLAUDE_HEADLESS,
            TRANSPORT_DEEPSEEK_HEADLESS,
            TRANSPORT_WEB_MODEL_BROWSER,
        } or queue_after_deepseek_wake:
            if current_runtime_group is None:
                logger.debug("[chat-delivery] defer actor=%s while group delivery is disabled", actor_id)
                if preclaimed:
                    finish("failed", "group delivery is disabled")
                continue
        if transport == TRANSPORT_SKIP:
            logger.debug("[chat-delivery] skip actor=%s (%s)", actor_id, decision.reason)
            if preclaimed and preclaimed_transport == "web_model_pull":
                # Pull-based structured runtimes consume this durable claim through
                # runtime_wait_next_turn; there is no direct transport to invoke.
                continue
            if preclaimed:
                finish("failed", decision.reason)
            continue
        if not preclaimed:
            claimed, _ = claim_delivery(
                group,
                actor_id=actor_id,
                actor_created_at=actor_created_at,
                source_event_id=event_id,
                transport=transport,
                force_ambiguous=force_ambiguous,
            )
            if not claimed:
                continue

        if decision.transport in {TRANSPORT_CODEX_HEADLESS, TRANSPORT_CODEX_APP_SERVER}:
            try:
                delivered = bool(
                    codex_submit_user_message(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        text=actor_headless_delivery_text,
                        event_id=event_id,
                        ts=event_ts,
                        reply_to=clean_reply_to or None,
                        attachments=clean_attachments,
                    )
                )
                finish("accepted" if delivered else "failed", "" if delivered else "runtime rejected payload")
            except Exception as exc:
                finish("failed", str(exc))
        elif decision.transport == TRANSPORT_CLAUDE_HEADLESS:
            try:
                delivered = bool(
                    claude_submit_user_message(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        text=actor_headless_delivery_text,
                        event_id=event_id,
                        ts=event_ts,
                        reply_to=clean_reply_to or None,
                        attachments=clean_attachments,
                    )
                )
                finish("accepted" if delivered else "failed", "" if delivered else "runtime rejected payload")
            except Exception as exc:
                finish("failed", str(exc))
        elif decision.transport in {TRANSPORT_DEEPSEEK_HEADLESS, TRANSPORT_PTY} or queue_after_deepseek_wake:
            is_deepseek_queue = decision.transport == TRANSPORT_DEEPSEEK_HEADLESS or queue_after_deepseek_wake
            kwargs = build_chat_queue_payload(
                actor_id=actor_id,
                event_id=event_id,
                by=by,
                effective_to=effective_to,
                delivery_text=actor_delivery_text,
                event_ts=event_ts,
                reply_to=clean_reply_to,
                quote_text=quote_text,
                source_platform=source_platform,
                source_user_name=source_user_name,
                source_user_id=source_user_id,
                deduplicate_by_event_id=is_deepseek_queue,
            )
            try:
                queued = queue_chat_message(group, **kwargs)
                if queued:
                    request_flush_pending_messages(group, actor_id=actor_id)
                else:
                    finish("ambiguous", "payload was already present in the runtime queue")
            except Exception as exc:
                finish("failed", str(exc))
        elif decision.transport == TRANSPORT_WEB_MODEL_BROWSER:
            if not schedule_web_model_browser_delivery(
                group_id=group.group_id,
                actor_id=actor_id,
                trigger_event_id=event_id,
                logger=logger,
            ):
                finish("failed", "browser delivery worker was not scheduled")
        elif actor_id in woken and decision.reason in {"codex_headless_not_running", "claude_headless_not_running"}:
            if current_runtime_group is None:
                logger.debug("[chat-delivery] defer post-wake actor=%s while group delivery is disabled", actor_id)
                finish("failed", "group delivery is disabled")
                continue
            scheduled = schedule_headless_post_wake_delivery(
                group_id=group.group_id,
                actor_id=actor_id,
                runtime=decision.runtime,
                text=actor_headless_delivery_text,
                event_id=event_id,
                ts=event_ts,
                reply_to=clean_reply_to or None,
                attachments=clean_attachments,
                codex_actor_running=codex_actor_running,
                claude_actor_running=claude_actor_running,
                codex_submit_user_message=codex_submit_user_message,
                claude_submit_user_message=claude_submit_user_message,
                logger=logger,
                on_result=lambda accepted, reason, finish=finish: finish(
                    "accepted" if accepted else "failed",
                    reason,
                ),
            )
            if not scheduled:
                finish("failed", "post-wake delivery worker was not scheduled")
        else:
            finish("failed", decision.reason)

    for actor_id, (actor_created_at, transport) in remaining_preclaimed.items():
        append_delivery_state(
            group,
            actor_id=actor_id,
            actor_created_at=actor_created_at,
            source_event_id=event_id,
            state="failed",
            transport=transport,
            reason="recipient actor no longer exists",
        )


def deliver_appended_chat_message(
    *,
    group: Any,
    event: dict[str, Any],
    by: str,
    effective_to: list[str],
    text: str,
    insight: str | None = None,
    message_mode: str,
    refs: Optional[list[dict[str, Any]]] = None,
    attachments: Optional[list[dict[str, Any]]] = None,
    reply_to: str = "",
    quote_text: str = "",
    source_platform: str = "",
    source_user_name: str = "",
    source_user_id: str = "",
    src_group_id: str = "",
    src_event_id: str = "",
    effective_runner_kind: Callable[[str], str] = default_effective_runner_kind,
    codex_actor_running: Callable[[str, str], bool] = codex_app_supervisor.actor_running,
    claude_actor_running: Callable[[str, str], bool] = claude_app_supervisor.actor_running,
    codex_submit_user_message: Callable[..., bool] = codex_app_supervisor.submit_user_message,
    claude_submit_user_message: Callable[..., bool] = claude_app_supervisor.submit_user_message,
    woken: Optional[set[str]] = None,
    force_ambiguous: bool = False,
    preclaimed_actors: Optional[dict[str, tuple[str, str]]] = None,
    logger: logging.Logger = logging.getLogger("cccc.daemon.server"),
) -> None:
    clean_refs = [item for item in (refs or []) if isinstance(item, dict)]
    clean_attachments = [item for item in (attachments or []) if isinstance(item, dict)]
    event_id = str(event.get("id") or "").strip()
    event_ts = str(event.get("ts") or "").strip()
    event_data = event.get("data") if isinstance(event.get("data"), dict) else {}
    remote_reply_to = (
        [str(item or "").strip() for item in event_data.get("remote_reply_to") if str(item or "").strip()]
        if isinstance(event_data.get("remote_reply_to"), list)
        else []
    )
    delivery_text = build_actor_delivery_text(
        text=text,
        insight=insight,
        message_mode=message_mode,
        event_id=event_id,
        refs=clean_refs,
        attachments=clean_attachments,
        src_group_id=src_group_id,
        src_event_id=src_event_id,
        remote_reply_to=remote_reply_to,
    )
    headless_delivery_text = build_actor_headless_delivery_text(
        by=by,
        to=effective_to,
        body=delivery_text,
        reply_to=reply_to,
        quote_text=quote_text,
        source_platform=source_platform,
        source_user_name=source_user_name,
        source_user_id=source_user_id,
    )
    deliver_chat_message(
        group=group,
        event=event,
        by=by,
        effective_to=effective_to,
        delivery_text=delivery_text,
        headless_delivery_text=headless_delivery_text,
        event_id=event_id,
        event_ts=event_ts,
        message_mode=message_mode,
        effective_runner_kind=effective_runner_kind,
        codex_actor_running=codex_actor_running,
        claude_actor_running=claude_actor_running,
        codex_submit_user_message=codex_submit_user_message,
        claude_submit_user_message=claude_submit_user_message,
        woken=woken or set(),
        logger=logger,
        attachments=clean_attachments,
        reply_to=reply_to,
        quote_text=quote_text,
        source_platform=source_platform,
        source_user_name=source_user_name,
        source_user_id=source_user_id,
        force_ambiguous=force_ambiguous,
        preclaimed_actors=preclaimed_actors,
    )
