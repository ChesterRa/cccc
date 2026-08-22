from __future__ import annotations

"""Messaging/inbox/ledger CLI command handlers."""

import os

from .common import *  # noqa: F401,F403

__all__ = [
    "cmd_send",
    "cmd_tracked_send",
    "cmd_reply",
    "cmd_deliver",
    "cmd_cancel_reply",
    "cmd_tail",
    "cmd_ledger_snapshot",
    "cmd_ledger_compact",
    "cmd_inbox",
    "cmd_prompt",
]


def _to_tokens_from_args(args: argparse.Namespace) -> list[str]:
    to_tokens: list[str] = []
    to_raw = getattr(args, "to", None)
    if isinstance(to_raw, list):
        for item in to_raw:
            if not isinstance(item, str):
                continue
            parts = [p.strip() for p in item.split(",") if p.strip()]
            to_tokens.extend(parts)
    return to_tokens


def _resolve_cli_message_sender(args: argparse.Namespace) -> str:
    explicit_by = str(getattr(args, "by", "") or "").strip()
    if explicit_by:
        return explicit_by
    actor_id = str(os.environ.get("CCCC_ACTOR_ID") or "").strip()
    if actor_id:
        return actor_id
    return "user"


def cmd_send(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = _resolve_cli_message_sender(args)

    to_tokens = _to_tokens_from_args(args)
    message_mode = str(getattr(args, "mode", "send") or "send").strip().replace("-", "_")
    if message_mode not in ("send", "request_reply", "mail"):
        _print_json({"ok": False, "error": {"code": "invalid_message_mode", "message": "mode must be send, request-reply, or mail"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "send requires the daemon"}})
        return 2
    resp = call_daemon(
        {
            "op": "send",
            "args": {
                "group_id": group_id,
                "text": args.text,
                "by": by,
                "path": str(args.path or ""),
                "to": to_tokens,
                "message_mode": message_mode,
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_tracked_send(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    task_priority = str(getattr(args, "task_priority", "normal") or "normal").strip() or "normal"
    checklist = [{"text": line.strip()} for line in str(getattr(args, "checklist", "") or "").splitlines() if line.strip()]
    by = _resolve_cli_message_sender(args)
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "tracked-send requires the daemon"}})
        return 2
    resp = call_daemon(
        {
            "op": "tracked_send",
            "args": {
                "group_id": group_id,
                "by": by,
                "title": str(getattr(args, "title", "") or ""),
                "text": str(getattr(args, "text", "") or ""),
                "to": _to_tokens_from_args(args),
                "outcome": str(getattr(args, "outcome", "") or ""),
                "checklist": checklist,
                "assignee": str(getattr(args, "assignee", "") or ""),
                "waiting_on": str(getattr(args, "waiting_on", "") or ""),
                "handoff_to": str(getattr(args, "handoff_to", "") or ""),
                "notes": str(getattr(args, "notes", "") or ""),
                "task_priority": task_priority,
                "idempotency_key": str(getattr(args, "idempotency_key", "") or ""),
            },
        }
    )
    if resp.get("ok"):
        _print_json(resp)
        return 0
    return _return_daemon_rejection(resp)

def cmd_reply(args: argparse.Namespace) -> int:
    """Reply to a message (IM-style, with quote)"""
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = _resolve_cli_message_sender(args)

    reply_to = str(args.event_id or "").strip()
    if not reply_to:
        _print_json({"ok": False, "error": {"code": "missing_event_id", "message": "missing event_id to reply to"}})
        return 2

    to_tokens: list[str] = []
    to_raw = getattr(args, "to", None)
    if isinstance(to_raw, list):
        for item in to_raw:
            if not isinstance(item, str):
                continue
            parts = [p.strip() for p in item.split(",") if p.strip()]
            to_tokens.extend(parts)

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "reply requires the daemon"}})
        return 2
    resp = call_daemon(
        {
            "op": "reply",
            "args": {
                "group_id": group_id,
                "text": args.text,
                "by": by,
                "reply_to": reply_to,
                "to": to_tokens,
                "message_mode": str(getattr(args, "mode", "send") or "send"),
            },
        }
    )
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def _call_message_control(args: argparse.Namespace, *, op: str) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    source_event_id = str(getattr(args, "event_id", "") or "").strip()
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not source_event_id:
        _print_json({"ok": False, "error": {"code": "missing_event_id", "message": "missing source event id"}})
        return 2
    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": f"{op} requires the daemon"}})
        return 2
    payload: dict[str, object] = {
        "group_id": group_id,
        "source_event_id": source_event_id,
        "by": _resolve_cli_message_sender(args),
    }
    if op == "message_deliver":
        actor_ids = _to_tokens_from_args(args)
        if not actor_ids:
            _print_json({"ok": False, "error": {"code": "concrete_recipients_required", "message": "deliver requires at least one --to actor id"}})
            return 2
        payload["actor_ids"] = actor_ids
        payload["force_ambiguous"] = bool(getattr(args, "force_ambiguous", False))
    resp = call_daemon({"op": op, "args": payload})
    _print_json(resp)
    return 0 if resp.get("ok") else 2


def cmd_deliver(args: argparse.Namespace) -> int:
    """Promote or retry an existing message without creating another message."""
    return _call_message_control(args, op="message_deliver")


def cmd_cancel_reply(args: argparse.Namespace) -> int:
    """Cancel the open reply obligations for an existing request."""
    return _call_message_control(args, op="reply_request_cancel")

def cmd_tail(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    if args.follow:
        for line in follow(group.ledger_path):
            print(line)
        return 0
    for line in read_last_lines(group.ledger_path, args.lines):
        print(line)
    return 0

def cmd_ledger_snapshot(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()
    reason = str(args.reason or "manual").strip()

    if _ensure_daemon_running():
        resp = call_daemon({"op": "ledger_snapshot", "args": {"group_id": group_id, "by": by, "reason": reason}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_group_permission(group, by=by, action="group.update")
        snap = snapshot_ledger(group, reason=reason)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "ledger_snapshot_failed", "message": str(e)}})
        return 2
    _print_json({"ok": True, "result": {"snapshot": snap}})
    return 0

def cmd_ledger_compact(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    by = str(args.by or "user").strip()
    reason = str(args.reason or "manual").strip()
    force = bool(args.force)

    if _ensure_daemon_running():
        resp = call_daemon(
            {"op": "ledger_compact", "args": {"group_id": group_id, "by": by, "reason": reason, "force": force}}
        )
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_group_permission(group, by=by, action="group.update")
        res = compact_ledger(group, reason=reason, force=force)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "ledger_compact_failed", "message": str(e)}})
        return 2
    _print_json({"ok": True, "result": res})
    return 0

def cmd_inbox(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()
    limit = int(args.limit) if isinstance(args.limit, int) else 50
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not actor_id:
        _print_json({"ok": False, "error": {"code": "missing_actor_id", "message": "missing actor_id"}})
        return 2

    if not _ensure_daemon_running():
        _print_json({"ok": False, "error": {"code": "daemon_unavailable", "message": "inbox requires the daemon"}})
        return 2
    resp = call_daemon({"op": "inbox_read", "args": {"group_id": group_id, "actor_id": actor_id, "by": by, "limit": limit}})
    _print_json(resp)
    return 0 if resp.get("ok") else 2

def cmd_prompt(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    actor_id = str(args.actor_id or "").strip()
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not actor_id:
        _print_json({"ok": False, "error": {"code": "missing_actor_id", "message": "missing actor id"}})
        return 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2

    actor = None
    for item in list_actors(group):
        if item.get("id") == actor_id:
            actor = item
            break
    if actor is None:
        _print_json({"ok": False, "error": {"code": "actor_not_found", "message": f"actor not found: {actor_id}"}})
        return 2
    prompt = render_system_prompt(group=group, actor=actor)

    _print_json({"ok": True, "result": {"group_id": group_id, "actor_id": actor_id, "prompt": prompt}})
    return 0
