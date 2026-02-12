"""Chat send/reply operation handlers for daemon."""

from __future__ import annotations

import logging
import re
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import ChatMessageData, DaemonError, DaemonResponse
from ...kernel.actors import find_actor, list_actors, resolve_recipient_tokens
from ...kernel.group import get_group_state, load_group, set_group_state
from ...kernel.inbox import find_event, get_quote_text, has_chat_ack, is_message_for_actor
from ...kernel.ledger import append_event
from ...kernel.messaging import (
    default_reply_recipients,
    enabled_recipient_actor_ids,
    get_default_send_to,
    targets_any_agent,
)
from ...kernel.registry import load_registry
from ...kernel.scope import detect_scope
from ...util.time import utc_now_iso
from ..delivery import get_headless_targets_for_message, queue_chat_message

logger = logging.getLogger("cccc.daemon.server")


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _wake_group_on_human_message(
    group: Any,
    *,
    by: str,
    automation_on_resume: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str, set[str]], None],
) -> Any:
    # Keep idle stable against agent chatter / throttled deliveries.
    try:
        if get_group_state(group) != "idle":
            return group
        is_actor_sender = isinstance(find_actor(group, by), dict)
        if not by or by == "system" or is_actor_sender:
            return group
        group = set_group_state(group, state="active")
        try:
            automation_on_resume(group)
        except Exception:
            pass
        try:
            clear_pending_system_notifies(
                group.group_id,
                {"nudge", "keepalive", "help_nudge", "actor_idle", "silence_check", "automation"},
            )
        except Exception:
            pass
        return group
    except Exception:
        return group


def _build_delivery_text(
    *,
    text: str,
    priority: str,
    reply_required: bool,
    event_id: str,
    attachments: list[dict[str, Any]],
    src_group_id: str = "",
    src_event_id: str = "",
) -> str:
    delivery_text = text
    prefix_lines: list[str] = []
    if priority == "attention" and event_id:
        prefix_lines.append(f"[cccc] IMPORTANT (event_id={event_id}):")
    if reply_required and event_id:
        prefix_lines.append(f"[cccc] REPLY REQUIRED (event_id={event_id}): reply via cccc_message_reply.")
    if src_group_id and src_event_id:
        prefix_lines.append(f"[cccc] RELAYED FROM (group_id={src_group_id}, event_id={src_event_id}):")
    if prefix_lines:
        delivery_text = "\n".join(prefix_lines) + "\n" + delivery_text
    if attachments:
        lines = ["[cccc] Attachments:"]
        for attachment in attachments[:8]:
            title = str(attachment.get("title") or attachment.get("path") or "file").strip()
            size_bytes = int(attachment.get("bytes") or 0)
            rel_path = str(attachment.get("path") or "").strip()
            lines.append(f"- {title} ({size_bytes} bytes) [{rel_path}]")
        if len(attachments) > 8:
            lines.append(f"- … ({len(attachments) - 8} more)")
        delivery_text = (delivery_text.rstrip("\n") + "\n\n" + "\n".join(lines)).strip()
    return delivery_text


def _touch_registry_updated_at(group_id: str, ts: str) -> None:
    try:
        reg = load_registry()
        meta = reg.groups.get(group_id)
        if isinstance(meta, dict):
            meta["updated_at"] = str(ts or utc_now_iso())
            reg.save()
    except Exception:
        pass


def _notify_headless_targets(
    *,
    group: Any,
    by: str,
    event_id: str,
    priority: str,
    reply_required: bool,
    event: dict[str, Any],
) -> None:
    try:
        headless_targets = get_headless_targets_for_message(group, event=event, by=by)
        if reply_required:
            notify_title = "Task message"
            notify_priority = "urgent" if priority == "attention" else "high"
        else:
            notify_title = "Important message" if priority == "attention" else "New message"
            notify_priority = "urgent" if priority == "attention" else "high"
        for actor_id in headless_targets:
            append_event(
                group.ledger_path,
                kind="system.notify",
                group_id=group.group_id,
                scope_key="",
                by="system",
                data={
                    "kind": "info",
                    "priority": notify_priority,
                    "title": notify_title,
                    "message": f"New message from {by}. Check your inbox.",
                    "target_actor_id": actor_id,
                    "requires_ack": False,
                    "context": {"event_id": event_id, "from": by},
                },
            )
    except Exception:
        pass


def handle_send(
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
    normalize_attachments: Callable[[Any, Any], list[dict[str, Any]]],
    effective_runner_kind: Callable[[str], str],
    auto_wake_recipients: Callable[[Any, list[str], str], list[str]],
    automation_on_resume: Callable[[Any], None],
    automation_on_new_message: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str, set[str]], None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    text = str(args.get("text") or "")
    by = str(args.get("by") or "user").strip()
    priority = str(args.get("priority") or "normal").strip() or "normal"
    reply_required = coerce_bool(args.get("reply_required"))
    src_group_id = str(args.get("src_group_id") or "").strip()
    src_event_id = str(args.get("src_event_id") or "").strip()
    dst_group_id = str(args.get("dst_group_id") or "").strip()
    dst_to_raw = args.get("dst_to")
    dst_to: list[str] = []
    if isinstance(dst_to_raw, list):
        dst_to = [str(x).strip() for x in dst_to_raw if isinstance(x, str) and str(x).strip()]
    if (src_group_id and not src_event_id) or (src_event_id and not src_group_id):
        src_group_id = ""
        src_event_id = ""
    to_raw = args.get("to")
    to_tokens: list[str] = []
    if isinstance(to_raw, list):
        to_tokens = [str(x).strip() for x in to_raw if isinstance(x, str) and str(x).strip()]

    if priority not in ("normal", "attention"):
        return _error("invalid_priority", "priority must be 'normal' or 'attention'")
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    group = _wake_group_on_human_message(
        group,
        by=by,
        automation_on_resume=automation_on_resume,
        clear_pending_system_notifies=clear_pending_system_notifies,
    )

    try:
        to = resolve_recipient_tokens(group, to_tokens)
    except Exception as e:
        return _error("invalid_recipient", str(e))

    if not to:
        mention_pattern = re.compile(r"@(\w[\w-]*)")
        mentions = mention_pattern.findall(text)
        if mentions:
            actors = list_actors(group)
            actor_ids = {str(actor.get("id") or "") for actor in actors if isinstance(actor, dict)}
            valid_mentions = [m for m in mentions if m in actor_ids or m in ("all", "peers", "foreman")]
            if valid_mentions:
                mention_tokens = [f"@{m}" if m in ("all", "peers", "foreman") else m for m in valid_mentions]
                try:
                    to = resolve_recipient_tokens(group, mention_tokens)
                except Exception:
                    pass

    if not to and get_default_send_to(group.doc) == "foreman":
        to = ["@foreman"]

    if targets_any_agent(to):
        matched_enabled = enabled_recipient_actor_ids(group, to)
        if by and by in matched_enabled:
            matched_enabled = [actor_id for actor_id in matched_enabled if actor_id != by]
        if not matched_enabled:
            woken = auto_wake_recipients(group, to, by)
            if not woken:
                wanted = " ".join(to) if to else "@all"
                return _error(
                    "no_enabled_recipients",
                    f"no enabled agents match recipients: {wanted}",
                    details={"to": list(to)},
                )

    path = str(args.get("path") or "").strip()
    if path:
        scope = detect_scope(Path(path))
        scope_key = scope.scope_key
        scopes = group.doc.get("scopes")
        attached = False
        if isinstance(scopes, list):
            attached = any(isinstance(item, dict) and item.get("scope_key") == scope_key for item in scopes)
        if not attached:
            return _error(
                "scope_not_attached",
                f"scope not attached: {scope_key}",
                details={"hint": "cccc attach <path> --group <id>"},
            )
    else:
        scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if not scope_key:
        scope_key = ""

    try:
        attachments = normalize_attachments(group, args.get("attachments"))
    except Exception as e:
        return _error("invalid_attachments", str(e))

    if not text.strip() and not attachments:
        return _error("empty_message", "message text cannot be empty")

    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data=ChatMessageData(
            text=text,
            format="plain",
            priority=priority,
            reply_required=reply_required,
            to=to,
            attachments=attachments,
            src_group_id=src_group_id or None,
            src_event_id=src_event_id or None,
            dst_group_id=dst_group_id or None,
            dst_to=dst_to if dst_group_id else None,
        ).model_dump(),
    )
    _touch_registry_updated_at(group.group_id, str(event.get("ts") or utc_now_iso()))

    effective_to = to if to else ["@all"]
    event_id = str(event.get("id") or "").strip()
    event_ts = str(event.get("ts") or "").strip()
    delivery_text = _build_delivery_text(
        text=text,
        priority=priority,
        reply_required=reply_required,
        event_id=event_id,
        attachments=attachments,
        src_group_id=src_group_id,
        src_event_id=src_event_id,
    )
    actors = list_actors(group)
    logger.debug(f"[SEND] group={group_id} text={text[:30]!r} actors={[a.get('id') for a in actors]} effective_to={effective_to}")
    for actor in actors:
        if not isinstance(actor, dict):
            continue
        actor_id = str(actor.get("id") or "").strip()
        if not actor_id or actor_id == "user" or actor_id == by:
            logger.debug(f"[SEND] skip actor={actor_id} (user/by)")
            continue
        event_with_effective_to = dict(event)
        event_with_effective_to["data"] = dict(event.get("data") or {})
        event_with_effective_to["data"]["to"] = effective_to
        if not is_message_for_actor(group, actor_id=actor_id, event=event_with_effective_to):
            logger.debug(f"[SEND] skip actor={actor_id} (not for actor)")
            continue
        runner_kind = str(actor.get("runner") or "pty").strip()
        if effective_runner_kind(runner_kind) == "pty":
            queue_chat_message(
                group,
                actor_id=actor_id,
                event_id=event_id,
                by=by,
                to=effective_to,
                text=delivery_text,
                ts=event_ts,
            )

    event_for_headless = dict(event)
    event_for_headless["data"] = dict(event.get("data") or {})
    event_for_headless["data"]["to"] = effective_to
    _notify_headless_targets(
        group=group,
        by=by,
        event_id=event_id,
        priority=priority,
        reply_required=reply_required,
        event=event_for_headless,
    )

    try:
        automation_on_new_message(group)
    except Exception:
        pass

    return DaemonResponse(ok=True, result={"event": event})


def handle_reply(
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
    normalize_attachments: Callable[[Any, Any], list[dict[str, Any]]],
    effective_runner_kind: Callable[[str], str],
    auto_wake_recipients: Callable[[Any, list[str], str], list[str]],
    automation_on_resume: Callable[[Any], None],
    automation_on_new_message: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str, set[str]], None],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    text = str(args.get("text") or "")
    by = str(args.get("by") or "user").strip()
    reply_to = str(args.get("reply_to") or "").strip()
    priority = str(args.get("priority") or "normal").strip() or "normal"
    reply_required = coerce_bool(args.get("reply_required"))
    to_raw = args.get("to")
    to_tokens: list[str] = []
    if isinstance(to_raw, list):
        to_tokens = [str(x).strip() for x in to_raw if isinstance(x, str) and str(x).strip()]

    if priority not in ("normal", "attention"):
        return _error("invalid_priority", "priority must be 'normal' or 'attention'")
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not reply_to:
        return _error("missing_reply_to", "missing reply_to event_id")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    group = _wake_group_on_human_message(
        group,
        by=by,
        automation_on_resume=automation_on_resume,
        clear_pending_system_notifies=clear_pending_system_notifies,
    )

    original = find_event(group, reply_to)
    if original is None:
        return _error("event_not_found", f"event not found: {reply_to}")
    quote_text = get_quote_text(group, reply_to, max_len=100)

    if not to_tokens:
        to_tokens = default_reply_recipients(group, by=by, original_event=original)
    try:
        to = resolve_recipient_tokens(group, to_tokens)
    except Exception as e:
        return _error("invalid_recipient", str(e))

    if targets_any_agent(to):
        matched_enabled = enabled_recipient_actor_ids(group, to)
        if by and by in matched_enabled:
            matched_enabled = [actor_id for actor_id in matched_enabled if actor_id != by]
        if not matched_enabled:
            woken = auto_wake_recipients(group, to, by)
            if not woken:
                wanted = " ".join(to) if to else "@all"
                return _error(
                    "no_enabled_recipients",
                    f"no enabled agents match recipients: {wanted}",
                    details={"to": list(to)},
                )

    scope_key = str(group.doc.get("active_scope_key") or "").strip()
    try:
        attachments = normalize_attachments(group, args.get("attachments"))
    except Exception as e:
        return _error("invalid_attachments", str(e))
    if not text.strip() and not attachments:
        return _error("empty_message", "message text cannot be empty")

    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data=ChatMessageData(
            text=text,
            format="plain",
            priority=priority,
            reply_required=reply_required,
            to=to,
            reply_to=reply_to,
            quote_text=quote_text,
            attachments=attachments,
        ).model_dump(),
    )

    ack_event: Optional[dict[str, Any]] = None
    try:
        if str(original.get("kind") or "") == "chat.message":
            original_by = str(original.get("by") or "").strip()
            original_data = original.get("data") if isinstance(original.get("data"), dict) else {}
            original_priority = str(original_data.get("priority") or "normal").strip()
            if by and by != original_by and original_priority == "attention":
                if is_message_for_actor(group, actor_id=by, event=original):
                    target_event_id = str(original.get("id") or "").strip()
                    if target_event_id and not has_chat_ack(group, event_id=target_event_id, actor_id=by):
                        ack_event = append_event(
                            group.ledger_path,
                            kind="chat.ack",
                            group_id=group.group_id,
                            scope_key="",
                            by=by,
                            data={"actor_id": by, "event_id": target_event_id},
                        )
    except Exception:
        ack_event = None

    _touch_registry_updated_at(group.group_id, str(event.get("ts") or utc_now_iso()))

    effective_to = to if to else ["@all"]
    event_with_effective_to = dict(event)
    event_with_effective_to["data"] = dict(event.get("data") or {})
    event_with_effective_to["data"]["to"] = effective_to

    event_id = str(event.get("id") or "").strip()
    event_ts = str(event.get("ts") or "").strip()
    delivery_text = _build_delivery_text(
        text=text,
        priority=priority,
        reply_required=reply_required,
        event_id=event_id,
        attachments=attachments,
    )
    for actor in list_actors(group):
        if not isinstance(actor, dict):
            continue
        actor_id = str(actor.get("id") or "").strip()
        if not actor_id or actor_id == "user" or actor_id == by:
            continue
        if not is_message_for_actor(group, actor_id=actor_id, event=event_with_effective_to):
            continue
        runner_kind = str(actor.get("runner") or "pty").strip()
        if effective_runner_kind(runner_kind) == "pty":
            queue_chat_message(
                group,
                actor_id=actor_id,
                event_id=event_id,
                by=by,
                to=effective_to,
                text=delivery_text,
                reply_to=reply_to,
                quote_text=quote_text,
                ts=event_ts,
            )

    _notify_headless_targets(
        group=group,
        by=by,
        event_id=event_id,
        priority=priority,
        reply_required=reply_required,
        event=event_with_effective_to,
    )

    try:
        automation_on_new_message(group)
    except Exception:
        pass

    return DaemonResponse(ok=True, result={"event": event, "ack_event": ack_event})


def try_handle_chat_op(
    op: str,
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
    normalize_attachments: Callable[[Any, Any], list[dict[str, Any]]],
    effective_runner_kind: Callable[[str], str],
    auto_wake_recipients: Callable[[Any, list[str], str], list[str]],
    automation_on_resume: Callable[[Any], None],
    automation_on_new_message: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str, set[str]], None],
) -> Optional[DaemonResponse]:
    if op == "send":
        return handle_send(
            args,
            coerce_bool=coerce_bool,
            normalize_attachments=normalize_attachments,
            effective_runner_kind=effective_runner_kind,
            auto_wake_recipients=auto_wake_recipients,
            automation_on_resume=automation_on_resume,
            automation_on_new_message=automation_on_new_message,
            clear_pending_system_notifies=clear_pending_system_notifies,
        )
    if op == "reply":
        return handle_reply(
            args,
            coerce_bool=coerce_bool,
            normalize_attachments=normalize_attachments,
            effective_runner_kind=effective_runner_kind,
            auto_wake_recipients=auto_wake_recipients,
            automation_on_resume=automation_on_resume,
            automation_on_new_message=automation_on_new_message,
            clear_pending_system_notifies=clear_pending_system_notifies,
        )
    return None
