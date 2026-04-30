"""Chat send/reply operation handlers for daemon."""

from __future__ import annotations

import logging
import hashlib
import json
import re
import uuid
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import ChatMessageData, ChatStreamData, DaemonError, DaemonResponse, SystemNotifyData
from ...kernel.actors import find_actor, list_actors, resolve_recipient_tokens
from ...kernel.group import get_group_state, load_group, set_group_state
from ...kernel.inbox import find_event_with_chat_ack, is_message_for_actor
from ...kernel.context import ContextStorage
from ...kernel.ledger import append_event, read_last_lines
from ...kernel.messaging import (
    default_reply_recipients,
    enabled_recipient_actor_ids,
    get_default_send_to,
    targets_any_agent,
)
from ...kernel.message_sender_snapshot import build_sender_snapshot
from ...kernel.scope import detect_scope
from ...kernel.pet_actor import PET_ACTOR_ID, get_pet_actor
from ...util.time import utc_now_iso
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from .delivery import (
    append_mcp_reply_reminder,
    emit_system_notify,
    flush_pending_messages,
    get_headless_targets_for_message,
    queue_chat_message,
    request_flush_pending_messages,
)
from .actor_delivery_planner import (
    TRANSPORT_CLAUDE_HEADLESS,
    TRANSPORT_CODEX_HEADLESS,
    TRANSPORT_PTY,
    TRANSPORT_WEB_MODEL_BROWSER,
    event_with_effective_to,
    plan_actor_chat_delivery,
)
from ..actors.web_model_browser_delivery import (
    schedule_web_model_browser_delivery,
    web_model_browser_delivery_enabled,
)
from .chat_support_ops import schedule_headless_post_wake_delivery
from .actor_turn_rendering import (
    build_actor_delivery_text as _build_delivery_text,
    build_actor_headless_delivery_text as _build_headless_delivery_text,
    compact_delivery_text as _compact_delivery_text,
)
from ..pet.review_scheduler import request_pet_review
from ..pet.profile_refresh import record_user_chat_message
from ..context.context_ops import handle_context_sync

logger = logging.getLogger("cccc.daemon.server")


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _is_internal_pet_sender(group: Any, by: str) -> bool:
    actor_id = str(by or "").strip()
    if actor_id != PET_ACTOR_ID:
        return False
    return isinstance(get_pet_actor(group), dict)


def _wake_group_on_human_message(
    group: Any,
    *,
    by: str,
    state_at_accept: str = "",
    automation_on_resume: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str, set[str]], None],
) -> Any:
    # Keep idle stable against agent chatter / throttled deliveries.
    try:
        accept_state = str(state_at_accept or "").strip().lower()
        if accept_state and accept_state != "idle":
            return group
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
                {"nudge", "keepalive", "help_nudge", "actor_idle", "silence_check", "auto_idle", "automation"},
            )
        except Exception:
            pass
        return group
    except Exception:
        return group


def _normalize_refs(raw: Any) -> list[dict[str, Any]]:
    if not isinstance(raw, list):
        return []
    refs: list[dict[str, Any]] = []
    for item in raw:
        if isinstance(item, dict):
            refs.append(item)
    return refs


def _normalize_to_tokens(raw: Any) -> list[str]:
    if isinstance(raw, list):
        return [str(item).strip() for item in raw if isinstance(item, str) and str(item).strip()]
    if isinstance(raw, str):
        token = raw.strip()
        return [token] if token else []
    return []


def _tracked_send_client_id(*, group_id: str, by: str, idempotency_key: str) -> str:
    basis = "\0".join([str(group_id or ""), str(by or ""), str(idempotency_key or "")])
    digest = hashlib.sha256(basis.encode("utf-8", errors="replace")).hexdigest()[:32]
    return f"tracked-send:{digest}"


def _tracked_send_existing_result(group: Any, *, client_id: str) -> Optional[Dict[str, Any]]:
    if not client_id:
        return None
    try:
        lines = read_last_lines(group.ledger_path, 800)
    except Exception:
        return None
    for raw_line in reversed(lines):
        try:
            event = json.loads(raw_line)
        except Exception:
            continue
        if not isinstance(event, dict) or str(event.get("kind") or "") != "chat.message":
            continue
        data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if str(data.get("client_id") or "").strip() != client_id:
            continue
        refs = data.get("refs") if isinstance(data.get("refs"), list) else []
        task_ref = next(
            (
                ref
                for ref in refs
                if isinstance(ref, dict)
                and str(ref.get("kind") or "").strip() == "task_ref"
                and str(ref.get("task_id") or "").strip()
            ),
            None,
        )
        task_id = str((task_ref or {}).get("task_id") or "").strip()
        return {
            "event": event,
            "event_id": str(event.get("id") or "").strip(),
            "task_id": task_id,
            "task_ref": task_ref,
            "replayed": True,
            "task_created": False,
            "message_sent": True,
            "partial_failure": False,
        }
    return None


def _tracked_send_existing_task(group: Any, *, client_request_id: str) -> Optional[Any]:
    if not client_request_id:
        return None
    try:
        storage = ContextStorage(group)
        tasks = storage.list_tasks()
    except Exception:
        return None
    matches = [
        task
        for task in tasks
        if str(getattr(task, "client_request_id", "") or "").strip() == client_request_id
    ]
    if not matches:
        return None
    matches.sort(
        key=lambda task: (
            str(getattr(task, "updated_at", "") or getattr(task, "created_at", "") or ""),
            str(getattr(task, "id", "") or ""),
        ),
        reverse=True,
    )
    return matches[0]


def _derive_tracked_send_assignee(args: Dict[str, Any]) -> str:
    explicit = str(args.get("assignee") or "").strip()
    if explicit:
        return explicit
    to_tokens = _normalize_to_tokens(args.get("to"))
    if len(to_tokens) != 1:
        return ""
    token = to_tokens[0].strip()
    if not token or token.startswith("@") or token == "user":
        return ""
    return token


def _normalize_tracked_checklist(raw: Any) -> Any:
    if raw is None:
        return None
    if isinstance(raw, list):
        out: list[Any] = []
        for item in raw:
            if isinstance(item, dict):
                text = str(item.get("text") or "").strip()
                if text:
                    out.append({**item, "text": text})
            else:
                text = str(item or "").strip()
                if text:
                    out.append({"text": text})
        return out
    text = str(raw or "").strip()
    if not text:
        return None
    return [{"text": line.strip()} for line in text.splitlines() if line.strip()]


def _task_ref(
    *,
    task_id: str,
    title: str,
    status: str = "planned",
    waiting_on: str = "none",
    handoff_to: str = "",
) -> dict[str, Any]:
    ref = {
        "kind": "task_ref",
        "task_id": task_id,
        "title": str(title or "").strip(),
        "status": str(status or "planned").strip() or "planned",
    }
    waiting_value = str(waiting_on or "").strip()
    if waiting_value:
        ref["waiting_on"] = waiting_value
    handoff_value = str(handoff_to or "").strip()
    if handoff_value:
        ref["handoff_to"] = handoff_value
    return ref


def _quote_text_from_message_data(data: dict[str, Any], *, max_len: int = 100) -> Optional[str]:
    text = data.get("text")
    if not isinstance(text, str):
        return None
    snippet = text.strip()
    if not snippet:
        return None
    if len(snippet) > max_len:
        return snippet[:max_len] + "..."
    return snippet


def _notify_headless_targets(
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
    quote_text = str(args.get("quote_text") or "").strip()
    src_group_id = str(args.get("src_group_id") or "").strip()
    src_event_id = str(args.get("src_event_id") or "").strip()
    dst_group_id = str(args.get("dst_group_id") or "").strip()
    client_id = str(args.get("client_id") or "").strip()
    source_platform = str(args.get("source_platform") or "").strip()
    source_user_name = str(args.get("source_user_name") or "").strip()
    source_user_id = str(args.get("source_user_id") or "").strip()
    mention_user_ids_raw = args.get("mention_user_ids")
    mention_user_ids = (
        [str(item).strip() for item in mention_user_ids_raw if str(item).strip()]
        if isinstance(mention_user_ids_raw, list)
        else []
    )
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
    elif isinstance(to_raw, str):
        token = to_raw.strip()
        if token:
            to_tokens = [token]
    to_explicitly_set = len(to_tokens) > 0

    if priority not in ("normal", "attention"):
        return _error("invalid_priority", "priority must be 'normal' or 'attention'")
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    if _is_internal_pet_sender(group, by):
        return _error(
            "pet_visible_chat_forbidden",
            "Pet cannot send or reply visible chat directly; use pet decisions instead.",
        )

    group = _wake_group_on_human_message(
        group,
        by=by,
        state_at_accept=str(args.get("__group_state_at_accept") or ""),
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

    if not to and not to_explicitly_set and get_default_send_to(group.doc) == "foreman":
        to = ["@foreman"]

    woken: list[str] = []
    if targets_any_agent(to):
        matched_enabled = enabled_recipient_actor_ids(group, to)
        if by and by in matched_enabled:
            matched_enabled = [actor_id for actor_id in matched_enabled if actor_id != by]
        woken = auto_wake_recipients(group, to, by)
        if not matched_enabled:
            if not woken:
                wanted = " ".join(to) if to else "@all"
                return _error(
                    "no_enabled_recipients",
                    (
                        "No enabled recipients after excluding sender. "
                        "Please specify 'to' explicitly, e.g. to=['user'], to=['@all'], or to=['peer-reviewer']. "
                        f"Current resolved recipients: {wanted}"
                    ),
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
    refs = _normalize_refs(args.get("refs"))

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
            quote_text=quote_text or None,
            to=to,
            refs=refs,
            attachments=attachments,
            source_platform=source_platform or None,
            source_user_name=source_user_name or None,
            source_user_id=source_user_id or None,
            mention_user_ids=mention_user_ids or None,
            **build_sender_snapshot(group, by=by),
            src_group_id=src_group_id or None,
            src_event_id=src_event_id or None,
            dst_group_id=dst_group_id or None,
            dst_to=dst_to if dst_group_id else None,
            client_id=client_id or None,
        ).model_dump(),
    )
    effective_to = to if to else ["@all"]
    event_id = str(event.get("id") or "").strip()
    event_ts = str(event.get("ts") or "").strip()
    delivery_text = _build_delivery_text(
        text=text,
        priority=priority,
        reply_required=reply_required,
        event_id=event_id,
        refs=refs,
        attachments=attachments,
        src_group_id=src_group_id,
        src_event_id=src_event_id,
    )
    headless_delivery_text = append_mcp_reply_reminder(
        _build_headless_delivery_text(
            by=by,
            to=effective_to,
            body=delivery_text,
            quote_text=quote_text,
            source_platform=source_platform,
            source_user_name=source_user_name,
            source_user_id=source_user_id,
        )
    )
    actors = list_actors(group)
    event_for_delivery = event_with_effective_to(event, effective_to)
    skip_headless_notify_actor_ids: set[str] = set()
    logger.debug(f"[SEND] group={group_id} text={text[:30]!r} actors={[a.get('id') for a in actors]} effective_to={effective_to}")
    for actor in actors:
        if not isinstance(actor, dict):
            continue
        decision = plan_actor_chat_delivery(
            group=group,
            actor=actor,
            event=event,
            by=by,
            effective_to=effective_to,
            effective_runner_kind=effective_runner_kind,
            codex_headless_running=codex_app_supervisor.actor_running,
            claude_headless_running=claude_app_supervisor.actor_running,
            web_model_browser_delivery_enabled=web_model_browser_delivery_enabled,
        )
        actor_id = decision.actor_id
        if decision.transport == TRANSPORT_CODEX_HEADLESS:
            delivered = bool(codex_app_supervisor.submit_user_message(
                group_id=group.group_id,
                actor_id=actor_id,
                text=headless_delivery_text,
                event_id=event_id,
                ts=event_ts,
                attachments=attachments,
            ))
            if delivered:
                skip_headless_notify_actor_ids.add(actor_id)
        elif decision.transport == TRANSPORT_CLAUDE_HEADLESS:
            delivered = bool(claude_app_supervisor.submit_user_message(
                group_id=group.group_id,
                actor_id=actor_id,
                text=headless_delivery_text,
                event_id=event_id,
                ts=event_ts,
                attachments=attachments,
            ))
            if delivered:
                skip_headless_notify_actor_ids.add(actor_id)
        elif decision.transport == TRANSPORT_PTY:
            queue_chat_message(
                group,
                actor_id=actor_id,
                event_id=event_id,
                by=by,
                to=effective_to,
                text=delivery_text,
                source_platform=source_platform or None,
                source_user_name=source_user_name or None,
                source_user_id=source_user_id or None,
                ts=event_ts,
            )
            request_flush_pending_messages(group, actor_id=actor_id)
        elif decision.transport == TRANSPORT_WEB_MODEL_BROWSER:
            if schedule_web_model_browser_delivery(
                group_id=group.group_id,
                actor_id=actor_id,
                trigger_event_id=event_id,
                logger=logger,
            ):
                skip_headless_notify_actor_ids.add(actor_id)
        else:
            if actor_id in woken and decision.reason in {"codex_headless_not_running", "claude_headless_not_running"}:
                if schedule_headless_post_wake_delivery(
                    group_id=group.group_id,
                    actor_id=actor_id,
                    runtime=decision.runtime,
                    text=headless_delivery_text,
                    event_id=event_id,
                    ts=event_ts,
                    attachments=attachments,
                    codex_actor_running=codex_app_supervisor.actor_running,
                    claude_actor_running=claude_app_supervisor.actor_running,
                    codex_submit_user_message=codex_app_supervisor.submit_user_message,
                    claude_submit_user_message=claude_app_supervisor.submit_user_message,
                    logger=logger,
                ):
                    skip_headless_notify_actor_ids.add(actor_id)
            logger.debug(f"[SEND] skip actor={actor_id} ({decision.reason})")

    _notify_headless_targets(
        group=group,
        by=by,
        event_id=event_id,
        priority=priority,
        reply_required=reply_required,
        event=event_for_delivery,
        skip_actor_ids=skip_headless_notify_actor_ids,
    )

    try:
        automation_on_new_message(group)
    except Exception:
        pass
    try:
        request_pet_review(
            group.group_id,
            reason="chat_message",
            source_event_id=event_id,
            immediate=reply_required,
        )
    except Exception:
        pass
    try:
        if by == "user":
            record_user_chat_message(
                group.group_id,
                event_id=event_id,
                ts=event_ts,
                text=text,
            )
    except Exception:
        pass

    return DaemonResponse(ok=True, result={"event": event})


def handle_tracked_send(
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
    """Create a task and send the linked chat message as one daemon-owned operation."""
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    title = str(args.get("title") or "").strip()
    text = str(args.get("text") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not title:
        title = _compact_delivery_text(text, limit=120)
    if not title:
        return _error("missing_title", "tracked_send requires a title or non-empty text")
    if not text:
        return _error("empty_message", "tracked_send message text cannot be empty")
    message_priority = str(args.get("message_priority") or args.get("priority") or "normal").strip() or "normal"
    if message_priority not in ("normal", "attention"):
        return _error("invalid_priority", "priority must be 'normal' or 'attention'")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    idempotency_key = str(args.get("idempotency_key") or args.get("client_request_id") or "").strip()
    client_id = _tracked_send_client_id(group_id=group_id, by=by, idempotency_key=idempotency_key) if idempotency_key else ""
    if client_id:
        existing = _tracked_send_existing_result(group, client_id=client_id)
        if existing is not None:
            return DaemonResponse(ok=True, result=existing)
        existing_task = _tracked_send_existing_task(group, client_request_id=client_id)
    else:
        existing_task = None

    assignee = _derive_tracked_send_assignee(args)
    outcome = str(args.get("outcome") or args.get("goal") or "").strip() or text
    status = str(args.get("status") or "planned").strip() or "planned"
    waiting_on = str(args.get("waiting_on") or ("actor" if assignee else "none")).strip() or "none"
    priority = str(args.get("task_priority") or message_priority).strip() or "normal"
    task_type = str(args.get("task_type") or "standard").strip() or "standard"
    checklist = _normalize_tracked_checklist(args.get("checklist"))
    notes = str(args.get("notes") or "").strip()
    blocked_by = args.get("blocked_by")
    handoff_to = str(args.get("handoff_to") or "").strip()
    base_refs = _normalize_refs(args.get("refs"))
    message_args = {
        "group_id": group_id,
        "text": text,
        "by": by,
        "to": _normalize_to_tokens(args.get("to")),
        "path": str(args.get("path") or ""),
        "priority": message_priority,
        "reply_required": coerce_bool(args.get("reply_required"), default=True) if "reply_required" in args else True,
        "refs": base_refs,
    }
    if client_id:
        message_args["client_id"] = client_id

    if existing_task is not None:
        existing_task_id = str(getattr(existing_task, "id", "") or "").strip()
        existing_title = str(getattr(existing_task, "title", "") or "").strip() or title
        existing_status = str(getattr(getattr(existing_task, "status", ""), "value", getattr(existing_task, "status", "")) or "planned").strip() or "planned"
        existing_waiting_on = str(getattr(getattr(existing_task, "waiting_on", ""), "value", getattr(existing_task, "waiting_on", "")) or "none").strip() or "none"
        existing_handoff_to = str(getattr(existing_task, "handoff_to", "") or "").strip()
        resumed_ref = _task_ref(
            task_id=existing_task_id,
            title=existing_title,
            status=existing_status,
            waiting_on=existing_waiting_on,
            handoff_to=existing_handoff_to,
        )
        message_args["refs"] = [*base_refs, resumed_ref]
        send_resp = handle_send(
            message_args,
            coerce_bool=coerce_bool,
            normalize_attachments=normalize_attachments,
            effective_runner_kind=effective_runner_kind,
            auto_wake_recipients=auto_wake_recipients,
            automation_on_resume=automation_on_resume,
            automation_on_new_message=automation_on_new_message,
            clear_pending_system_notifies=clear_pending_system_notifies,
        )
        if not send_resp.ok:
            err = send_resp.error.model_dump() if send_resp.error is not None else None
            return DaemonResponse(
                ok=True,
                result={
                    "task_id": existing_task_id,
                    "task_ref": resumed_ref,
                    "task_created": False,
                    "message_sent": False,
                    "partial_failure": True,
                    "message_error": err,
                    "recovered_from_partial_failure": False,
                },
            )
        send_result = send_resp.result if isinstance(send_resp.result, dict) else {}
        event = send_result.get("event") if isinstance(send_result.get("event"), dict) else {}
        return DaemonResponse(
            ok=True,
            result={
                "task_id": existing_task_id,
                "task_ref": resumed_ref,
                "event": event,
                "event_id": str(event.get("id") or "").strip(),
                "task_created": False,
                "message_sent": True,
                "partial_failure": False,
                "replayed": False,
                "recovered_from_partial_failure": True,
            },
        )

    task_op: dict[str, Any] = {
        "op": "task.create",
        "title": title,
        "outcome": outcome,
        "status": status,
        "priority": priority,
        "waiting_on": waiting_on,
        "task_type": task_type,
    }
    if client_id:
        task_op["client_request_id"] = client_id
    if assignee:
        task_op["assignee"] = assignee
    if notes:
        task_op["notes"] = notes
    if blocked_by is not None:
        task_op["blocked_by"] = blocked_by
    if handoff_to:
        task_op["handoff_to"] = handoff_to
    if checklist is not None:
        task_op["checklist"] = checklist

    task_resp = handle_context_sync({"group_id": group_id, "by": by, "ops": [task_op]})
    if not task_resp.ok:
        return task_resp
    task_result = task_resp.result if isinstance(task_resp.result, dict) else {}
    changes = task_result.get("changes") if isinstance(task_result.get("changes"), list) else []
    task_id = ""
    for change in changes:
        if isinstance(change, dict) and str(change.get("op") or "") == "task.create":
            task_id = str(change.get("task_id") or "").strip()
            if task_id:
                break
    if not task_id:
        return _error("tracked_send_task_missing", "task.create succeeded but did not return a task_id")

    ref = _task_ref(
        task_id=task_id,
        title=title,
        status=status,
        waiting_on=waiting_on,
        handoff_to=handoff_to,
    )
    message_args["refs"] = [*base_refs, ref]

    send_resp = handle_send(
        message_args,
        coerce_bool=coerce_bool,
        normalize_attachments=normalize_attachments,
        effective_runner_kind=effective_runner_kind,
        auto_wake_recipients=auto_wake_recipients,
        automation_on_resume=automation_on_resume,
        automation_on_new_message=automation_on_new_message,
        clear_pending_system_notifies=clear_pending_system_notifies,
    )
    if not send_resp.ok:
        err = send_resp.error.model_dump() if send_resp.error is not None else None
        return DaemonResponse(
            ok=True,
            result={
                "task_id": task_id,
                "task_ref": ref,
                "context_result": task_result,
                "task_created": True,
                "message_sent": False,
                "partial_failure": True,
                "message_error": err,
            },
        )
    send_result = send_resp.result if isinstance(send_resp.result, dict) else {}
    event = send_result.get("event") if isinstance(send_result.get("event"), dict) else {}
    return DaemonResponse(
        ok=True,
        result={
            "task_id": task_id,
            "task_ref": ref,
            "context_result": task_result,
            "event": event,
            "event_id": str(event.get("id") or "").strip(),
            "task_created": True,
            "message_sent": True,
            "partial_failure": False,
            "replayed": False,
        },
    )


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
    client_id = str(args.get("client_id") or "").strip()
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
    if _is_internal_pet_sender(group, by):
        return _error(
            "pet_visible_chat_forbidden",
            "Pet cannot send or reply visible chat directly; use pet decisions instead.",
        )

    group = _wake_group_on_human_message(
        group,
        by=by,
        state_at_accept=str(args.get("__group_state_at_accept") or ""),
        automation_on_resume=automation_on_resume,
        clear_pending_system_notifies=clear_pending_system_notifies,
    )

    original, existing_ack = find_event_with_chat_ack(group, event_id=reply_to, actor_id=by)
    if original is None:
        return _error("event_not_found", f"event not found: {reply_to}")
    target_event_id = str(original.get("id") or "").strip()
    original_data = original.get("data") if isinstance(original.get("data"), dict) else {}
    quote_text = _quote_text_from_message_data(original_data, max_len=100)
    original_source_platform = str(original_data.get("source_platform") or "").strip()
    original_source_user_name = str(original_data.get("source_user_name") or "").strip()
    original_source_user_id = str(original_data.get("source_user_id") or "").strip()
    original_mention_user_ids_raw = original_data.get("mention_user_ids")
    original_mention_user_ids = (
        [str(item).strip() for item in original_mention_user_ids_raw if str(item).strip()]
        if isinstance(original_mention_user_ids_raw, list)
        else []
    )

    if not to_tokens:
        to_tokens = default_reply_recipients(group, by=by, original_event=original)
    try:
        to = resolve_recipient_tokens(group, to_tokens)
    except Exception as e:
        return _error("invalid_recipient", str(e))

    woken: list[str] = []
    if targets_any_agent(to):
        matched_enabled = enabled_recipient_actor_ids(group, to)
        if by and by in matched_enabled:
            matched_enabled = [actor_id for actor_id in matched_enabled if actor_id != by]
        woken = auto_wake_recipients(group, to, by)
        if not matched_enabled:
            if not woken:
                wanted = " ".join(to) if to else "@all"
                return _error(
                    "no_enabled_recipients",
                    (
                        "No enabled recipients after excluding sender. "
                        "Please specify 'to' explicitly, e.g. to=['user'], to=['@all'], or to=['peer-reviewer']. "
                        f"Current resolved recipients: {wanted}"
                    ),
                    details={"to": list(to)},
                )

    scope_key = str(group.doc.get("active_scope_key") or "").strip()
    try:
        attachments = normalize_attachments(group, args.get("attachments"))
    except Exception as e:
        return _error("invalid_attachments", str(e))
    refs = _normalize_refs(args.get("refs"))
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
            reply_to=target_event_id or reply_to,
            quote_text=quote_text,
            refs=refs,
            attachments=attachments,
            source_platform=original_source_platform or None,
            source_user_name=original_source_user_name or None,
            source_user_id=original_source_user_id or None,
            mention_user_ids=original_mention_user_ids or None,
            **build_sender_snapshot(group, by=by),
            client_id=client_id or None,
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
                    if target_event_id and not existing_ack:
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

    effective_to = to if to else ["@all"]
    event_for_delivery = event_with_effective_to(event, effective_to)

    event_id = str(event.get("id") or "").strip()
    event_ts = str(event.get("ts") or "").strip()
    delivery_text = _build_delivery_text(
        text=text,
        priority=priority,
        reply_required=reply_required,
        event_id=event_id,
        refs=refs,
        attachments=attachments,
    )
    headless_delivery_text = append_mcp_reply_reminder(
        _build_headless_delivery_text(
            by=by,
            to=effective_to,
            body=delivery_text,
            reply_to=target_event_id or reply_to,
            quote_text=quote_text,
        )
    )
    skip_headless_notify_actor_ids: set[str] = set()
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
            codex_headless_running=codex_app_supervisor.actor_running,
            claude_headless_running=claude_app_supervisor.actor_running,
            web_model_browser_delivery_enabled=web_model_browser_delivery_enabled,
        )
        actor_id = decision.actor_id
        if decision.transport == TRANSPORT_CODEX_HEADLESS:
            delivered = bool(codex_app_supervisor.submit_user_message(
                group_id=group.group_id,
                actor_id=actor_id,
                text=headless_delivery_text,
                event_id=event_id,
                ts=event_ts,
                reply_to=target_event_id or reply_to,
                attachments=attachments,
            ))
            if delivered:
                skip_headless_notify_actor_ids.add(actor_id)
        elif decision.transport == TRANSPORT_CLAUDE_HEADLESS:
            delivered = bool(claude_app_supervisor.submit_user_message(
                group_id=group.group_id,
                actor_id=actor_id,
                text=headless_delivery_text,
                event_id=event_id,
                ts=event_ts,
                reply_to=target_event_id or reply_to,
                attachments=attachments,
            ))
            if delivered:
                skip_headless_notify_actor_ids.add(actor_id)
        elif decision.transport == TRANSPORT_PTY:
            queue_chat_message(
                group,
                actor_id=actor_id,
                event_id=event_id,
                by=by,
                to=effective_to,
                text=delivery_text,
                reply_to=target_event_id or reply_to,
                quote_text=quote_text,
                ts=event_ts,
            )
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
                reply_to=target_event_id or reply_to,
                attachments=attachments,
                codex_actor_running=codex_app_supervisor.actor_running,
                claude_actor_running=claude_app_supervisor.actor_running,
                codex_submit_user_message=codex_app_supervisor.submit_user_message,
                claude_submit_user_message=claude_app_supervisor.submit_user_message,
                logger=logger,
            ):
                skip_headless_notify_actor_ids.add(actor_id)

    _notify_headless_targets(
        group=group,
        by=by,
        event_id=event_id,
        priority=priority,
        reply_required=reply_required,
        event=event_for_delivery,
        skip_actor_ids=skip_headless_notify_actor_ids,
    )

    try:
        automation_on_new_message(group)
    except Exception:
        pass
    try:
        request_pet_review(
            group.group_id,
            reason="chat_reply",
            source_event_id=event_id,
            immediate=reply_required,
        )
    except Exception:
        pass
    try:
        if by == "user":
            record_user_chat_message(
                group.group_id,
                event_id=event_id,
                ts=event_ts,
                text=text,
            )
    except Exception:
        pass

    return DaemonResponse(ok=True, result={"event": event, "ack_event": ack_event})


def handle_stream_emit(args: Dict[str, Any]) -> DaemonResponse:
    """Handle chat.stream events (start/update/end)."""
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "").strip()
    op = str(args.get("op") or "").strip()

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not by:
        return _error("missing_by", "missing by")
    if op not in ("start", "update", "end"):
        return _error("invalid_op", "op must be 'start', 'update', or 'end'")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    stream_id = str(args.get("stream_id") or "").strip()
    if op == "start":
        stream_id = uuid.uuid4().hex
    elif not stream_id:
        return _error("missing_stream_id", "stream_id is required for update/end")

    text = str(args.get("text") or "")
    fmt = str(args.get("format") or "plain").strip() or "plain"
    seq = int(args.get("seq") or 0)
    to_raw = args.get("to")
    to: list[str] = []
    if isinstance(to_raw, list):
        to = [str(x).strip() for x in to_raw if isinstance(x, str) and str(x).strip()]
    reply_to = str(args.get("reply_to") or "").strip() or None
    client_id = str(args.get("client_id") or "").strip() or None

    data = ChatStreamData(
        stream_id=stream_id,
        op=op,
        text=text,
        format=fmt,
        seq=seq,
        to=to,
        reply_to=reply_to,
        client_id=client_id,
    )

    scope_key = str(group.doc.get("active_scope_key") or "").strip()
    event = append_event(
        group.ledger_path,
        kind="chat.stream",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data=data.model_dump(),
    )

    return DaemonResponse(ok=True, result={"event": event, "stream_id": stream_id})


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
    if op == "stream_emit":
        return handle_stream_emit(args)
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
    if op == "tracked_send":
        return handle_tracked_send(
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
