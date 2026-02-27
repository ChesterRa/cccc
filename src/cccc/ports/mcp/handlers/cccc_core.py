"""MCP handler functions for core tools (help, inbox, bootstrap, project_info)."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict, List, Optional

from ....kernel.actors import get_effective_role
from ....kernel.group import load_group
from ....kernel.inbox import is_message_for_actor
from ....kernel.ledger import read_last_lines
from ....kernel.prompt_files import HELP_FILENAME, load_builtin_help_markdown as _load_builtin_help_markdown, read_group_prompt_file
from ..common import MCPError, _call_daemon_or_raise
from ..utils.help_markdown import _select_help_markdown
from . import cccc_group_actor as _group_actor_mod
from . import context as _context_mod


_CCCC_HELP_BUILTIN = _load_builtin_help_markdown().strip()


def _append_runtime_skill_digest(markdown: str, *, group_id: str, actor_id: str) -> str:
    base = str(markdown or "")
    if not base.strip():
        return base
    if "## Active Skills (Runtime)" in base:
        return base
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        return base
    try:
        state = _call_daemon_or_raise(
            {"op": "capability_state", "args": {"group_id": gid, "actor_id": aid, "by": aid}},
            timeout_s=3.0,
        )
    except Exception:
        return base
    active = state.get("active_skills") if isinstance(state, dict) else []
    pinned = state.get("pinned_skills") if isinstance(state, dict) else []
    active_list = active if isinstance(active, list) else []
    pinned_list = pinned if isinstance(pinned, list) else []
    if not active_list and not pinned_list:
        return base
    lines: List[str] = ["## Active Skills (Runtime)"]
    if pinned_list:
        lines.append("- pinned:")
        for item in pinned_list[:8]:
            if not isinstance(item, dict):
                continue
            sid = str(item.get("capability_id") or "").strip()
            name = str(item.get("name") or sid).strip()
            desc = str(item.get("description_short") or "").strip()
            line = f"  - {name} ({sid})"
            if desc:
                line += f": {desc[:120]}"
            lines.append(line)
    if active_list:
        lines.append("- active_now:")
        for item in active_list[:8]:
            if not isinstance(item, dict):
                continue
            sid = str(item.get("capability_id") or "").strip()
            name = str(item.get("name") or sid).strip()
            desc = str(item.get("description_short") or "").strip()
            line = f"  - {name} ({sid})"
            if desc:
                line += f": {desc[:120]}"
            lines.append(line)
    return base.rstrip() + "\n\n" + "\n".join(lines).rstrip() + "\n"


def inbox_list(*, group_id: str, actor_id: str, limit: int = 50, kind_filter: str = "all") -> Dict[str, Any]:
    return _call_daemon_or_raise(
        {"op": "inbox_list", "args": {"group_id": group_id, "actor_id": actor_id, "by": actor_id, "limit": limit, "kind_filter": kind_filter}},
    )


def inbox_mark_read(*, group_id: str, actor_id: str, event_id: str) -> Dict[str, Any]:
    eid = str(event_id or "").strip()
    if not eid:
        raise MCPError(code="missing_event_id", message="missing event_id")
    return _call_daemon_or_raise(
        {"op": "inbox_mark_read", "args": {"group_id": group_id, "actor_id": actor_id, "event_id": eid, "by": actor_id}},
    )


def inbox_mark_all_read(*, group_id: str, actor_id: str, kind_filter: str = "all") -> Dict[str, Any]:
    return _call_daemon_or_raise(
        {"op": "inbox_mark_all_read", "args": {"group_id": group_id, "actor_id": actor_id, "kind_filter": kind_filter, "by": actor_id}},
    )


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
    gi = _group_actor_mod.group_info(group_id=group_id)
    group = gi.get("group") if isinstance(gi, dict) else None

    al = _group_actor_mod.actor_list(group_id=group_id)
    actors = al.get("actors") if isinstance(al, dict) else None

    help_payload: Dict[str, Any] = {
        "markdown": _append_runtime_skill_digest(
            _select_help_markdown(_CCCC_HELP_BUILTIN, role=None, actor_id=None),
            group_id=group_id,
            actor_id=actor_id,
        ),
        "source": "cccc.resources/cccc-help.md",
    }
    try:
        g = load_group(str(group_id or "").strip())
        if g is not None:
            role = get_effective_role(g, str(actor_id or "").strip())
            pf = read_group_prompt_file(g, HELP_FILENAME)
            if pf.found and isinstance(pf.content, str) and pf.content.strip():
                help_payload = {
                    "markdown": _append_runtime_skill_digest(
                        _select_help_markdown(pf.content, role=role, actor_id=actor_id),
                        group_id=group_id,
                        actor_id=actor_id,
                    ),
                    "source": str(pf.path or ""),
                }
            else:
                help_payload = {
                    "markdown": _append_runtime_skill_digest(
                        _select_help_markdown(_CCCC_HELP_BUILTIN, role=role, actor_id=actor_id),
                        group_id=group_id,
                        actor_id=actor_id,
                    ),
                    "source": "cccc.resources/cccc-help.md",
                }
    except Exception:
        pass

    project = project_info(group_id=group_id)
    context = _context_mod.context_get(group_id=group_id)
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

    memory_guide: Optional[str] = None
    try:
        from ....kernel.memory_guide import build_memory_guide
        memory_guide = build_memory_guide(group_id=group_id)
    except Exception:
        pass

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
        "memory_guide": memory_guide,
    }


def project_info(*, group_id: str) -> Dict[str, Any]:
    """Get PROJECT.md content for the group's active scope"""
    group = load_group(group_id)
    if group is None:
        raise MCPError(code="group_not_found", message=f"group not found: {group_id}")

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
        if scopes and isinstance(scopes[0], dict):
            project_root = str(scopes[0].get("url") or "")

    if not project_root:
        return {
            "found": False,
            "path": None,
            "content": None,
            "error": "No scope attached to group. Use 'cccc attach <path>' first.",
        }

    project_md_path = Path(project_root) / "PROJECT.md"
    if not project_md_path.exists():
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
