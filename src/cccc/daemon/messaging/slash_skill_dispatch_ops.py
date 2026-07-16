"""Hidden dispatch for CCCC slash capsule skills."""

from __future__ import annotations

import logging
from typing import Any, Callable, Dict, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import resolve_recipient_tokens
from ...kernel.chat_idempotency import find_existing_reply_result
from ...kernel.group import load_group
from ...kernel.ledger import append_event
from ...kernel.messaging import targets_any_agent
from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
from .actor_turn_rendering import SLASH_SKILL_CONTROL_KIND, build_actor_headless_delivery_text
from .chat_delivery_ops import deliver_chat_message
from .delivery import append_mcp_reply_reminder

logger = logging.getLogger("cccc.daemon.server")


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _normalize_to_tokens(raw: Any) -> list[str]:
    if isinstance(raw, list):
        return [str(item).strip() for item in raw if isinstance(item, str) and str(item).strip()]
    if isinstance(raw, str):
        token = raw.strip()
        return [token] if token else []
    return []


def _render_hidden_skill_turn(*, task_text: str, command: str, capability_id: str) -> str:
    return "\n\n".join(
        [
            "[CCCC] INTERNAL CONTROL: CCCC capability skill dispatch",
            (
                "Use the CCCC capability skill selected by the user. This is not a visible chat message. "
                "Do not inspect the host runtime's local Codex/Claude/Gemini skill list for this command."
            ),
            f"skill_command: {command}",
            f"capability_id: {capability_id}",
            (
                "Procedure: run `cccc_help` first to refresh Active Skills (Runtime); if needed, check "
                "`cccc_capability_state` to confirm active_capsule_skills; then execute the user task "
                "per that CCCC skill's runtime rules."
            ),
            f"User task:\n{task_text}",
        ]
    ).strip()


def handle_slash_skill_dispatch(
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
    effective_runner_kind: Callable[[str], str],
    auto_wake_recipients: Callable[[Any, list[str], str], list[str]],
) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "user").strip() or "user"
    task_text = str(args.get("task_text") or args.get("text") or "").strip()
    command = str(args.get("command") or "").strip()
    capability_id = str(args.get("capability_id") or "").strip()
    priority = str(args.get("priority") or "normal").strip() or "normal"
    reply_required = coerce_bool(args.get("reply_required"))
    reply_to = str(args.get("reply_to") or "").strip()
    quote_text = str(args.get("quote_text") or "").strip()
    client_id = str(args.get("client_id") or "").strip()

    if priority not in ("normal", "attention"):
        return _error("invalid_priority", "priority must be 'normal' or 'attention'")
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not task_text:
        return _error("empty_task", "slash skill task text cannot be empty")
    if not command:
        return _error("missing_command", "missing slash skill command")
    if not capability_id.startswith("skill:"):
        return _error("invalid_capability_id", "slash skill capability_id must start with skill:")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    to_tokens = _normalize_to_tokens(args.get("to"))
    try:
        effective_to = resolve_recipient_tokens(group, to_tokens)
    except Exception as e:
        return _error("invalid_recipient", str(e))
    if not effective_to:
        effective_to = ["@foreman"]
    if not targets_any_agent(effective_to):
        return _error("no_agent_recipients", "slash skill dispatch requires at least one actor recipient")

    if client_id:
        existing = find_existing_reply_result(group, client_id=client_id, by=by)
        if existing is not None:
            return DaemonResponse(
                ok=True,
                result={
                    "hidden": True,
                    "delivered": True,
                    "replayed": True,
                    "event_id": str(existing.get("event_id") or ""),
                    "command": command,
                    "capability_id": capability_id,
                    "to": effective_to,
                },
            )

    woken = auto_wake_recipients(group, effective_to, by)
    delivery_text = _render_hidden_skill_turn(
        task_text=task_text,
        command=command,
        capability_id=capability_id,
    )
    event_data = {
        "client_id": client_id,
        "text": delivery_text,
        "format": "plain",
        "priority": priority,
        "reply_required": reply_required,
        "reply_to": reply_to or None,
        "quote_text": quote_text or None,
        "to": effective_to,
        "refs": [
            {
                "kind": "text",
                "title": SLASH_SKILL_CONTROL_KIND,
                "hidden": True,
                "control_kind": SLASH_SKILL_CONTROL_KIND,
                "command": command,
                "capability_id": capability_id,
                "task_text": task_text,
            }
        ],
        "attachments": [],
    }
    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=str(group.doc.get("active_scope_key") or "").strip(),
        by=by,
        data=event_data,
    )
    event_id = str(event.get("id") or "").strip()
    event_ts = str(event.get("ts") or "").strip()
    headless_delivery_text = append_mcp_reply_reminder(
        build_actor_headless_delivery_text(
            by=by,
            to=effective_to,
            body=delivery_text,
            reply_to=reply_to,
            quote_text=quote_text,
        )
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
        priority=priority,
        reply_required=reply_required,
        effective_runner_kind=effective_runner_kind,
        codex_actor_running=codex_app_supervisor.actor_running,
        claude_actor_running=claude_app_supervisor.actor_running,
        codex_submit_user_message=codex_app_supervisor.submit_user_message,
        claude_submit_user_message=claude_app_supervisor.submit_user_message,
        woken=set(woken),
        logger=logger,
        reply_to=reply_to,
        quote_text=quote_text,
    )
    return DaemonResponse(
        ok=True,
        result={
            "hidden": True,
            "delivered": True,
            "event_id": event_id,
            "command": command,
            "capability_id": capability_id,
            "to": effective_to,
        },
    )


def try_handle_slash_skill_dispatch_op(
    op: str,
    args: Dict[str, Any],
    *,
    coerce_bool: Callable[[Any], bool],
    effective_runner_kind: Callable[[str], str],
    auto_wake_recipients: Callable[[Any, list[str], str], list[str]],
) -> Optional[DaemonResponse]:
    if op != "slash_skill_dispatch":
        return None
    return handle_slash_skill_dispatch(
        args,
        coerce_bool=coerce_bool,
        effective_runner_kind=effective_runner_kind,
        auto_wake_recipients=auto_wake_recipients,
    )
