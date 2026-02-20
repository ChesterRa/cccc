"""
CCCC MCP Server - IM-style Agent Collaboration Tools

Core tools exposed to agents:

cccc.* namespace (collaboration control plane):
- cccc_help: CCCC help playbook (authoritative; returns effective CCCC_HELP.md if present)
- cccc_bootstrap: One-call session bootstrap (group+help+project+context+inbox)
- cccc_inbox_list: Get unread messages (supports kind_filter)
- cccc_inbox_mark_read: Mark messages as read
- cccc_inbox_mark_all_read: Mark all current unread messages as read
- cccc_message_send: Send message
- cccc_message_reply: Reply to message
- cccc_file_send: Send a local file as an attachment
- cccc_blob_path: Resolve attachment blob path
- cccc_group_info: Get group info
- cccc_actor_list: Get actor list
- cccc_actor_profile_list: List reusable Actor Profiles
- cccc_actor_add: Add new actor (foreman only)
- cccc_actor_remove: Remove an actor (foreman/peer can only remove themselves)
- cccc_actor_start: Start actor
- cccc_actor_stop: Stop actor
- cccc_runtime_list: List available agent runtimes
- cccc_automation_state: Read automation reminders/status visible to caller
- cccc_automation_manage: Manage automation reminders (MCP actor writes are notify-only)
- cccc_project_info: Get PROJECT.md content (project goals/constraints)

context.* namespace (state sync):
- cccc_context_get: Get full context
- cccc_context_sync: Batch sync operations
- cccc_vision_update / cccc_sketch_update: Vision/sketch
- cccc_milestone_*: Milestone management (create/update/complete/remove)
- cccc_task_*: Task management (list/create/update/delete)
- cccc_note_*: Note management (add/update/remove)
- cccc_reference_*: Reference management (add/update/remove)
- cccc_presence_*: Presence status (get/update/clear)

headless.* namespace (headless runner control):
- cccc_headless_status: Get headless session status
- cccc_headless_set_status: Update status (idle/working/waiting/stopped)
- cccc_headless_ack_message: Acknowledge processed message

notify.* namespace (system notifications, separate from chat):
- cccc_notify_send: Send system notification
- cccc_notify_ack: Acknowledge system notification

terminal.* namespace (diagnostics):
- cccc_terminal_tail: Tail an actor terminal transcript (group policy)

debug.* namespace (developer mode diagnostics):
- cccc_debug_snapshot: Get a structured debug snapshot (dev mode)
- cccc_debug_tail_logs: Tail local CCCC logs (dev mode)

All operations go through daemon IPC to ensure single-writer principle.
"""

from __future__ import annotations

import mimetypes
import json
import re
from pathlib import Path
from typing import Any, Dict, List, Optional

from ...kernel.blobs import resolve_blob_attachment_path, store_blob_bytes
from ...kernel.actors import get_effective_role
from ...kernel.group import load_group
from ...kernel.inbox import is_message_for_actor
from ...kernel.ledger import read_last_lines
from ...kernel.prompt_files import HELP_FILENAME, load_builtin_help_markdown as _load_builtin_help_markdown, read_group_prompt_file
from ...util.conv import coerce_bool
from .common import (
    MCPError,
    _call_daemon_or_raise,
    _env_str,
    _resolve_caller_actor_id,
    _resolve_caller_from_by,
    _resolve_group_id,
    _resolve_self_actor_id,
)
from .toolspecs import MCP_TOOLS


_CCCC_HELP_BUILTIN = _load_builtin_help_markdown().strip()

_HELP_ROLE_HEADER_RE = re.compile(r"^##\s*@role:\s*(\w+)\s*$", re.IGNORECASE)
_HELP_ACTOR_HEADER_RE = re.compile(r"^##\s*@actor:\s*(.+?)\s*$", re.IGNORECASE)
_HELP_H2_RE = re.compile(r"^##(?!#)\s+.*$")


def _select_help_markdown(markdown: str, *, role: Optional[str], actor_id: Optional[str]) -> str:
    """Filter CCCC_HELP markdown by optional conditional blocks.

    Supported markers (level-2 headings):
    - "## @role: foreman|peer"
    - "## @actor: <actor_id>"

    Untagged content is always included. Tagged blocks are filtered only when the selector is known.

    A tagged block starts at its marker heading and ends at the next level-2 heading.
    Within tagged blocks, prefer "###" for subheadings (so "##" can remain a block boundary).
    """
    raw = str(markdown or "")
    if not raw.strip():
        return raw

    role_norm = str(role or "").strip().casefold()
    actor_norm = str(actor_id or "").strip()
    lines = raw.splitlines()
    keep_trailing_newline = raw.endswith("\n")

    out: list[str] = []
    buf: list[str] = []
    tag_kind: Optional[str] = None
    tag_value: str = ""

    def _include_block() -> bool:
        if tag_kind is None:
            return True
        if tag_kind == "role":
            if not role_norm:
                return True
            return role_norm == str(tag_value or "").strip().casefold()
        if tag_kind == "actor":
            if not actor_norm:
                return False
            return actor_norm == str(tag_value or "").strip()
        return True

    def _flush() -> None:
        nonlocal buf
        if buf and _include_block():
            out.extend(buf)
        buf = []

    for ln in lines:
        m_role = _HELP_ROLE_HEADER_RE.match(ln)
        m_actor = _HELP_ACTOR_HEADER_RE.match(ln)
        is_h2 = bool(_HELP_H2_RE.match(ln))

        if m_role or m_actor:
            _flush()
            if m_role:
                tag_kind = "role"
                tag_value = str(m_role.group(1) or "").strip()
                role_label = tag_value.strip().casefold()
                if role_label == "foreman":
                    ln = "## Foreman"
                elif role_label == "peer":
                    ln = "## Peer"
                else:
                    ln = f"## Role: {tag_value}"
            else:
                tag_kind = "actor"
                tag_value = str(m_actor.group(1) or "").strip()
                ln = "## Notes for you"
            buf.append(ln)
            continue

        if is_h2:
            _flush()
            tag_kind = None
            tag_value = ""

        buf.append(ln)
    _flush()

    out_text = "\n".join(out)
    if keep_trailing_newline:
        out_text += "\n"
    return out_text


# =============================================================================
# Inbox Tools
# =============================================================================


def inbox_list(*, group_id: str, actor_id: str, limit: int = 50, kind_filter: str = "all") -> Dict[str, Any]:
    """Get actor's unread message list
    
    Args:
        kind_filter: "all" | "chat" | "notify"
    """
    return _call_daemon_or_raise({
        "op": "inbox_list",
        "args": {"group_id": group_id, "actor_id": actor_id, "by": actor_id, "limit": limit, "kind_filter": kind_filter},
    })


def inbox_mark_read(*, group_id: str, actor_id: str, event_id: str) -> Dict[str, Any]:
    """Mark messages as read up to specified event"""
    return _call_daemon_or_raise({
        "op": "inbox_mark_read",
        "args": {"group_id": group_id, "actor_id": actor_id, "event_id": event_id, "by": actor_id},
    })


def inbox_mark_all_read(*, group_id: str, actor_id: str, kind_filter: str = "all") -> Dict[str, Any]:
    """Mark all currently-unread messages as read (safe: only up to current latest unread)."""
    return _call_daemon_or_raise({
        "op": "inbox_mark_all_read",
        "args": {"group_id": group_id, "actor_id": actor_id, "kind_filter": kind_filter, "by": actor_id},
    })


def bootstrap(
    *,
    group_id: str,
    actor_id: str,
    inbox_limit: int = 50,
    inbox_kind_filter: str = "all",
    ledger_tail_limit: int = 10,
    ledger_tail_max_chars: int = 8000,
) -> Dict[str, Any]:
    """One-call session bootstrap for agents.

    Returns:
    - group: group metadata
    - actors: actor list (roles + runtime)
    - help: effective CCCC help playbook (markdown + source)
    - project: PROJECT.md info
    - context: group context
    - inbox: unread messages
    - ledger_tail: recent chat.message tail (optional)
    """
    gi = group_info(group_id=group_id)
    group = gi.get("group") if isinstance(gi, dict) else None

    al = actor_list(group_id=group_id)
    actors = al.get("actors") if isinstance(al, dict) else None

    help_payload: Dict[str, Any] = {
        "markdown": _select_help_markdown(_CCCC_HELP_BUILTIN, role=None, actor_id=None),
        "source": "cccc.resources/cccc-help.md",
    }
    try:
        g = load_group(str(group_id or "").strip())
        if g is not None:
            role = get_effective_role(g, str(actor_id or "").strip())
            pf = read_group_prompt_file(g, HELP_FILENAME)
            if pf.found and isinstance(pf.content, str) and pf.content.strip():
                help_payload = {
                    "markdown": _select_help_markdown(pf.content, role=role, actor_id=actor_id),
                    "source": str(pf.path or ""),
                }
            else:
                help_payload = {
                    "markdown": _select_help_markdown(_CCCC_HELP_BUILTIN, role=role, actor_id=actor_id),
                    "source": "cccc.resources/cccc-help.md",
                }
    except Exception:
        pass

    project = project_info(group_id=group_id)
    context = context_get(group_id=group_id)
    inbox = inbox_list(group_id=group_id, actor_id=actor_id, limit=int(inbox_limit or 50), kind_filter=inbox_kind_filter)

    # Recent chat tail (for resuming mid-task). Keep it small: only chat.message.
    ledger_tail: List[Dict[str, Any]] = []
    ledger_tail_truncated = False
    try:
        limit = int(ledger_tail_limit or 0)
        max_chars = int(ledger_tail_max_chars or 0)
        if limit > 0 and max_chars > 0:
            g = load_group(str(group_id or "").strip())
            if g is not None:
                # Read a reasonable tail of lines, then filter to chat.message and take the last N.
                # This avoids scanning the whole file while still being robust to non-chat events.
                read_lines = min(2000, max(200, limit * 10))
                lines = read_last_lines(g.ledger_path, read_lines)
                chat_events: List[Dict[str, Any]] = []
                for raw in lines:
                    try:
                        ev = json.loads(raw)
                    except Exception:
                        continue
                    if not isinstance(ev, dict) or str(ev.get("kind") or "") != "chat.message":
                        continue
                    # Respect delivery semantics: only include messages visible to the caller (or sent by the caller).
                    # This prevents leaking directed messages via the convenience "ledger_tail" bootstrap field.
                    by = str(ev.get("by") or "").strip()
                    if by != str(actor_id or "").strip() and not is_message_for_actor(g, actor_id=actor_id, event=ev):
                        continue
                    data = ev.get("data") if isinstance(ev.get("data"), dict) else {}
                    text = data.get("text") if isinstance(data, dict) else None
                    if not isinstance(text, str) or not text:
                        continue
                    to = data.get("to") if isinstance(data, dict) else None
                    to_list = [str(x) for x in to] if isinstance(to, list) else []
                    chat_events.append(
                        {
                            "id": str(ev.get("id") or ""),
                            "ts": str(ev.get("ts") or ""),
                            "by": by,
                            "to": to_list,
                            "text": text,
                        }
                    )
                chat_events = chat_events[-limit:]

                # Enforce a max total character budget across returned message texts.
                used = 0
                for ev in chat_events:
                    remaining = max_chars - used
                    if remaining <= 0:
                        ledger_tail_truncated = True
                        break
                    t = str(ev.get("text") or "")
                    if len(t) > remaining:
                        ev["text"] = t[:remaining]
                        ledger_tail_truncated = True
                        ledger_tail.append(ev)
                        break
                    ledger_tail.append(ev)
                    used += len(t)
    except Exception:
        ledger_tail = []
        ledger_tail_truncated = False

    last_event_id = ""
    try:
        msgs = inbox.get("messages") if isinstance(inbox, dict) else None
        if isinstance(msgs, list) and msgs:
            last_event_id = str((msgs[-1] if isinstance(msgs[-1], dict) else {}).get("id") or "").strip()
    except Exception:
        last_event_id = ""

    return {
        "group": group,
        "actors": actors,
        "help": help_payload,
        "project": project,
        "context": context,
        "inbox": inbox,
        "ledger_tail": ledger_tail,
        "ledger_tail_truncated": ledger_tail_truncated,
        "suggested_mark_read_event_id": last_event_id,
    }


# =============================================================================
# Message Tools
# =============================================================================


def message_send(
    *,
    group_id: str,
    actor_id: str,
    text: str,
    to: Optional[List[str]] = None,
    reply_to: Optional[str] = None,
    priority: str = "normal",
    reply_required: bool = False,
    dst_group_id: Optional[str] = None,
) -> Dict[str, Any]:
    """Send a message"""
    prio = str(priority or "normal").strip() or "normal"
    if prio not in ("normal", "attention"):
        raise MCPError(code="invalid_priority", message="priority must be 'normal' or 'attention'")
    reply_required_flag = coerce_bool(reply_required, default=False)
    dst = str(dst_group_id or "").strip()
    if dst and dst != str(group_id or "").strip():
        if reply_to:
            raise MCPError(code="unsupported", message="cross-group reply is not supported; send a new message instead")
        return _call_daemon_or_raise({
            "op": "send_cross_group",
            "args": {
                "group_id": group_id,
                "dst_group_id": dst,
                "text": text,
                "by": actor_id,
                "to": to if to is not None else [],
                "priority": prio,
                "reply_required": reply_required_flag,
            },
        })
    if reply_to:
        return _call_daemon_or_raise({
            "op": "reply",
            "args": {
                "group_id": group_id,
                "text": text,
                "by": actor_id,
                "reply_to": reply_to,
                "to": to if to is not None else [],
                "priority": prio,
                "reply_required": reply_required_flag,
            },
        })
    return _call_daemon_or_raise({
        "op": "send",
        "args": {
            "group_id": group_id,
            "text": text,
            "by": actor_id,
            "to": to if to is not None else [],
            "path": "",
            "priority": prio,
            "reply_required": reply_required_flag,
        },
    })


def message_reply(
    *,
    group_id: str,
    actor_id: str,
    reply_to: str,
    text: str,
    to: Optional[List[str]] = None,
    priority: str = "normal",
    reply_required: bool = False,
) -> Dict[str, Any]:
    """Reply to a message"""
    prio = str(priority or "normal").strip() or "normal"
    if prio not in ("normal", "attention"):
        raise MCPError(code="invalid_priority", message="priority must be 'normal' or 'attention'")
    reply_required_flag = coerce_bool(reply_required, default=False)
    return _call_daemon_or_raise({
        "op": "reply",
        "args": {
            "group_id": group_id,
            "text": text,
            "by": actor_id,
            "reply_to": reply_to,
            "to": to if to is not None else [],
            "priority": prio,
            "reply_required": reply_required_flag,
        },
    })


def blob_path(*, group_id: str, rel_path: str) -> Dict[str, Any]:
    """Resolve a blob attachment path to an absolute filesystem path."""
    group = load_group(str(group_id or "").strip())
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {group_id}")
    abs_path = resolve_blob_attachment_path(group, rel_path=str(rel_path or "").strip())
    return {"path": str(abs_path)}


def file_send(
    *,
    group_id: str,
    actor_id: str,
    path: str,
    text: str = "",
    to: Optional[List[str]] = None,
    priority: str = "normal",
    reply_required: bool = False,
) -> Dict[str, Any]:
    """Send a local file as a chat.message attachment.

    Security: only files under the group's active scope root are allowed.
    """
    gid = str(group_id or "").strip()
    group = load_group(gid)
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {group_id}")

    scope_key = str(group.doc.get("active_scope_key") or "").strip()
    if not scope_key:
        raise MCPError(code="missing_scope", message="group has no active scope")
    scopes = group.doc.get("scopes")
    scope_url = ""
    if isinstance(scopes, list):
        for sc in scopes:
            if isinstance(sc, dict) and str(sc.get("scope_key") or "").strip() == scope_key:
                scope_url = str(sc.get("url") or "").strip()
                break
    if not scope_url:
        raise MCPError(code="missing_scope", message="active scope url not found")

    root = Path(scope_url).expanduser().resolve()
    src = Path(str(path or "").strip())
    if not src.is_absolute():
        src = (root / src).resolve()
    else:
        src = src.expanduser().resolve()

    try:
        src.relative_to(root)
    except ValueError:
        raise MCPError(code="invalid_path", message="path must be under the group's active scope root")
    if not src.exists() or not src.is_file():
        raise MCPError(code="not_found", message=f"file not found: {src}")

    try:
        raw = src.read_bytes()
    except Exception as e:
        raise MCPError(code="read_failed", message=str(e))

    mt, _ = mimetypes.guess_type(src.name)
    att = store_blob_bytes(group, data=raw, filename=src.name, mime_type=str(mt or ""))
    msg = str(text or "").strip() or f"[file] {att.get('title') or src.name}"
    prio = str(priority or "normal").strip() or "normal"
    if prio not in ("normal", "attention"):
        raise MCPError(code="invalid_priority", message="priority must be 'normal' or 'attention'")
    reply_required_flag = coerce_bool(reply_required, default=False)
    return _call_daemon_or_raise({
        "op": "send",
        "args": {
            "group_id": gid,
            "text": msg,
            "by": actor_id,
            "to": to if to is not None else [],
            "path": "",
            "attachments": [att],
            "priority": prio,
            "reply_required": reply_required_flag,
        },
    })


# =============================================================================
# Group/Actor Info Tools
# =============================================================================

def _sanitize_group_doc_for_agent(doc: Any) -> Dict[str, Any]:
    """Return a minimal, non-secret group view for agents.

    This intentionally excludes sensitive fields such as IM tokens and actor env.
    """
    if not isinstance(doc, dict):
        return {}
    out: Dict[str, Any] = {}
    for k in ("group_id", "title", "topic", "created_at", "updated_at", "state", "active_scope_key"):
        if k in doc:
            out[k] = doc.get(k)
    out["running"] = coerce_bool(doc.get("running"), default=False)
    scopes = doc.get("scopes")
    if isinstance(scopes, list):
        safe_scopes: list[dict[str, Any]] = []
        for sc in scopes:
            if not isinstance(sc, dict):
                continue
            safe_scopes.append({
                "scope_key": sc.get("scope_key"),
                "url": sc.get("url"),
                "label": sc.get("label"),
                "git_remote": sc.get("git_remote"),
            })
        out["scopes"] = safe_scopes
    return out


def _sanitize_actors_for_agent(raw: Any) -> List[Dict[str, Any]]:
    """Return a minimal actor view for agents (no env/command)."""
    if not isinstance(raw, list):
        return []
    out: List[Dict[str, Any]] = []
    for a in raw:
        if not isinstance(a, dict):
            continue
        out.append({
            "id": a.get("id"),
            "role": a.get("role"),
            "title": a.get("title"),
            "enabled": coerce_bool(a.get("enabled"), default=True),
            "running": coerce_bool(a.get("running"), default=False),
            "runner": a.get("runner"),
            "runtime": a.get("runtime"),
            "submit": a.get("submit"),
            "unread_count": a.get("unread_count"),
            "updated_at": a.get("updated_at"),
            "created_at": a.get("created_at"),
        })
    return out


def group_info(*, group_id: str) -> Dict[str, Any]:
    """Get group information"""
    res = _call_daemon_or_raise({"op": "group_show", "args": {"group_id": group_id}})
    doc = res.get("group") if isinstance(res, dict) else None
    return {"group": _sanitize_group_doc_for_agent(doc)}


def group_list() -> Dict[str, Any]:
    """List working groups (metadata only)."""
    res = _call_daemon_or_raise({"op": "groups"})
    raw = res.get("groups") if isinstance(res, dict) else None
    if not isinstance(raw, list):
        raw = []
    out: List[Dict[str, Any]] = []
    for g in raw:
        if not isinstance(g, dict):
            continue
        gid = str(g.get("group_id") or "").strip()
        if not gid:
            continue
        out.append(
            {
                "group_id": gid,
                "title": g.get("title") or "",
                "topic": g.get("topic") or "",
                "running": coerce_bool(g.get("running"), default=False),
                "state": g.get("state") or "",
                "updated_at": g.get("updated_at") or "",
                "created_at": g.get("created_at") or "",
            }
        )
    return {"groups": out}


def actor_list(*, group_id: str) -> Dict[str, Any]:
    """Get actor list"""
    res = _call_daemon_or_raise({"op": "actor_list", "args": {"group_id": group_id, "include_unread": True}})
    actors = res.get("actors") if isinstance(res, dict) else None
    return {"actors": _sanitize_actors_for_agent(actors)}


def actor_profile_list(*, by: str) -> Dict[str, Any]:
    """List reusable Actor Profiles."""
    return _call_daemon_or_raise({"op": "actor_profile_list", "args": {"by": by}})


def actor_add(
    *, group_id: str, by: str, actor_id: str,
    runtime: str = "codex", runner: str = "pty", title: str = "",
    command: Optional[List[str]] = None, env: Optional[Dict[str, str]] = None, profile_id: str = ""
) -> Dict[str, Any]:
    """Add a new actor (foreman only). Role is auto-determined by position."""
    req_args: Dict[str, Any] = {
        "group_id": group_id,
        "actor_id": actor_id,
        "runtime": runtime,
        "runner": runner,
        "title": title,
        "command": command or [],
        "env": env or {},
        "by": by,
    }
    pid = str(profile_id or "").strip()
    if pid:
        req_args["profile_id"] = pid
    return _call_daemon_or_raise({
        "op": "actor_add",
        "args": req_args,
    })


def actor_remove(*, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
    """Remove an actor. Foreman/peer can only remove themselves."""
    return _call_daemon_or_raise({
        "op": "actor_remove",
        "args": {"group_id": group_id, "actor_id": actor_id, "by": by},
    })


def actor_start(*, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
    """Start an actor (set enabled=true). Foreman can start any; peer cannot start."""
    return _call_daemon_or_raise({
        "op": "actor_start",
        "args": {"group_id": group_id, "actor_id": actor_id, "by": by},
    })


def actor_stop(*, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
    """Stop an actor (set enabled=false). Foreman can stop any; peer can only stop self."""
    return _call_daemon_or_raise({
        "op": "actor_stop",
        "args": {"group_id": group_id, "actor_id": actor_id, "by": by},
    })


def actor_restart(*, group_id: str, by: str, actor_id: str) -> Dict[str, Any]:
    """Restart an actor (stop + start, clears context). Foreman can restart any; peer can only restart self."""
    return _call_daemon_or_raise({
        "op": "actor_restart",
        "args": {"group_id": group_id, "actor_id": actor_id, "by": by},
    })


def runtime_list() -> Dict[str, Any]:
    """List available agent runtimes on the system"""
    from ...kernel.runtime import detect_all_runtimes
    from ...kernel.settings import get_runtime_pool
    
    runtimes = detect_all_runtimes(primary_only=False)
    pool = get_runtime_pool()
    
    return {
        "runtimes": [
            {
                "name": rt.name,
                "display_name": rt.display_name,
                "command": rt.command,
                "available": rt.available,
                "path": rt.path,
                "capabilities": rt.capabilities,
            }
            for rt in runtimes
        ],
        "available": [rt.name for rt in runtimes if rt.available],
        "pool": [
            {
                "runtime": e.runtime,
                "priority": e.priority,
                "scenarios": e.scenarios,
                "notes": e.notes,
            }
            for e in pool
        ],
    }


def group_set_state(*, group_id: str, by: str, state: str) -> Dict[str, Any]:
    """Set group state (active/idle/paused/stopped)."""
    s = str(state or "").strip().lower()
    if s == "stopped":
        return _call_daemon_or_raise({
            "op": "group_stop",
            "args": {"group_id": group_id, "by": by},
        })
    return _call_daemon_or_raise({
        "op": "group_set_state",
        "args": {"group_id": group_id, "state": s, "by": by},
    })


def automation_state(*, group_id: str, by: str) -> Dict[str, Any]:
    """Read automation reminders/status visible to caller."""
    return _call_daemon_or_raise({
        "op": "group_automation_state",
        "args": {"group_id": group_id, "by": by},
    })


def automation_manage(
    *,
    group_id: str,
    by: str,
    actions: List[Dict[str, Any]],
    expected_version: Optional[int] = None,
) -> Dict[str, Any]:
    """Manage automation reminders incrementally."""
    req_args: Dict[str, Any] = {
        "group_id": group_id,
        "by": by,
        "actions": actions,
    }
    if expected_version is not None:
        req_args["expected_version"] = int(expected_version)
    return _call_daemon_or_raise({"op": "group_automation_manage", "args": req_args})


def _assert_agent_notify_only_actions(actions: List[Dict[str, Any]]) -> None:
    for idx, action in enumerate(actions):
        action_type = str(action.get("type") or "").strip()
        if action_type in {"create_rule", "update_rule"}:
            rule = action.get("rule")
            if not isinstance(rule, dict):
                continue
            action_doc = rule.get("action")
            if not isinstance(action_doc, dict):
                continue
            kind = str(action_doc.get("kind") or "notify").strip()
            if kind != "notify":
                raise MCPError(
                    code="permission_denied",
                    message=f"actions[{idx}] uses action.kind={kind}; agents may only manage notify rules",
                )
            continue
        if action_type == "replace_all_rules":
            ruleset = action.get("ruleset")
            if not isinstance(ruleset, dict):
                continue
            rules = ruleset.get("rules")
            if not isinstance(rules, list):
                continue
            for j, rule in enumerate(rules):
                if not isinstance(rule, dict):
                    continue
                action_doc = rule.get("action")
                if not isinstance(action_doc, dict):
                    continue
                kind = str(action_doc.get("kind") or "notify").strip()
                if kind != "notify":
                    raise MCPError(
                        code="permission_denied",
                        message=f"actions[{idx}].rules[{j}] uses action.kind={kind}; agents may only manage notify rules",
                    )


def _assert_action_trigger_compat(actions: List[Dict[str, Any]]) -> None:
    def _validate_rule(rule: Dict[str, Any], *, loc: str) -> None:
        action_doc = rule.get("action")
        trigger_doc = rule.get("trigger")
        if not isinstance(action_doc, dict) or not isinstance(trigger_doc, dict):
            return
        action_kind = str(action_doc.get("kind") or "notify").strip()
        trigger_kind = str(trigger_doc.get("kind") or "").strip()
        if action_kind in {"group_state", "actor_control"} and trigger_kind != "at":
            raise MCPError(
                code="invalid_request",
                message=f"{loc} uses action.kind={action_kind}; only one-time trigger.kind=at is allowed",
            )

    for idx, action in enumerate(actions):
        action_type = str(action.get("type") or "").strip()
        if action_type in {"create_rule", "update_rule"}:
            rule = action.get("rule")
            if isinstance(rule, dict):
                _validate_rule(rule, loc=f"actions[{idx}].rule")
            continue
        if action_type == "replace_all_rules":
            ruleset = action.get("ruleset")
            if not isinstance(ruleset, dict):
                continue
            rules = ruleset.get("rules")
            if not isinstance(rules, list):
                continue
            for j, rule in enumerate(rules):
                if isinstance(rule, dict):
                    _validate_rule(rule, loc=f"actions[{idx}].rules[{j}]")


def _map_simple_automation_op_to_action(arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    op = str(arguments.get("op") or "").strip().lower()
    if not op:
        return None
    if op == "create":
        rule = arguments.get("rule")
        if not isinstance(rule, dict):
            raise MCPError(code="invalid_request", message="op=create requires reminder object (rule)")
        return {"type": "create_rule", "rule": rule}
    if op == "update":
        rule = arguments.get("rule")
        if not isinstance(rule, dict):
            raise MCPError(code="invalid_request", message="op=update requires reminder object (rule)")
        return {"type": "update_rule", "rule": rule}
    if op == "enable":
        rule_id = str(arguments.get("rule_id") or "").strip()
        if not rule_id:
            raise MCPError(code="invalid_request", message="op=enable requires rule_id")
        return {"type": "set_rule_enabled", "rule_id": rule_id, "enabled": True}
    if op == "disable":
        rule_id = str(arguments.get("rule_id") or "").strip()
        if not rule_id:
            raise MCPError(code="invalid_request", message="op=disable requires rule_id")
        return {"type": "set_rule_enabled", "rule_id": rule_id, "enabled": False}
    if op == "delete":
        rule_id = str(arguments.get("rule_id") or "").strip()
        if not rule_id:
            raise MCPError(code="invalid_request", message="op=delete requires rule_id")
        return {"type": "delete_rule", "rule_id": rule_id}
    if op == "replace_all":
        ruleset = arguments.get("ruleset")
        if not isinstance(ruleset, dict):
            raise MCPError(code="invalid_request", message="op=replace_all requires reminder set object (ruleset)")
        return {"type": "replace_all_rules", "ruleset": ruleset}
    raise MCPError(
        code="invalid_request",
        message="op must be one of: create, update, enable, disable, delete, replace_all",
    )


def im_bind(*, group_id: str, key: str) -> Dict[str, Any]:
    """Bind an IM chat using a one-time key from /subscribe."""
    k = str(key or "").strip()
    if not k:
        raise MCPError(code="missing_key", message="key is required")
    return _call_daemon_or_raise({
        "op": "im_bind_chat",
        "args": {"group_id": group_id, "key": k},
    })


def project_info(*, group_id: str) -> Dict[str, Any]:
    """Get PROJECT.md content for the group's active scope"""
    from pathlib import Path
    from ...kernel.group import load_group
    
    group = load_group(group_id)
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {group_id}")
    
    # Get active scope's project root
    scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
    active_scope_key = str(group.doc.get("active_scope_key") or "")
    
    project_root: Optional[str] = None
    for sc in scopes:
        if not isinstance(sc, dict):
            continue
        sk = str(sc.get("scope_key") or "")
        if sk == active_scope_key:
            project_root = str(sc.get("url") or "")
            break
    
    if not project_root:
        # Fallback: try first scope
        if scopes and isinstance(scopes[0], dict):
            project_root = str(scopes[0].get("url") or "")
    
    if not project_root:
        return {
            "found": False,
            "path": None,
            "content": None,
            "error": "No scope attached to group. Use 'cccc attach <path>' first.",
        }
    
    # Look for PROJECT.md in project root
    project_md_path = Path(project_root) / "PROJECT.md"
    if not project_md_path.exists():
        # Also try lowercase
        project_md_path_lower = Path(project_root) / "project.md"
        if project_md_path_lower.exists():
            project_md_path = project_md_path_lower
        else:
            return {
                "found": False,
                "path": str(project_md_path),
                "content": None,
                "error": f"PROJECT.md not found at {project_md_path}",
            }
    
    try:
        content = project_md_path.read_text(encoding="utf-8", errors="replace")
        return {
            "found": True,
            "path": str(project_md_path),
            "content": content,
        }
    except Exception as e:
        return {
            "found": False,
            "path": str(project_md_path),
            "content": None,
            "error": f"Failed to read PROJECT.md: {e}",
        }


# =============================================================================
# Context Tools (all via daemon IPC)
# =============================================================================


def context_get(*, group_id: str, include_archived: bool = False) -> Dict[str, Any]:
    """Get full context.

    By default, archived milestones are hidden to reduce cognitive load.
    """
    result = _call_daemon_or_raise({"op": "context_get", "args": {"group_id": group_id}})
    if include_archived:
        return result

    milestones = result.get("milestones")
    if isinstance(milestones, list):
        result["milestones"] = [
            m
            for m in milestones
            if isinstance(m, dict) and str(m.get("status") or "").strip().lower() != "archived"
        ]

    tasks_summary = result.get("tasks_summary")
    if isinstance(tasks_summary, dict):
        try:
            active = int(tasks_summary.get("active") or 0)
            planned = int(tasks_summary.get("planned") or 0)
            done = int(tasks_summary.get("done") or 0)
            tasks_summary["total"] = active + planned + done
        except Exception:
            pass

    return result


def context_sync(*, group_id: str, ops: List[Dict[str, Any]], dry_run: bool = False) -> Dict[str, Any]:
    """Batch sync context operations"""
    return _call_daemon_or_raise({
        "op": "context_sync",
        "args": {"group_id": group_id, "ops": ops, "dry_run": dry_run},
    })


def task_list(
    *, group_id: str, task_id: Optional[str] = None, include_archived: bool = False
) -> Dict[str, Any]:
    """List tasks.

    By default, archived tasks are hidden to reduce cognitive load.
    """
    args: Dict[str, Any] = {"group_id": group_id}
    if task_id:
        args["task_id"] = task_id
    result = _call_daemon_or_raise({"op": "task_list", "args": args})
    if include_archived:
        return result

    if "task" in result and isinstance(result.get("task"), dict):
        task = result.get("task")
        status = str(task.get("status") or "").strip().lower() if isinstance(task, dict) else ""
        if status == "archived":
            raise MCPError(code="archived_hidden", message="archived task is hidden by default")
        return result

    tasks = result.get("tasks")
    if isinstance(tasks, list):
        result["tasks"] = [
            t
            for t in tasks
            if isinstance(t, dict) and str(t.get("status") or "").strip().lower() != "archived"
        ]
    return result


def presence_get(*, group_id: str) -> Dict[str, Any]:
    """Get presence status"""
    return _call_daemon_or_raise({"op": "presence_get", "args": {"group_id": group_id}})


# Convenience wrappers (all delegate to context_sync)


def vision_update(*, group_id: str, vision: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "vision.update", "vision": vision}])


def sketch_update(*, group_id: str, sketch: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "sketch.update", "sketch": sketch}])


def milestone_create(*, group_id: str, name: str, description: str, status: str = "planned") -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{
        "op": "milestone.create", "name": name, "description": description, "status": status
    }])


def milestone_update(
    *, group_id: str, milestone_id: str,
    name: Optional[str] = None, description: Optional[str] = None, status: Optional[str] = None
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "milestone.update", "milestone_id": milestone_id}
    if name is not None:
        op["name"] = name
    if description is not None:
        op["description"] = description
    if status is not None:
        op["status"] = status
    return context_sync(group_id=group_id, ops=[op])


def milestone_complete(*, group_id: str, milestone_id: str, outcomes: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{
        "op": "milestone.complete", "milestone_id": milestone_id, "outcomes": outcomes
    }])


def task_create(
    *, group_id: str, name: str, goal: str, steps: List[Dict[str, str]],
    milestone_id: Optional[str] = None, assignee: Optional[str] = None
) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{
        "op": "task.create", "name": name, "goal": goal, "steps": steps,
        "milestone_id": milestone_id, "assignee": assignee
    }])


def task_update(
    *, group_id: str, task_id: str,
    status: Optional[str] = None, name: Optional[str] = None, goal: Optional[str] = None,
    assignee: Optional[str] = None, milestone_id: Optional[str] = None,
    step_id: Optional[str] = None, step_status: Optional[str] = None
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "task.update", "task_id": task_id}
    if status is not None:
        op["status"] = status
    if name is not None:
        op["name"] = name
    if goal is not None:
        op["goal"] = goal
    if assignee is not None:
        op["assignee"] = assignee
    if milestone_id is not None:
        op["milestone_id"] = milestone_id
    if step_id is not None and step_status is not None:
        op["step_id"] = step_id
        op["step_status"] = step_status
    return context_sync(group_id=group_id, ops=[op])


def note_add(*, group_id: str, content: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "note.add", "content": content}])


def note_update(*, group_id: str, note_id: str, content: Optional[str] = None) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "note.update", "note_id": note_id}
    if content is not None:
        op["content"] = content
    return context_sync(group_id=group_id, ops=[op])


def note_remove(*, group_id: str, note_id: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "note.remove", "note_id": note_id}])


def reference_add(*, group_id: str, url: str, note: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "reference.add", "url": url, "note": note}])


def reference_update(
    *, group_id: str, reference_id: str,
    url: Optional[str] = None, note: Optional[str] = None
) -> Dict[str, Any]:
    op: Dict[str, Any] = {"op": "reference.update", "reference_id": reference_id}
    if url is not None:
        op["url"] = url
    if note is not None:
        op["note"] = note
    return context_sync(group_id=group_id, ops=[op])


def reference_remove(*, group_id: str, reference_id: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "reference.remove", "reference_id": reference_id}])


def presence_update(*, group_id: str, agent_id: str, status: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "presence.update", "agent_id": agent_id, "status": status}])


def presence_clear(*, group_id: str, agent_id: str) -> Dict[str, Any]:
    return context_sync(group_id=group_id, ops=[{"op": "presence.clear", "agent_id": agent_id}])


# =============================================================================
# Headless Runner Tools (for MCP-driven agents)
# =============================================================================


def headless_status(*, group_id: str, actor_id: str) -> Dict[str, Any]:
    """Get headless session status"""
    return _call_daemon_or_raise({
        "op": "headless_status",
        "args": {"group_id": group_id, "actor_id": actor_id},
    })


def headless_set_status(
    *, group_id: str, actor_id: str, status: str, task_id: Optional[str] = None
) -> Dict[str, Any]:
    """Update headless session status"""
    return _call_daemon_or_raise({
        "op": "headless_set_status",
        "args": {"group_id": group_id, "actor_id": actor_id, "status": status, "task_id": task_id},
    })


def headless_ack_message(*, group_id: str, actor_id: str, message_id: str) -> Dict[str, Any]:
    """Acknowledge processed message"""
    return _call_daemon_or_raise({
        "op": "headless_ack_message",
        "args": {"group_id": group_id, "actor_id": actor_id, "message_id": message_id},
    })


# =============================================================================
# System Notification Tools
# =============================================================================


def notify_send(
    *, group_id: str, actor_id: str, kind: str, title: str, message: str,
    target_actor_id: Optional[str] = None, priority: str = "normal", requires_ack: bool = False
) -> Dict[str, Any]:
    """Send system notification"""
    return _call_daemon_or_raise({
        "op": "system_notify",
        "args": {
            "group_id": group_id,
            "by": actor_id,
            "kind": kind,
            "priority": priority,
            "title": title,
            "message": message,
            "target_actor_id": target_actor_id,
            "requires_ack": requires_ack,
        },
    })


def notify_ack(*, group_id: str, actor_id: str, notify_event_id: str) -> Dict[str, Any]:
    """Acknowledge system notification"""
    return _call_daemon_or_raise({
        "op": "notify_ack",
        "args": {"group_id": group_id, "actor_id": actor_id, "notify_event_id": notify_event_id, "by": actor_id},
    })


# =============================================================================
# Debug Tools (developer mode)
# =============================================================================


def debug_snapshot(*, group_id: str, actor_id: str) -> Dict[str, Any]:
    """Get a structured debug snapshot (developer mode only; user+foreman only)."""
    return _call_daemon_or_raise({
        "op": "debug_snapshot",
        "args": {"group_id": group_id, "by": actor_id},
    })


def terminal_tail(
    *,
    group_id: str,
    actor_id: str,
    target_actor_id: str,
    max_chars: int = 8000,
    strip_ansi: bool = True,
) -> Dict[str, Any]:
    """Tail an actor terminal transcript (subject to group policy; may include sensitive stdout/stderr)."""
    return _call_daemon_or_raise({
        "op": "terminal_tail",
        "args": {
            "group_id": group_id,
            "actor_id": str(target_actor_id or ""),
            "by": actor_id,
            "max_chars": int(max_chars or 8000),
            "strip_ansi": bool(strip_ansi),
        },
    })


def debug_tail_logs(
    *,
    group_id: str,
    actor_id: str,
    component: str,
    lines: int = 200,
) -> Dict[str, Any]:
    """Tail CCCC local logs (developer mode only; user+foreman only)."""
    return _call_daemon_or_raise({
        "op": "debug_tail_logs",
        "args": {
            "group_id": group_id,
            "by": actor_id,
            "component": str(component or ""),
            "lines": int(lines or 200),
        },
    })


# =============================================================================
# Tool Call Handler
# =============================================================================


def _handle_cccc_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if name == "cccc_help":
        gid = _env_str("CCCC_GROUP_ID")
        aid = _env_str("CCCC_ACTOR_ID")
        role: Optional[str] = None
        if gid:
            g = load_group(gid)
            if g is not None:
                if aid:
                    try:
                        role = get_effective_role(g, aid)
                    except Exception:
                        role = None
                pf = read_group_prompt_file(g, HELP_FILENAME)
                if pf.found and isinstance(pf.content, str) and pf.content.strip():
                    return {
                        "markdown": _select_help_markdown(pf.content, role=role, actor_id=aid),
                        "source": str(pf.path or ""),
                    }
        return {
            "markdown": _select_help_markdown(_CCCC_HELP_BUILTIN, role=role, actor_id=aid),
            "source": "cccc.resources/cccc-help.md",
        }

    if name == "cccc_inbox_list":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return inbox_list(
            group_id=gid,
            actor_id=aid,
            limit=min(max(int(arguments.get("limit") or 50), 1), 1000),
            kind_filter=str(arguments.get("kind_filter") or "all"),
        )

    if name == "cccc_bootstrap":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return bootstrap(
            group_id=gid,
            actor_id=aid,
            inbox_limit=min(max(int(arguments.get("inbox_limit") or 50), 1), 1000),
            inbox_kind_filter=str(arguments.get("inbox_kind_filter") or "all"),
            ledger_tail_limit=min(max(int(arguments.get("ledger_tail_limit") or 10), 0), 1000),
            ledger_tail_max_chars=min(max(int(arguments.get("ledger_tail_max_chars") or 8000), 0), 100000),
        )

    if name == "cccc_inbox_mark_read":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return inbox_mark_read(
            group_id=gid,
            actor_id=aid,
            event_id=str(arguments.get("event_id") or ""),
        )

    if name == "cccc_inbox_mark_all_read":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return inbox_mark_all_read(
            group_id=gid,
            actor_id=aid,
            kind_filter=str(arguments.get("kind_filter") or "all"),
        )

    if name == "cccc_message_send":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        to_raw = arguments.get("to")
        if isinstance(to_raw, list):
            to_val: Optional[List[str]] = [str(x).strip() for x in to_raw if str(x).strip()]
        elif isinstance(to_raw, str) and to_raw.strip():
            to_val = [to_raw.strip()]
        else:
            to_val = None
        return message_send(
            group_id=gid,
            dst_group_id=arguments.get("dst_group_id"),
            actor_id=aid,
            text=str(arguments.get("text") or ""),
            to=to_val,
            priority=str(arguments.get("priority") or "normal"),
            reply_required=coerce_bool(arguments.get("reply_required"), default=False),
        )

    if name == "cccc_message_reply":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        to_raw = arguments.get("to")
        if isinstance(to_raw, list):
            to_val_reply: Optional[List[str]] = [str(x).strip() for x in to_raw if str(x).strip()]
        elif isinstance(to_raw, str) and to_raw.strip():
            to_val_reply = [to_raw.strip()]
        else:
            to_val_reply = None
        reply_to = str(arguments.get("event_id") or arguments.get("reply_to") or "").strip()
        return message_reply(
            group_id=gid,
            actor_id=aid,
            reply_to=reply_to,
            text=str(arguments.get("text") or ""),
            to=to_val_reply,
            priority=str(arguments.get("priority") or "normal"),
            reply_required=coerce_bool(arguments.get("reply_required"), default=False),
        )

    if name == "cccc_file_send":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        to_raw = arguments.get("to")
        if isinstance(to_raw, list):
            to_val_file: Optional[List[str]] = [str(x).strip() for x in to_raw if str(x).strip()]
        elif isinstance(to_raw, str) and to_raw.strip():
            to_val_file = [to_raw.strip()]
        else:
            to_val_file = None
        return file_send(
            group_id=gid,
            actor_id=aid,
            path=str(arguments.get("path") or ""),
            text=str(arguments.get("text") or ""),
            to=to_val_file,
            priority=str(arguments.get("priority") or "normal"),
            reply_required=coerce_bool(arguments.get("reply_required"), default=False),
        )

    if name == "cccc_blob_path":
        gid = _resolve_group_id(arguments)
        return blob_path(
            group_id=gid,
            rel_path=str(arguments.get("rel_path") or ""),
        )

    if name == "cccc_group_info":
        gid = _resolve_group_id(arguments)
        return group_info(group_id=gid)

    if name == "cccc_group_list":
        return group_list()

    if name == "cccc_actor_list":
        gid = _resolve_group_id(arguments)
        return actor_list(group_id=gid)

    if name == "cccc_actor_profile_list":
        by = _resolve_caller_from_by(arguments)
        return actor_profile_list(by=by)

    if name == "cccc_actor_add":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        cmd_raw = arguments.get("command")
        env_raw = arguments.get("env")
        return actor_add(
            group_id=gid,
            by=by,
            actor_id=str(arguments.get("actor_id") or ""),
            runtime=str(arguments.get("runtime") or "codex"),
            runner=str(arguments.get("runner") or "pty"),
            title=str(arguments.get("title") or ""),
            command=list(cmd_raw) if isinstance(cmd_raw, list) else None,
            env=dict(env_raw) if isinstance(env_raw, dict) else None,
            profile_id=str(arguments.get("profile_id") or ""),
        )

    if name == "cccc_actor_remove":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        target = str(arguments.get("actor_id") or "").strip() or by
        return actor_remove(group_id=gid, by=by, actor_id=target)

    if name == "cccc_actor_start":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return actor_start(
            group_id=gid,
            by=by,
            actor_id=str(arguments.get("actor_id") or ""),
        )

    if name == "cccc_actor_stop":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return actor_stop(
            group_id=gid,
            by=by,
            actor_id=str(arguments.get("actor_id") or ""),
        )

    if name == "cccc_actor_restart":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_from_by(arguments)
        return actor_restart(
            group_id=gid,
            by=by,
            actor_id=str(arguments.get("actor_id") or ""),
        )

    if name == "cccc_runtime_list":
        return runtime_list()

    if name == "cccc_group_set_state":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        return group_set_state(
            group_id=gid,
            by=by,
            state=str(arguments.get("state") or ""),
        )

    if name == "cccc_automation_state":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        return automation_state(group_id=gid, by=by)

    if name == "cccc_automation_manage":
        gid = _resolve_group_id(arguments)
        by = _resolve_caller_actor_id(arguments)
        actions: List[Dict[str, Any]] = []
        mapped = _map_simple_automation_op_to_action(arguments)
        if isinstance(mapped, dict):
            actions.append(mapped)
        actions_raw = arguments.get("actions")
        if isinstance(actions_raw, list):
            for i, action in enumerate(actions_raw):
                if not isinstance(action, dict):
                    raise MCPError(code="invalid_request", message=f"actions[{i}] must be an object")
                actions.append(action)
        if not actions:
            raise MCPError(code="invalid_request", message="provide op (simple mode) or actions[] (advanced mode)")
        _assert_action_trigger_compat(actions)
        if by != "user":
            _assert_agent_notify_only_actions(actions)
        expected_version_raw = arguments.get("expected_version")
        expected_version: Optional[int] = None
        if expected_version_raw is not None:
            try:
                expected_version = int(expected_version_raw)
            except Exception:
                raise MCPError(code="invalid_request", message="expected_version must be an integer")
        return automation_manage(group_id=gid, by=by, actions=actions, expected_version=expected_version)

    if name == "cccc_project_info":
        gid = _resolve_group_id(arguments)
        return project_info(group_id=gid)

    if name == "cccc_im_bind":
        gid = _resolve_group_id(arguments)
        return im_bind(group_id=gid, key=str(arguments.get("key") or ""))

    return None


def _handle_context_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if name == "cccc_context_get":
        gid = _resolve_group_id(arguments)
        return context_get(group_id=gid, include_archived=coerce_bool(arguments.get("include_archived"), default=False))

    if name == "cccc_context_sync":
        gid = _resolve_group_id(arguments)
        ops_raw = arguments.get("ops")
        return context_sync(
            group_id=gid,
            ops=list(ops_raw) if isinstance(ops_raw, list) else [],
            dry_run=coerce_bool(arguments.get("dry_run"), default=False),
        )

    if name == "cccc_vision_update":
        gid = _resolve_group_id(arguments)
        return vision_update(group_id=gid, vision=str(arguments.get("vision") or ""))

    if name == "cccc_sketch_update":
        gid = _resolve_group_id(arguments)
        return sketch_update(group_id=gid, sketch=str(arguments.get("sketch") or ""))

    if name == "cccc_milestone_create":
        gid = _resolve_group_id(arguments)
        return milestone_create(
            group_id=gid,
            name=str(arguments.get("name") or ""),
            description=str(arguments.get("description") or ""),
            status=str(arguments.get("status") or "planned"),
        )

    if name == "cccc_milestone_update":
        gid = _resolve_group_id(arguments)
        return milestone_update(
            group_id=gid,
            milestone_id=str(arguments.get("milestone_id") or ""),
            name=arguments.get("name"),
            description=arguments.get("description"),
            status=arguments.get("status"),
        )

    if name == "cccc_milestone_complete":
        gid = _resolve_group_id(arguments)
        return milestone_complete(
            group_id=gid,
            milestone_id=str(arguments.get("milestone_id") or ""),
            outcomes=str(arguments.get("outcomes") or ""),
        )

    if name == "cccc_task_list":
        gid = _resolve_group_id(arguments)
        return task_list(
            group_id=gid,
            task_id=arguments.get("task_id"),
            include_archived=coerce_bool(arguments.get("include_archived"), default=False),
        )

    if name == "cccc_task_create":
        gid = _resolve_group_id(arguments)
        steps_raw = arguments.get("steps")
        return task_create(
            group_id=gid,
            name=str(arguments.get("name") or ""),
            goal=str(arguments.get("goal") or ""),
            steps=list(steps_raw) if isinstance(steps_raw, list) else [],
            milestone_id=arguments.get("milestone_id"),
            assignee=arguments.get("assignee"),
        )

    if name == "cccc_task_update":
        gid = _resolve_group_id(arguments)
        return task_update(
            group_id=gid,
            task_id=str(arguments.get("task_id") or ""),
            status=arguments.get("status"),
            name=arguments.get("name"),
            goal=arguments.get("goal"),
            assignee=arguments.get("assignee"),
            milestone_id=arguments.get("milestone_id"),
            step_id=arguments.get("step_id"),
            step_status=arguments.get("step_status"),
        )

    if name == "cccc_note_add":
        gid = _resolve_group_id(arguments)
        return note_add(group_id=gid, content=str(arguments.get("content") or ""))

    if name == "cccc_note_update":
        gid = _resolve_group_id(arguments)
        return note_update(group_id=gid, note_id=str(arguments.get("note_id") or ""), content=arguments.get("content"))

    if name == "cccc_note_remove":
        gid = _resolve_group_id(arguments)
        return note_remove(group_id=gid, note_id=str(arguments.get("note_id") or ""))

    if name == "cccc_reference_add":
        gid = _resolve_group_id(arguments)
        return reference_add(group_id=gid, url=str(arguments.get("url") or ""), note=str(arguments.get("note") or ""))

    if name == "cccc_reference_update":
        gid = _resolve_group_id(arguments)
        return reference_update(
            group_id=gid,
            reference_id=str(arguments.get("reference_id") or ""),
            url=arguments.get("url"),
            note=arguments.get("note"),
        )

    if name == "cccc_reference_remove":
        gid = _resolve_group_id(arguments)
        return reference_remove(group_id=gid, reference_id=str(arguments.get("reference_id") or ""))

    if name == "cccc_presence_get":
        gid = _resolve_group_id(arguments)
        return presence_get(group_id=gid)

    if name == "cccc_presence_update":
        gid = _resolve_group_id(arguments)
        self_aid = _resolve_self_actor_id(arguments)
        agent_id = str(arguments.get("agent_id") or "").strip() or self_aid
        return presence_update(group_id=gid, agent_id=agent_id, status=str(arguments.get("status") or ""))

    if name == "cccc_presence_clear":
        gid = _resolve_group_id(arguments)
        self_aid = _resolve_self_actor_id(arguments)
        agent_id = str(arguments.get("agent_id") or "").strip() or self_aid
        return presence_clear(group_id=gid, agent_id=agent_id)

    return None


def _handle_headless_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if name == "cccc_headless_status":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return headless_status(group_id=gid, actor_id=aid)

    if name == "cccc_headless_set_status":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return headless_set_status(
            group_id=gid,
            actor_id=aid,
            status=str(arguments.get("status") or ""),
            task_id=arguments.get("task_id"),
        )

    if name == "cccc_headless_ack_message":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return headless_ack_message(group_id=gid, actor_id=aid, message_id=str(arguments.get("message_id") or ""))

    return None


def _handle_notify_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if name == "cccc_notify_send":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return notify_send(
            group_id=gid,
            actor_id=aid,
            kind=str(arguments.get("kind") or "info"),
            title=str(arguments.get("title") or ""),
            message=str(arguments.get("message") or ""),
            target_actor_id=arguments.get("target_actor_id"),
            priority=str(arguments.get("priority") or "normal"),
            requires_ack=coerce_bool(arguments.get("requires_ack"), default=False),
        )

    if name == "cccc_notify_ack":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return notify_ack(group_id=gid, actor_id=aid, notify_event_id=str(arguments.get("notify_event_id") or ""))

    return None


def _handle_terminal_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if name == "cccc_terminal_tail":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return terminal_tail(
            group_id=gid,
            actor_id=aid,
            target_actor_id=str(arguments.get("target_actor_id") or ""),
            max_chars=min(max(int(arguments.get("max_chars") or 8000), 1), 100000),
            strip_ansi=coerce_bool(arguments.get("strip_ansi"), default=True),
        )
    return None


def _handle_debug_namespace(name: str, arguments: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if name == "cccc_debug_snapshot":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return debug_snapshot(group_id=gid, actor_id=aid)

    if name == "cccc_debug_tail_logs":
        gid = _resolve_group_id(arguments)
        aid = _resolve_self_actor_id(arguments)
        return debug_tail_logs(
            group_id=gid,
            actor_id=aid,
            component=str(arguments.get("component") or ""),
            lines=min(max(int(arguments.get("lines") or 200), 1), 10000),
        )
    return None


def handle_tool_call(name: str, arguments: Dict[str, Any]) -> Dict[str, Any]:
    """Handle MCP tool call."""
    for handler in (
        _handle_cccc_namespace,
        _handle_context_namespace,
        _handle_headless_namespace,
        _handle_notify_namespace,
        _handle_terminal_namespace,
        _handle_debug_namespace,
    ):
        out = handler(name, arguments)
        if out is not None:
            return out
    raise MCPError(code="unknown_tool", message=f"unknown tool: {name}")
