"""Chat send/reply operation handlers for daemon."""

from __future__ import annotations

import logging
import hashlib
import json
import mimetypes
import uuid
from pathlib import Path
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import (
    ChatMessageData,
    ChatStreamData,
    DaemonError,
    DaemonResponse,
    SUGGESTED_USER_MESSAGE_MAX_CHARS,
)
from ...kernel.actors import find_actor, resolve_recipient_tokens
from ...kernel.group import get_group_state, load_group, set_group_state
from ...kernel.chat_idempotency import find_existing_reply_result
from ...kernel.inbox import actor_existed_at_event, find_event, is_message_for_actor, iter_events
from ...kernel.context import ContextStorage
from ...kernel.ledger import append_event, read_last_lines
from ...kernel.blobs import store_blob_bytes
from ...kernel.messaging import (
    default_reply_recipients,
    recipient_actor_ids,
    targets_any_agent,
)
from ...kernel.peer_insight import (
    PeerRecipientError,
    normalized_insight_or_error,
    peer_insight_required_details,
    preflight_local_peer_audience,
    remote_recipients_include_peer,
    validate_message_audience,
)
from ...kernel.message_sender_snapshot import build_sender_snapshot
from ...kernel.scope import detect_scope
from ...util.time import utc_now_iso
from ..group_bridge.reply_relay import (
    can_relay_group_bridge_reply,
    default_group_bridge_reply_recipients,
    group_bridge_reply_return_recipients,
    relay_group_bridge_reply,
)
from ..group_bridge.cancellation import propagate_reply_request_cancel
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from ..actors.web_model_browser_delivery import web_model_browser_delivery_enabled
from .delivery import flush_pending_messages
from .chat_delivery_ops import deliver_appended_chat_message
from .actor_turn_rendering import (
    build_actor_headless_delivery_text as _build_headless_delivery_text,
    compact_delivery_text as _compact_delivery_text,
)
from ..context.context_ops import handle_context_sync
from .install_slash_command import INSTALL_CAPABILITY_ID, parse_install_slash_command, render_install_command_task
from .chat_side_effects import schedule_chat_side_effects
from .post_commit import run_chat_post_commit, run_group_chat_post_commit
from .chat_diagnostics import make_chat_diagnostics
from .runtime_delivery import append_delivery_state, claim_deliveries, latest_delivery_state

logger = logging.getLogger("cccc.daemon.server")


def _normalize_suggested_user_message(value: Any) -> Optional[str]:
    text = str(value or "").strip()
    if not text:
        return None
    return text[:SUGGESTED_USER_MESSAGE_MAX_CHARS]


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))

def _wake_group_on_human_message(
    group: Any,
    *,
    by: str,
    targets_agent: bool = True,
    state_at_accept: str = "",
    automation_on_resume: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str, set[str]], None],
) -> Any:
    # Keep idle stable against agent chatter / throttled deliveries.
    try:
        accept_state = str(state_at_accept or "").strip().lower()
        explicit_user_wake = str(by or "").strip() == "user" and targets_agent
        if accept_state and accept_state != "idle" and not (
            accept_state in {"paused", "stopped"} and explicit_user_wake
        ):
            return group
        current_state = get_group_state(group)
        if current_state not in {"idle", "paused", "stopped"}:
            return group
        if current_state in {"paused", "stopped"} and not explicit_user_wake:
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


def _tracked_send_existing_result(group: Any, *, client_id: str, by: str = "") -> Optional[Dict[str, Any]]:
    if not client_id:
        return None
    sender = str(by or "").strip()
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
        if sender and str(event.get("by") or "").strip() != sender:
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
            "message_mode": str(data.get("message_mode") or ""),
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
    diagnostics_enabled: Callable[[], bool] | None = None,
    preflight_only: bool = False,
    has_attachments: bool = False,
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    text = str(args.get("text") or "")
    by = str(args.get("by") or "user").strip()
    message_mode = str(args.get("message_mode") or "").strip()
    reply_to = str(args.get("reply_to") or "").strip()
    quote_text = str(args.get("quote_text") or "").strip()
    src_group_id = str(args.get("src_group_id") or "").strip()
    src_event_id = str(args.get("src_event_id") or "").strip()
    dst_group_id = str(args.get("dst_group_id") or "").strip()
    dst_message_mode = str(args.get("dst_message_mode") or "").strip()
    client_id = str(args.get("client_id") or "").strip()
    suggested_user_message = _normalize_suggested_user_message(args.get("suggested_user_message"))
    source_platform = str(args.get("source_platform") or "").strip()
    source_user_name = str(args.get("source_user_name") or "").strip()
    source_user_id = str(args.get("source_user_id") or "").strip()
    source_multiaddrs_raw = args.get("source_multiaddrs")
    source_multiaddrs = (
        [str(item).strip() for item in source_multiaddrs_raw if str(item).strip()]
        if isinstance(source_multiaddrs_raw, list)
        else []
    )
    diag = make_chat_diagnostics(
        op="send",
        group_id=group_id,
        client_id=client_id,
        diagnostics_enabled=diagnostics_enabled,
        logger=logger,
    )
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
    install_slash_command = parse_install_slash_command(text)

    legacy_fields = [key for key in ("priority", "reply_required", "requires_ack") if key in args]
    if legacy_fields:
        return diag.finish_response(
            _error(
                "unsupported_message_fields",
                "use message_mode; legacy priority/reply_required/requires_ack fields are not supported",
                details={"fields": legacy_fields},
            )
        )
    if message_mode not in ("send", "request_reply", "mail"):
        return diag.finish_response(
            _error(
                "invalid_message_mode",
                "message_mode is required and must be send, request_reply, or mail",
            )
        )
    if dst_message_mode and dst_message_mode not in ("send", "request_reply", "mail"):
        return diag.finish_response(
            _error(
                "invalid_message_mode",
                "dst_message_mode must be send, request_reply, or mail",
            )
        )
    if dst_group_id and dst_message_mode:
        try:
            validate_message_audience(dst_to, message_mode=dst_message_mode)
        except PeerRecipientError as exc:
            return diag.finish_response(_error(exc.code, exc.message, details=exc.details))
    if not group_id:
        return diag.finish_response(_error("missing_group_id", "missing group_id"))

    group = load_group(group_id)
    diag.mark("load_group")
    if group is None:
        resp = _error("group_not_found", f"group not found: {group_id}")
        return diag.finish_response(resp)
    if client_id:
        existing = _tracked_send_existing_result(group, client_id=client_id, by=by)
        if existing is not None:
            return diag.finish_response(DaemonResponse(ok=True, result=existing))

    try:
        insight = normalized_insight_or_error(args.get("insight"))
    except ValueError as exc:
        return diag.finish_response(_error("invalid_insight", str(exc)))
    try:
        audience = preflight_local_peer_audience(
            group,
            to_tokens=to_tokens,
            by=by,
            apply_default_send=message_mode != "request_reply",
            message_mode=message_mode,
        )
    except PeerRecipientError as exc:
        return diag.finish_response(_error(exc.code, exc.message, details=exc.details))
    to = audience.recipients
    if message_mode == "request_reply":
        if not to_tokens or any(token in {"@all", "@peers", "@foreman"} for token in to_tokens):
            return diag.finish_response(
                _error(
                    "concrete_recipients_required",
                    "request_reply requires one or more explicit concrete recipients",
                )
            )
    if coerce_bool(args.get("require_peer_insight")) and audience.peer_actor_ids and insight is None:
        return diag.finish_response(
            _error(
                "peer_insight_required",
                "Not sent: this peer-facing message is missing `insight`.",
                details=peer_insight_required_details(),
            )
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
            return diag.finish_response(
                _error(
                    "scope_not_attached",
                    f"scope not attached: {scope_key}",
                    details={"hint": "cccc attach <path> --group <id>"},
                )
            )
    else:
        scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if not scope_key:
        scope_key = ""

    try:
        attachments = normalize_attachments(group, args.get("attachments"))
    except Exception as e:
        return diag.finish_response(_error("invalid_attachments", str(e)))
    refs = _normalize_refs(args.get("refs"))
    if not text.strip() and not attachments and not has_attachments:
        return diag.finish_response(_error("empty_message", "message text cannot be empty"))
    if preflight_only:
        return diag.finish_response(DaemonResponse(ok=True, result={"ready": True}))

    if source_multiaddrs and src_group_id and source_user_id:
        try:
            from ..group_bridge.peer_address_sync import sync_group_bridge_peer_multiaddrs

            sync_group_bridge_peer_multiaddrs(
                group_id=group.group_id,
                remote_group_id=src_group_id,
                remote_peer_id=source_user_id,
                multiaddrs=source_multiaddrs,
            )
        except Exception:
            logger.exception(
                "[group_bridge] failed to sync source multiaddrs group=%s remote_group=%s peer=%s",
                group.group_id,
                src_group_id,
                source_user_id,
            )

    if message_mode != "mail":
        group = _wake_group_on_human_message(
            group,
            by=by,
            targets_agent=targets_any_agent(to),
            state_at_accept=str(args.get("__group_state_at_accept") or ""),
            automation_on_resume=automation_on_resume,
            clear_pending_system_notifies=clear_pending_system_notifies,
        )
        diag.mark("wake_group")

    diag.mark("resolve_recipients")

    woken: list[str] = []
    if message_mode != "mail" and targets_any_agent(to):
        woken = auto_wake_recipients(group, to, by)
        diag.mark("auto_wake")

    delivery_body_text = text
    if install_slash_command is not None:
        delivery_body_text = render_install_command_task(install_slash_command)
        refs = [
            *refs,
            {
                "kind": "text",
                "title": "slash_command",
                "command": "/install",
                "capability_id": INSTALL_CAPABILITY_ID,
                "args_text": install_slash_command.get("args_text", ""),
                "target": install_slash_command.get("target", ""),
                "target_kind": install_slash_command.get("target_kind", ""),
            },
        ]

    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data=ChatMessageData(
            text=text,
            format="plain",
            insight=insight,
            message_mode=message_mode,
            reply_to=reply_to or None,
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
            dst_message_mode=dst_message_mode if dst_group_id and dst_message_mode else None,
            client_id=client_id or None,
            suggested_user_message=suggested_user_message,
        ).model_dump(),
    )
    diag.mark("append_event")
    effective_to = to if to else ["@all"]
    event_id = str(event.get("id") or "").strip()
    event_ts = str(event.get("ts") or "").strip()
    logger.debug("[SEND] group=%s text=%r effective_to=%s", group_id, text[:30], effective_to)
    if message_mode != "mail":
        run_group_chat_post_commit(
            group_id,
            "send-delivery",
            lambda: deliver_appended_chat_message(
                group=group,
                event=event,
                by=by,
                effective_to=effective_to,
                text=delivery_body_text,
                insight=insight,
                message_mode=message_mode,
                refs=refs,
                attachments=attachments,
                quote_text=quote_text,
                source_platform=source_platform,
                source_user_name=source_user_name,
                source_user_id=source_user_id,
                src_group_id=src_group_id,
                src_event_id=src_event_id,
                effective_runner_kind=effective_runner_kind,
                codex_actor_running=codex_app_supervisor.actor_running,
                claude_actor_running=claude_app_supervisor.actor_running,
                codex_submit_user_message=codex_app_supervisor.submit_user_message,
                claude_submit_user_message=claude_app_supervisor.submit_user_message,
                woken=set(woken),
                logger=logger,
            ),
        )
        diag.mark("schedule_delivery")
    schedule_chat_side_effects(
        group=group,
        automation_on_new_message=automation_on_new_message,
    )
    diag.mark("schedule_side_effects")

    return diag.finish_response(
        DaemonResponse(ok=True, result={"event": event, "message_mode": message_mode})
    )


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
    legacy_fields = [key for key in ("priority", "message_priority", "reply_required") if key in args]
    if legacy_fields:
        return _error(
            "unsupported_message_fields",
            "tracked_send uses fixed message_mode=send; use task_priority for the task and omit priority/message_priority/reply_required",
            details={"fields": legacy_fields},
        )

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

    try:
        insight = normalized_insight_or_error(args.get("insight"))
    except ValueError as exc:
        return _error("invalid_insight", str(exc))
    try:
        audience = preflight_local_peer_audience(
            group,
            to_tokens=_normalize_to_tokens(args.get("to")),
            by=by,
            apply_default_send=True,
            message_mode="send",
        )
    except PeerRecipientError as exc:
        return _error(exc.code, exc.message, details=exc.details)
    if coerce_bool(args.get("require_peer_insight")) and audience.peer_actor_ids and insight is None:
        existing_task_id = str(getattr(existing_task, "id", "") or "").strip() if existing_task is not None else ""
        return _error(
            "peer_insight_required",
            "Not sent: this peer-facing message is missing `insight`.",
            details=peer_insight_required_details(existing_task_id=existing_task_id),
        )

    assignee = _derive_tracked_send_assignee(args)
    outcome = str(args.get("outcome") or args.get("goal") or "").strip() or text
    status = str(args.get("status") or "planned").strip() or "planned"
    waiting_on = str(args.get("waiting_on") or ("actor" if assignee else "none")).strip() or "none"
    priority = str(args.get("task_priority") or "normal").strip() or "normal"
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
        "to": audience.recipients,
        "path": str(args.get("path") or ""),
        "message_mode": "send",
        "refs": base_refs,
        "insight": insight,
        "require_peer_insight": coerce_bool(args.get("require_peer_insight")),
    }
    if "__group_state_at_accept" in args:
        message_args["__group_state_at_accept"] = str(args.get("__group_state_at_accept") or "")
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
            if send_resp.error is not None and send_resp.error.code == "peer_insight_required":
                return _error(
                    "peer_insight_required",
                    send_resp.error.message,
                    details=peer_insight_required_details(existing_task_id=existing_task_id),
                )
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
                "message_mode": "send",
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
            "message_mode": "send",
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
    diagnostics_enabled: Callable[[], bool] | None = None,
    preflight_only: bool = False,
    has_attachments: bool = False,
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    text = str(args.get("text") or "")
    by = str(args.get("by") or "user").strip()
    reply_to = str(args.get("reply_to") or "").strip()
    client_id = str(args.get("client_id") or "").strip()
    message_mode = str(args.get("message_mode") or "send").strip().lower()
    suggested_user_message = _normalize_suggested_user_message(args.get("suggested_user_message"))
    diag = make_chat_diagnostics(
        op="reply",
        group_id=group_id,
        client_id=client_id,
        reply_to=reply_to,
        diagnostics_enabled=diagnostics_enabled,
        logger=logger,
    )
    to_raw = args.get("to")
    to_tokens: list[str] = []
    if isinstance(to_raw, list):
        to_tokens = [str(x).strip() for x in to_raw if isinstance(x, str) and str(x).strip()]
    to_explicitly_set = bool(to_tokens)

    legacy_fields = [key for key in ("priority", "reply_required", "requires_ack") if key in args]
    if legacy_fields:
        return diag.finish_response(
            _error(
                "unsupported_message_fields",
                "reply accepts message_mode=send or mail; legacy delivery fields are not supported",
                details={"fields": legacy_fields},
            )
        )
    if message_mode not in {"send", "mail"}:
        return diag.finish_response(
            _error(
                "invalid_message_mode",
                "reply message_mode must be send or mail",
            )
        )
    if not group_id:
        return diag.finish_response(_error("missing_group_id", "missing group_id"))
    if not reply_to:
        return diag.finish_response(_error("missing_reply_to", "missing reply_to event_id"))

    group = load_group(group_id)
    diag.mark("load_group")
    if group is None:
        resp = _error("group_not_found", f"group not found: {group_id}")
        return diag.finish_response(resp)
    if client_id:
        existing = find_existing_reply_result(group, client_id=client_id, by=by, reply_to=reply_to)
        if existing is not None:
            return diag.finish_response(DaemonResponse(ok=True, result=existing))

    original = find_event(group, reply_to)
    diag.mark("load_reply_target")
    if original is None:
        resp = _error("event_not_found", f"event not found: {reply_to}")
        return diag.finish_response(resp)
    target_event_id = str(original.get("id") or "").strip()
    if client_id and target_event_id and target_event_id != reply_to:
        existing = find_existing_reply_result(group, client_id=client_id, by=by, reply_to=target_event_id or reply_to)
        if existing is not None:
            return diag.finish_response(DaemonResponse(ok=True, result=existing))
    try:
        insight = normalized_insight_or_error(args.get("insight"))
    except ValueError as exc:
        return diag.finish_response(_error("invalid_insight", str(exc)))
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
    relayable_group_bridge_reply = can_relay_group_bridge_reply(group_id=group.group_id, original_data=original_data)
    group_bridge_reply_to = default_group_bridge_reply_recipients(original_data) if relayable_group_bridge_reply else []

    if not to_tokens:
        if relayable_group_bridge_reply:
            if not group_bridge_reply_to:
                return diag.finish_response(
                    _error(
                        "missing_remote_recipient",
                        "Group Bridge replies require an explicit recipient. Please pass to=['user'], to=['@foreman'], or another recipient.",
                    )
                )
        else:
            to_tokens = default_reply_recipients(group, by=by, original_event=original)
    if relayable_group_bridge_reply:
        # `to` names the remote audience for a Group Bridge reply. The durable
        # source row is only a local human-visible audit record and must never
        # redeliver the outbound reply to similarly named local actors.
        to = ["user"]
        local_peer_actor_ids = []
    else:
        try:
            audience = preflight_local_peer_audience(
                group,
                to_tokens=to_tokens,
                by=by,
                apply_default_send=False,
                message_mode=message_mode,
            )
        except PeerRecipientError as exc:
            return diag.finish_response(_error(exc.code, exc.message, details=exc.details))
        to = audience.recipients
        local_peer_actor_ids = audience.peer_actor_ids
    diag.mark("resolve_recipients")

    group_bridge_remote_to = (
        group_bridge_reply_return_recipients(
            original_data=original_data,
            fallback=to_tokens,
            fallback_was_explicit=to_explicitly_set,
        )
        if relayable_group_bridge_reply
        else []
    )
    if relayable_group_bridge_reply:
        try:
            validate_message_audience(group_bridge_remote_to, message_mode=message_mode)
        except PeerRecipientError as exc:
            return diag.finish_response(_error(exc.code, exc.message, details=exc.details))
    peer_facing = bool(local_peer_actor_ids) or remote_recipients_include_peer(group_bridge_remote_to)
    if coerce_bool(args.get("require_peer_insight")) and peer_facing and insight is None:
        return diag.finish_response(
            _error(
                "peer_insight_required",
                "Not sent: this peer-facing message is missing `insight`.",
                details=peer_insight_required_details(),
            )
        )

    scope_key = str(group.doc.get("active_scope_key") or "").strip()
    try:
        attachments = normalize_attachments(group, args.get("attachments"))
    except Exception as e:
        return diag.finish_response(_error("invalid_attachments", str(e)))
    refs = _normalize_refs(args.get("refs"))
    if not text.strip() and not attachments and not has_attachments:
        return diag.finish_response(_error("empty_message", "message text cannot be empty"))
    if preflight_only:
        return diag.finish_response(DaemonResponse(ok=True, result={"ready": True}))

    if message_mode != "mail":
        group = _wake_group_on_human_message(
            group,
            by=by,
            targets_agent=targets_any_agent(to),
            state_at_accept=str(args.get("__group_state_at_accept") or ""),
            automation_on_resume=automation_on_resume,
            clear_pending_system_notifies=clear_pending_system_notifies,
        )
    diag.mark("wake_group")

    woken: list[str] = []
    if message_mode != "mail" and targets_any_agent(to):
        woken = auto_wake_recipients(group, to, by)
        diag.mark("auto_wake")

    group_bridge_remote_group_id = (
        str(original_data.get("src_group_id") or "").strip()
        if relayable_group_bridge_reply
        else ""
    )

    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data=ChatMessageData(
            text=text,
            format="plain",
            insight=insight,
            message_mode="send" if relayable_group_bridge_reply else message_mode,
            to=to,
            reply_to=target_event_id or reply_to,
            quote_text=quote_text,
            refs=refs,
            attachments=attachments,
            source_platform=original_source_platform or None,
            source_user_name=original_source_user_name or None,
            source_user_id=original_source_user_id or None,
            mention_user_ids=original_mention_user_ids or None,
            dst_group_id=group_bridge_remote_group_id or None,
            dst_to=group_bridge_remote_to or None,
            dst_message_mode=message_mode if relayable_group_bridge_reply else None,
            **build_sender_snapshot(group, by=by),
            client_id=client_id or None,
            suggested_user_message=suggested_user_message,
        ).model_dump(),
    )
    diag.mark("append_event")
    group_bridge_reply_result = relay_group_bridge_reply(
        group_id=group.group_id,
        original_data=original_data,
        reply_event_id=str(event.get("id") or ""),
        text=text,
        insight=insight,
        by=by,
        to=group_bridge_remote_to if relayable_group_bridge_reply else to,
        message_mode=message_mode,
        refs=refs,
        to_was_explicit=to_explicitly_set,
        require_peer_insight=coerce_bool(args.get("require_peer_insight")),
    )
    diag.mark("group_bridge_reply")

    effective_to = to if to else ["@all"]
    event_id = str(event.get("id") or "").strip()
    event_ts = str(event.get("ts") or "").strip()
    if message_mode != "mail":
        run_group_chat_post_commit(
            group_id,
            "reply-delivery",
            lambda: deliver_appended_chat_message(
                group=group,
                event=event,
                by=by,
                effective_to=effective_to,
                text=text,
                insight=insight,
                message_mode=message_mode,
                refs=refs,
                attachments=attachments,
                reply_to=target_event_id or reply_to,
                quote_text=quote_text,
                effective_runner_kind=effective_runner_kind,
                codex_actor_running=codex_app_supervisor.actor_running,
                claude_actor_running=claude_app_supervisor.actor_running,
                codex_submit_user_message=codex_app_supervisor.submit_user_message,
                claude_submit_user_message=claude_app_supervisor.submit_user_message,
                woken=set(woken),
                logger=logger,
            ),
        )
    diag.mark("schedule_delivery")
    schedule_chat_side_effects(
        group=group,
        automation_on_new_message=automation_on_new_message,
    )
    diag.mark("schedule_side_effects")

    result: Dict[str, Any] = {"event": event, "message_mode": message_mode}
    if group_bridge_reply_result is not None:
        result["group_bridge_reply"] = group_bridge_reply_result.result if group_bridge_reply_result.ok else {
            "error": group_bridge_reply_result.error.model_dump() if group_bridge_reply_result.error is not None else None
        }
    return diag.finish_response(DaemonResponse(ok=True, result=result))


def handle_message_upload_preflight(
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
    normalize_attachments: Callable[[Any, Any], list[dict[str, Any]]],
    effective_runner_kind: Callable[[str], str],
    auto_wake_recipients: Callable[[Any, list[str], str], list[str]],
    automation_on_resume: Callable[[Any], None],
    automation_on_new_message: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str, set[str]], None],
    diagnostics_enabled: Callable[[], bool] | None = None,
) -> DaemonResponse:
    """Validate a staged Web upload without creating durable state."""

    operation = str(args.get("operation") or "").strip()
    if operation == "send_cross_group":
        from ..ops.maintenance_ops import handle_send_cross_group

        return handle_send_cross_group(
            args,
            dispatch_send=lambda _op, _args: (
                _error("internal_error", "cross-group preflight attempted a send"),
                False,
            ),
            preflight_only=True,
            has_attachments=coerce_bool(args.get("has_attachments")),
        )
    if operation not in {"send", "reply"}:
        return _error("invalid_args", "operation must be send, reply, or send_cross_group")
    forwarded = dict(args)
    forwarded.pop("operation", None)
    has_attachments = coerce_bool(forwarded.pop("has_attachments", False))
    handler = handle_reply if operation == "reply" else handle_send
    response = handler(
        forwarded,
        coerce_bool=coerce_bool,
        normalize_attachments=normalize_attachments,
        effective_runner_kind=effective_runner_kind,
        auto_wake_recipients=auto_wake_recipients,
        automation_on_resume=automation_on_resume,
        automation_on_new_message=automation_on_new_message,
        clear_pending_system_notifies=clear_pending_system_notifies,
        diagnostics_enabled=diagnostics_enabled,
        preflight_only=True,
        has_attachments=has_attachments,
    )
    if not response.ok:
        return response
    result = response.result if isinstance(response.result, dict) else {}
    if result.get("ready") is True:
        return response
    return DaemonResponse(
        ok=True,
        result={"ready": False, "duplicate": True, "result": result},
    )


def handle_send_files(
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
    normalize_attachments: Callable[[Any, Any], list[dict[str, Any]]],
    effective_runner_kind: Callable[[str], str],
    auto_wake_recipients: Callable[[Any, list[str], str], list[str]],
    automation_on_resume: Callable[[Any], None],
    automation_on_new_message: Callable[[Any], None],
    clear_pending_system_notifies: Callable[[str, set[str]], None],
    diagnostics_enabled: Callable[[], bool] | None = None,
) -> DaemonResponse:
    """Upload active-scope files and send them through the normal chat path."""

    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    raw_paths = args.get("paths")
    if not isinstance(raw_paths, list) or not raw_paths:
        return _error("invalid_paths", "paths must be a non-empty list")
    if args.get("attachments") is not None:
        return _error(
            "invalid_attachments",
            "send_files owns attachments; do not provide attachment records",
        )

    by = str(args.get("by") or "user").strip()
    client_id = str(args.get("client_id") or "").strip()
    if client_id:
        existing = _tracked_send_existing_result(group, client_id=client_id, by=by)
        if existing is not None:
            return DaemonResponse(ok=True, result=existing)

    scope_key = str(group.doc.get("active_scope_key") or "").strip()
    scopes = group.doc.get("scopes")
    scope_url = ""
    if isinstance(scopes, list):
        for scope in scopes:
            if not isinstance(scope, dict):
                continue
            if str(scope.get("scope_key") or "").strip() == scope_key:
                scope_url = str(scope.get("url") or "").strip()
                break
    if not scope_key or not scope_url:
        return _error("missing_scope", "group has no active scope")

    root = Path(scope_url).expanduser().resolve()
    sources: list[tuple[Path, bytes]] = []
    for raw_path in raw_paths:
        value = str(raw_path or "").strip()
        if not value:
            return _error("invalid_path", "file path must not be empty")
        candidate = Path(value).expanduser()
        source = candidate.resolve() if candidate.is_absolute() else (root / candidate).resolve()
        try:
            source.relative_to(root)
        except ValueError:
            return _error(
                "invalid_path",
                "file path must be under the group's active scope root",
                details={"path": str(source)},
            )
        if not source.is_file():
            return _error("not_found", f"file not found: {source}")
        try:
            sources.append((source, source.read_bytes()))
        except OSError as exc:
            return _error("read_failed", str(exc), details={"path": str(source)})

    legacy_fields = [key for key in ("priority", "reply_required", "requires_ack") if key in args]
    if legacy_fields:
        return _error(
            "unsupported_message_fields",
            "use message_mode; legacy priority/reply_required/requires_ack fields are not supported",
            details={"fields": legacy_fields},
        )
    message_mode = str(args.get("message_mode") or "").strip()
    if message_mode not in ("send", "request_reply", "mail"):
        return _error(
            "invalid_message_mode",
            "message_mode is required and must be send, request_reply, or mail",
        )
    try:
        insight = normalized_insight_or_error(args.get("insight"))
    except ValueError as exc:
        return _error("invalid_insight", str(exc))
    try:
        audience = preflight_local_peer_audience(
            group,
            to_tokens=_normalize_to_tokens(args.get("to")),
            by=by,
            apply_default_send=message_mode != "request_reply",
            message_mode=message_mode,
        )
    except PeerRecipientError as exc:
        return _error(exc.code, exc.message, details=exc.details)
    if message_mode == "request_reply" and (
        not _normalize_to_tokens(args.get("to"))
        or any(token in {"@all", "@peers", "@foreman"} for token in _normalize_to_tokens(args.get("to")))
    ):
        return _error(
            "concrete_recipients_required",
            "request_reply requires one or more explicit concrete recipients",
        )
    if coerce_bool(args.get("require_peer_insight")) and audience.peer_actor_ids and insight is None:
        return _error(
            "peer_insight_required",
            "Not sent: this peer-facing message is missing `insight`.",
            details=peer_insight_required_details(),
        )

    attachments = [
        store_blob_bytes(
            group,
            data=data,
            filename=source.name,
            mime_type=mimetypes.guess_type(source.name)[0] or "application/octet-stream",
        )
        for source, data in sources
    ]
    send_args = dict(args)
    send_args.pop("paths", None)
    send_args["attachments"] = attachments
    send_args["path"] = str(root)
    if not str(send_args.get("text") or "").strip():
        names = ", ".join(str(item.get("title") or "file") for item in attachments)
        send_args["text"] = f"[files] {names}"
    return handle_send(
        send_args,
        coerce_bool=coerce_bool,
        normalize_attachments=normalize_attachments,
        effective_runner_kind=effective_runner_kind,
        auto_wake_recipients=auto_wake_recipients,
        automation_on_resume=automation_on_resume,
        automation_on_new_message=automation_on_new_message,
        clear_pending_system_notifies=clear_pending_system_notifies,
        diagnostics_enabled=diagnostics_enabled,
    )


def handle_reply_request_cancel(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    source_event_id = str(args.get("source_event_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not source_event_id:
        return _error("missing_source_event_id", "missing source_event_id")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    source = find_event(group, source_event_id)
    if source is None or str(source.get("kind") or "") != "chat.message":
        return _error("event_not_found", f"chat message not found: {source_event_id}")
    source_event_id = str(source.get("id") or "").strip()
    data = source.get("data") if isinstance(source.get("data"), dict) else {}
    effective_message_mode = str(data.get("dst_message_mode") or data.get("message_mode") or "")
    if effective_message_mode != "request_reply":
        return _error("not_a_reply_request", "source message does not request a reply")
    sender = str(source.get("by") or "").strip()
    if by != "user" and by != sender:
        return _error("permission_denied", "only the source sender or user may cancel a reply request")
    for event in iter_events(group.ledger_path):
        event_data = event.get("data") if isinstance(event.get("data"), dict) else {}
        if (
            str(event.get("kind") or "") == "chat.reply_request.cancelled"
            and str(event_data.get("source_event_id") or "").strip() == source_event_id
        ):
            propagation = propagate_reply_request_cancel(
                source_group=group,
                source_message=source,
                cancel_event=event,
            )
            return DaemonResponse(
                ok=True,
                result={"event": event, "already": True, "propagation": propagation},
            )
    event = append_event(
        group.ledger_path,
        kind="chat.reply_request.cancelled",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"source_event_id": source_event_id},
    )
    propagation = propagate_reply_request_cancel(
        source_group=group,
        source_message=source,
        cancel_event=event,
    )
    return DaemonResponse(
        ok=True,
        result={"event": event, "already": False, "propagation": propagation},
    )


def handle_message_deliver(
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
    effective_runner_kind: Callable[[str], str],
    auto_wake_recipients: Callable[[Any, list[str], str], list[str]],
    automation_on_resume: Callable[[Any], None] = lambda _group: None,
    clear_pending_system_notifies: Callable[[str, set[str]], None] = lambda _group_id, _kinds: None,
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    source_event_id = str(args.get("source_event_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    raw_actor_ids = args.get("actor_ids")
    actor_ids = (
        list(dict.fromkeys(str(item or "").strip() for item in raw_actor_ids if str(item or "").strip()))
        if isinstance(raw_actor_ids, list)
        else []
    )
    force_ambiguous = coerce_bool(args.get("force_ambiguous"))
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not source_event_id:
        return _error("missing_source_event_id", "missing source_event_id")
    if not actor_ids:
        return _error("concrete_recipients_required", "actor_ids must contain explicit recipients")
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")
    source = find_event(group, source_event_id)
    if source is None or str(source.get("kind") or "") != "chat.message":
        return _error("event_not_found", f"chat message not found: {source_event_id}")
    source_event_id = str(source.get("id") or "").strip()
    data = source.get("data") if isinstance(source.get("data"), dict) else {}
    message_mode = str(data.get("message_mode") or "")
    if message_mode not in {"send", "request_reply", "mail"}:
        return _error("legacy_message", "historical messages without message_mode cannot be delivered")
    sender = str(source.get("by") or "").strip()
    if by != "user" and by != sender:
        return _error("permission_denied", "only the source sender or user may request delivery")
    delivery_claims: list[tuple[str, str, str]] = []
    preclaimed_actors: dict[str, tuple[str, str]] = {}
    for actor_id in actor_ids:
        actor = find_actor(group, actor_id)
        if not isinstance(actor, dict):
            return _error("unknown_actor", f"unknown actor: {actor_id}")
        enabled_value = actor.get("enabled")
        if enabled_value is not None and not coerce_bool(enabled_value):
            return _error(
                "delivery_blocked",
                f"actor is stopped: {actor_id}",
                details={"actor_id": actor_id, "reason": "actor_disabled"},
            )
        if not actor_existed_at_event(group, actor=actor, event=source) or not is_message_for_actor(
            group, actor_id=actor_id, event=source
        ):
            return _error("event_not_for_actor", f"event is not addressed to actor: {actor_id}")
        actor_created_at = str(actor.get("created_at") or "").strip()
        claim_transport = "manual_request"
        if str(actor.get("runtime") or "").strip().lower() == "web_model":
            claim_transport = (
                "web_model_browser"
                if web_model_browser_delivery_enabled(group.group_id, actor)
                else "web_model_pull"
            )
        delivery_claims.append((actor_id, actor_created_at, claim_transport))
        preclaimed_actors[actor_id] = (actor_created_at, claim_transport)

    claimed, states = claim_deliveries(
        group,
        deliveries=delivery_claims,
        source_event_id=source_event_id,
        force_ambiguous=force_ambiguous,
    )
    if not claimed:
        actor_id, state = next(
            (
                (candidate, states.get(candidate, ""))
                for candidate in actor_ids
                if states.get(candidate, "") in {"claimed", "accepted"}
                or (states.get(candidate, "") == "ambiguous" and not force_ambiguous)
            ),
            (actor_ids[0], "claimed"),
        )
        if state == "accepted":
            return _error("already_delivered", f"message was already accepted for actor: {actor_id}")
        if state == "claimed":
            return _error(
                "delivery_in_progress",
                f"delivery is already in progress for actor: {actor_id}",
                details={"actor_id": actor_id},
            )
        if state == "ambiguous" and not force_ambiguous:
            return _error(
                "delivery_ambiguous",
                f"delivery may already have occurred for actor: {actor_id}",
                details={"actor_id": actor_id, "force_ambiguous_required": True},
            )

    def _settle_claims_failed(reason: str) -> None:
        for actor_id, (actor_created_at, transport) in preclaimed_actors.items():
            append_delivery_state(
                group,
                actor_id=actor_id,
                actor_created_at=actor_created_at,
                source_event_id=source_event_id,
                state="failed",
                transport=transport,
                reason=reason,
            )

    if get_group_state(group) in {"paused", "stopped"}:
        try:
            group = set_group_state(group, state="active")
        except Exception as exc:
            _settle_claims_failed(f"group resume failed: {exc}")
            return _error("delivery_failed", f"group resume failed: {exc}")
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

    try:
        woken = auto_wake_recipients(group, actor_ids, by)
    except Exception as exc:
        _settle_claims_failed(f"recipient wake failed: {exc}")
        return _error("delivery_failed", f"recipient wake failed: {exc}")
    delivery_event = dict(source)
    delivery_event["data"] = dict(data)
    delivery_event["data"]["to"] = actor_ids

    def _deliver_preclaimed() -> None:
        try:
            deliver_appended_chat_message(
                group=group,
                event=delivery_event,
                by=sender,
                effective_to=actor_ids,
                text=str(data.get("text") or ""),
                insight=data.get("insight") if isinstance(data.get("insight"), str) else None,
                message_mode=message_mode,
                refs=[item for item in data.get("refs", []) if isinstance(item, dict)]
                if isinstance(data.get("refs"), list)
                else [],
                attachments=[item for item in data.get("attachments", []) if isinstance(item, dict)]
                if isinstance(data.get("attachments"), list)
                else [],
                reply_to=str(data.get("reply_to") or ""),
                quote_text=str(data.get("quote_text") or ""),
                source_platform=str(data.get("source_platform") or ""),
                source_user_name=str(data.get("source_user_name") or ""),
                source_user_id=str(data.get("source_user_id") or ""),
                src_group_id=str(data.get("src_group_id") or ""),
                src_event_id=str(data.get("src_event_id") or ""),
                effective_runner_kind=effective_runner_kind,
                woken=set(woken),
                force_ambiguous=force_ambiguous,
                preclaimed_actors=preclaimed_actors,
                logger=logger,
            )
        except Exception as exc:
            for actor_id, (actor_created_at, transport) in preclaimed_actors.items():
                existing = latest_delivery_state(
                    group,
                    actor_id=actor_id,
                    source_event_id=source_event_id,
                )
                existing_data = (
                    existing.get("data")
                    if isinstance(existing, dict) and isinstance(existing.get("data"), dict)
                    else {}
                )
                if str(existing_data.get("state") or "").strip() != "claimed":
                    continue
                append_delivery_state(
                    group,
                    actor_id=actor_id,
                    actor_created_at=actor_created_at,
                    source_event_id=source_event_id,
                    state="failed",
                    transport=transport,
                    reason=f"delivery worker failed: {exc}",
                )
            raise

    run_group_chat_post_commit(
        group_id,
        "manual-message-delivery",
        _deliver_preclaimed,
    )
    return DaemonResponse(
        ok=True,
        result={"event": source, "actor_ids": actor_ids, "delivery_state": "claimed"},
    )


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
    sender = find_actor(group, by)
    sender_title = (
        str(sender.get("title") or "").strip()
        if isinstance(sender, dict)
        else ""
    )

    data = ChatStreamData(
        stream_id=stream_id,
        op=op,
        text=text,
        format=fmt,
        seq=seq,
        to=to,
        reply_to=reply_to,
        sender_title=sender_title or None,
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
    diagnostics_enabled: Callable[[], bool] | None = None,
) -> Optional[DaemonResponse]:
    if op == "stream_emit":
        return handle_stream_emit(args)
    if op == "message_upload_preflight":
        return handle_message_upload_preflight(
            args,
            coerce_bool=coerce_bool,
            normalize_attachments=normalize_attachments,
            effective_runner_kind=effective_runner_kind,
            auto_wake_recipients=auto_wake_recipients,
            automation_on_resume=automation_on_resume,
            automation_on_new_message=automation_on_new_message,
            clear_pending_system_notifies=clear_pending_system_notifies,
            diagnostics_enabled=diagnostics_enabled,
        )
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
            diagnostics_enabled=diagnostics_enabled,
        )
    if op == "send_files":
        return handle_send_files(
            args,
            coerce_bool=coerce_bool,
            normalize_attachments=normalize_attachments,
            effective_runner_kind=effective_runner_kind,
            auto_wake_recipients=auto_wake_recipients,
            automation_on_resume=automation_on_resume,
            automation_on_new_message=automation_on_new_message,
            clear_pending_system_notifies=clear_pending_system_notifies,
            diagnostics_enabled=diagnostics_enabled,
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
    if op == "reply_request_cancel":
        return handle_reply_request_cancel(args)
    if op == "message_deliver":
        return handle_message_deliver(
            args,
            coerce_bool=coerce_bool,
            effective_runner_kind=effective_runner_kind,
            auto_wake_recipients=auto_wake_recipients,
            automation_on_resume=automation_on_resume,
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
            diagnostics_enabled=diagnostics_enabled,
        )
    return None
