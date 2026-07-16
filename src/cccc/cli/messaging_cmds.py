from __future__ import annotations

"""Messaging/inbox/ledger CLI command handlers."""

import os

from .common import *  # noqa: F401,F403

__all__ = [
    "cmd_send",
    "cmd_tracked_send",
    "cmd_reply",
    "cmd_tail",
    "cmd_ledger_snapshot",
    "cmd_ledger_compact",
    "cmd_inbox",
    "cmd_read",
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
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    by = _resolve_cli_message_sender(args)

    to_tokens = _to_tokens_from_args(args)
    priority = str(getattr(args, "priority", "normal") or "normal").strip() or "normal"
    if priority not in ("normal", "attention"):
        _print_json({"ok": False, "error": {"code": "invalid_priority", "message": "priority must be 'normal' or 'attention'"}})
        return 2
    reply_required = bool(getattr(args, "reply_required", False))

    if _ensure_daemon_running():
        resp = call_daemon(
            {
                "op": "send",
                "args": {
                    "group_id": group_id,
                    "text": args.text,
                    "by": by,
                    "path": str(args.path or ""),
                    "to": to_tokens,
                    "priority": priority,
                    "reply_required": reply_required,
                },
            }
        )
        if resp.get("ok"):
            _print_json(resp)
            return 0
        if not _daemon_response_allows_local_fallback(resp):
            return _return_daemon_rejection(resp)

    # Fallback: local execution (dev convenience)
    try:
        to = resolve_recipient_tokens(group, to_tokens)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "invalid_recipient", "message": str(e)}})
        return 2
    scope_key = str(group.doc.get("active_scope_key") or "")
    if args.path:
        scope = detect_scope(Path(args.path))
        scope_key = scope.scope_key
        scopes = group.doc.get("scopes")
        attached = False
        if isinstance(scopes, list):
            attached = any(isinstance(item, dict) and item.get("scope_key") == scope_key for item in scopes)
        if not attached:
            _print_json(
                {
                    "ok": False,
                    "error": {
                        "code": "scope_not_attached",
                        "message": f"scope not attached: {scope_key}",
                        "details": {"hint": "cccc attach <path> --group <id>"},
                    },
                }
            )
            return 2
    if not scope_key:
        scope_key = ""
    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data=ChatMessageData(
            text=args.text,
            format="plain",
            to=to,
            priority=priority,
            reply_required=reply_required,
        ).model_dump(),
    )
    try:
        reg = load_registry()
        meta = reg.groups.get(group.group_id)
        if isinstance(meta, dict):
            ts = str(event.get("ts") or meta.get("updated_at") or "")
            if ts:
                meta["updated_at"] = ts
                reg.save()
    except Exception:
        pass
    _print_json({"ok": True, "result": {"event": event}})
    return 0


def cmd_tracked_send(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    priority = str(getattr(args, "priority", "normal") or "normal").strip() or "normal"
    if priority not in ("normal", "attention"):
        _print_json({"ok": False, "error": {"code": "invalid_priority", "message": "priority must be 'normal' or 'attention'"}})
        return 2
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
                "priority": priority,
                "reply_required": not bool(getattr(args, "no_reply_required", False)),
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
    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    by = _resolve_cli_message_sender(args)

    reply_to = str(args.event_id or "").strip()
    if not reply_to:
        _print_json({"ok": False, "error": {"code": "missing_event_id", "message": "missing event_id to reply to"}})
        return 2

    # Find the original message to get quote_text
    original = find_event(group, reply_to)
    if original is None:
        _print_json({"ok": False, "error": {"code": "event_not_found", "message": f"event not found: {reply_to}"}})
        return 2

    quote_text = get_quote_text(group, reply_to, max_len=100)

    to_tokens: list[str] = []
    to_raw = getattr(args, "to", None)
    if isinstance(to_raw, list):
        for item in to_raw:
            if not isinstance(item, str):
                continue
            parts = [p.strip() for p in item.split(",") if p.strip()]
            to_tokens.extend(parts)

    priority = str(getattr(args, "priority", "normal") or "normal").strip() or "normal"
    if priority not in ("normal", "attention"):
        _print_json({"ok": False, "error": {"code": "invalid_priority", "message": "priority must be 'normal' or 'attention'"}})
        return 2
    reply_required = bool(getattr(args, "reply_required", False))

    if _ensure_daemon_running():
        resp = call_daemon(
            {
                "op": "reply",
                "args": {
                    "group_id": group_id,
                    "text": args.text,
                    "by": by,
                    "reply_to": reply_to,
                    "to": to_tokens,
                    "priority": priority,
                    "reply_required": reply_required,
                },
            }
        )
        if resp.get("ok"):
            _print_json(resp)
            return 0
        if not _daemon_response_allows_local_fallback(resp):
            return _return_daemon_rejection(resp)

    # Fallback: local execution
    if not to_tokens:
        to_tokens = default_reply_recipients(group, by=by, original_event=original)
    try:
        to = resolve_recipient_tokens(group, to_tokens)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "invalid_recipient", "message": str(e)}})
        return 2

    scope_key = str(group.doc.get("active_scope_key") or "")
    event = append_event(
        group.ledger_path,
        kind="chat.message",
        group_id=group.group_id,
        scope_key=scope_key,
        by=by,
        data=ChatMessageData(
            text=args.text,
            format="plain",
            to=to,
            priority=priority,
            reply_required=reply_required,
            reply_to=reply_to,
            quote_text=quote_text,
        ).model_dump(),
    )
    try:
        reg = load_registry()
        meta = reg.groups.get(group.group_id)
        if isinstance(meta, dict):
            ts = str(event.get("ts") or meta.get("updated_at") or "")
            if ts:
                meta["updated_at"] = ts
                reg.save()
    except Exception:
        pass
    _print_json({"ok": True, "result": {"event": event}})
    return 0

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
    kind_filter = str(getattr(args, "kind_filter", "all") or "all").strip()
    if kind_filter not in ("all", "chat", "notify"):
        kind_filter = "all"

    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not actor_id:
        _print_json({"ok": False, "error": {"code": "missing_actor_id", "message": "missing actor_id"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "inbox_list", "args": {"group_id": group_id, "actor_id": actor_id, "by": by, "limit": limit, "kind_filter": kind_filter}})
        if resp.get("ok") and not args.mark_read:
            _print_json(resp)
            return 0
        if resp.get("ok") and args.mark_read:
            result = resp.get("result") if isinstance(resp.get("result"), dict) else {}
            messages = result.get("messages") if isinstance(result.get("messages"), list) else []
            if messages:
                last_id = str((messages[-1] or {}).get("id") or "").strip()
                if last_id:
                    mark = call_daemon({"op": "inbox_mark_read", "args": {"group_id": group_id, "actor_id": actor_id, "event_id": last_id, "by": by}})
                    if mark.get("ok"):
                        _print_json({"ok": True, "result": {"messages": messages, "marked": mark.get("result", {})}})
                        return 0
            _print_json(resp)
            return 0

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "permission_denied", "message": str(e)}})
        return 2

    messages = unread_messages(group, actor_id=actor_id, limit=limit, kind_filter=kind_filter)  # type: ignore
    cur_event_id, cur_ts = get_cursor(group, actor_id)
    if args.mark_read and messages:
        last = messages[-1]
        last_id = str(last.get("id") or "").strip()
        last_ts = str(last.get("ts") or "")
        if last_id:
            cursor = set_cursor(group, actor_id, event_id=last_id, ts=last_ts)
            read_ev = append_event(
                group.ledger_path,
                kind="chat.read",
                group_id=group.group_id,
                scope_key="",
                by=by,
                data={"actor_id": actor_id, "event_id": last_id},
            )
            _print_json({"ok": True, "result": {"messages": messages, "cursor": cursor, "event": read_ev}})
            return 0

    _print_json({"ok": True, "result": {"messages": messages, "cursor": {"event_id": cur_event_id, "ts": cur_ts}}})
    return 0

def cmd_read(args: argparse.Namespace) -> int:
    group_id = _resolve_group_id(getattr(args, "group", ""))
    actor_id = str(args.actor_id or "").strip()
    by = str(args.by or "user").strip()
    event_id = str(args.event_id or "").strip()

    if not group_id:
        _print_json({"ok": False, "error": {"code": "missing_group_id", "message": "missing group_id (no active group?)"}})
        return 2
    if not actor_id:
        _print_json({"ok": False, "error": {"code": "missing_actor_id", "message": "missing actor_id"}})
        return 2
    if not event_id:
        _print_json({"ok": False, "error": {"code": "missing_event_id", "message": "missing event_id"}})
        return 2

    if _ensure_daemon_running():
        resp = call_daemon({"op": "inbox_mark_read", "args": {"group_id": group_id, "actor_id": actor_id, "event_id": event_id, "by": by}})
        _print_json(resp)
        return 0 if resp.get("ok") else 2

    group = load_group(group_id)
    if group is None:
        _print_json({"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}})
        return 2
    try:
        require_inbox_permission(group, by=by, target_actor_id=actor_id)
    except Exception as e:
        _print_json({"ok": False, "error": {"code": "permission_denied", "message": str(e)}})
        return 2
    ev = find_event(group, event_id)
    if ev is None:
        _print_json({"ok": False, "error": {"code": "event_not_found", "message": f"event not found: {event_id}"}})
        return 2
    ts = str(ev.get("ts") or "")
    cursor = set_cursor(group, actor_id, event_id=event_id, ts=ts)
    read_ev = append_event(
        group.ledger_path,
        kind="chat.read",
        group_id=group.group_id,
        scope_key="",
        by=by,
        data={"actor_id": actor_id, "event_id": event_id},
    )
    _print_json({"ok": True, "result": {"cursor": cursor, "event": read_ev}})
    return 0

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
