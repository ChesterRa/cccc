"""MCP handler functions for core tools (help, inbox, bootstrap, project_info)."""

from __future__ import annotations

from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

from ....kernel.group import load_group
from ....kernel.prompt_files import load_builtin_help_markdown as _load_builtin_help_markdown
from ....util.time import parse_utc_iso
from ..common import MCPError, _call_daemon_or_raise
from . import cccc_group_actor as _group_actor_mod
from . import context as _context_mod


_CCCC_HELP_BUILTIN = _load_builtin_help_markdown().strip()

_PACK_QUICK_USE_EXAMPLES: Dict[str, str] = {
    "pack:space": 'cccc_capability_use(tool_name="cccc_space", tool_arguments={"action":"status"})',
    "pack:group-runtime": 'cccc_capability_use(tool_name="cccc_group", tool_arguments={"action":"info"})',
    "pack:file-im": 'cccc_capability_use(tool_name="cccc_file", tool_arguments={"action":"blob_path","rel_path":"state/blobs/..."})',
    "pack:automation": 'cccc_capability_use(tool_name="cccc_automation", tool_arguments={"action":"state"})',
    "pack:context-advanced": 'cccc_capability_use(tool_name="cccc_memory_admin", tool_arguments={"action":"index_sync","mode":"scan"})',
}


def _trim_text(value: Any, *, max_chars: int) -> str:
    text = str(value or "").strip()
    if len(text) <= max_chars:
        return text
    if max_chars <= 3:
        return text[:max_chars]
    return text[: max_chars - 3].rstrip() + "..."


def _find_actor_state(*, context: Dict[str, Any], actor_id: str) -> Optional[Dict[str, Any]]:
    states = context.get("agent_states") if isinstance(context.get("agent_states"), list) else []
    target = str(actor_id or "").strip().lower()
    for item in states:
        if not isinstance(item, dict):
            continue
        if str(item.get("id") or "").strip().lower() == target:
            return item
    return None


def _memory_recall_query_from_context(*, context: Dict[str, Any], actor_id: str) -> str:
    actor_state = context.get("agent_state") if isinstance(context.get("agent_state"), dict) else {}
    hot = actor_state.get("hot") if isinstance(actor_state.get("hot"), dict) else {}
    warm = actor_state.get("warm") if isinstance(actor_state.get("warm"), dict) else {}
    brief = context.get("coordination_brief") if isinstance(context.get("coordination_brief"), dict) else {}
    tasks = context.get("tasks") if isinstance(context.get("tasks"), dict) else {}

    ranked: List[tuple[int, str]] = []
    seen: set[str] = set()

    def _add(priority: int, value: Any, *, max_chars: int = 120) -> None:
        text = _trim_text(value, max_chars=max_chars)
        if not text:
            return
        normalized = text.casefold()
        if normalized in seen:
            return
        seen.add(normalized)
        ranked.append((priority, text))

    _add(0, hot.get("active_task_id"), max_chars=48)
    _add(1, hot.get("focus"), max_chars=120)
    _add(2, hot.get("next_action"), max_chars=120)
    _add(3, brief.get("current_focus"), max_chars=120)

    assigned_active = tasks.get("assigned_active") if isinstance(tasks.get("assigned_active"), list) else []
    attention = tasks.get("attention") if isinstance(tasks.get("attention"), list) else []
    if assigned_active and isinstance(assigned_active[0], dict):
        _add(4, assigned_active[0].get("title"), max_chars=100)
        _add(5, assigned_active[0].get("outcome"), max_chars=120)
    if attention and isinstance(attention[0], dict):
        _add(8, attention[0].get("title"), max_chars=100)

    _add(6, warm.get("what_changed"), max_chars=120)
    _add(7, warm.get("resume_hint"), max_chars=120)
    _add(9, brief.get("objective"), max_chars=120)
    _add(10, warm.get("environment_summary"), max_chars=100)
    _add(11, warm.get("user_model"), max_chars=100)
    _add(12, warm.get("persona_notes"), max_chars=100)

    if not ranked:
        return "recent decisions constraints preferences"
    ranked.sort(key=lambda item: item[0])
    merged = " | ".join(text for _, text in ranked[:6]).strip()
    return _trim_text(merged, max_chars=240) or "recent decisions constraints preferences"


def _build_memory_recall_gate(*, group_id: str, actor_id: str, context: Dict[str, Any]) -> Dict[str, Any]:
    query = _memory_recall_query_from_context(context=context, actor_id=actor_id)
    gate: Dict[str, Any] = {
        "required": True,
        "status": "empty",
        "query": query,
        "hits": [],
        "note": (
            "Recall gate: read bootstrap.memory_recall_gate before planning/implementation. "
            "If empty, run cccc_memory(search/get) manually."
        ),
    }
    try:
        search_result = _call_daemon_or_raise(
            {
                "op": "memory_reme_search",
                "args": {
                    "group_id": group_id,
                    "query": query,
                    "max_results": 3,
                    "min_score": 0.1,
                },
            },
            timeout_s=4.0,
        )
        hits = search_result.get("hits") if isinstance(search_result, dict) else []
        compact_hits: List[Dict[str, Any]] = []
        if isinstance(hits, list):
            for item in hits[:3]:
                if not isinstance(item, dict):
                    continue
                compact_hits.append(
                    {
                        "path": str(item.get("path") or ""),
                        "start_line": int(item.get("start_line") or 1),
                        "score": float(item.get("score") or 0.0),
                        "snippet": _trim_text(item.get("snippet"), max_chars=220),
                    }
                )
        gate["hits"] = compact_hits
        gate["status"] = "ready" if compact_hits else "empty"
    except Exception as e:
        gate["status"] = "error"
        gate["error"] = str(e)
    return gate


def _build_context_hygiene_hint(*, context: Dict[str, Any], actor_id: str) -> Dict[str, Any]:
    aid = str(actor_id or "").strip()
    hint: Dict[str, Any] = {
        "actor_id": aid,
        "present": False,
        "stale": True,
        "age_seconds": None,
        "min_fields_ready": False,
        "update_command": (
            'cccc_agent_state(action="update", actor_id="<self>", '
            'focus="...", next_action="...", what_changed="...")'
        ),
        "recommendation": "update_agent_state_now",
    }
    if not aid or not isinstance(context, dict):
        return hint
    target = _find_actor_state(context=context, actor_id=aid)
    if target is None:
        return hint
    hint["present"] = True
    hot = target.get("hot") if isinstance(target.get("hot"), dict) else {}
    warm = target.get("warm") if isinstance(target.get("warm"), dict) else {}
    blockers = hot.get("blockers") if isinstance(hot.get("blockers"), list) else []
    min_fields_ready = any(
        str(value or "").strip()
        for value in (
            hot.get("focus"),
            hot.get("next_action"),
            warm.get("what_changed"),
            warm.get("resume_hint"),
        )
    ) or bool(blockers)
    hint["min_fields_ready"] = bool(min_fields_ready)
    updated_at = str(target.get("updated_at") or "").strip()
    age_seconds: Optional[int] = None
    if updated_at:
        dt = parse_utc_iso(updated_at)
        if dt is not None:
            age_seconds = max(0, int((datetime.now(timezone.utc) - dt).total_seconds()))
    hint["age_seconds"] = age_seconds
    stale = (age_seconds is None) or (age_seconds > 20 * 60)
    hint["stale"] = bool(stale)
    if (not stale) and min_fields_ready:
        hint["recommendation"] = "state_healthy"
    elif stale and min_fields_ready:
        hint["recommendation"] = "refresh_agent_state"
    else:
        hint["recommendation"] = "fill_agent_state_basics"
    return hint


def _estimate_payload_tokens(value: Any) -> int:
    try:
        return max(1, len(json.dumps(value, ensure_ascii=False, sort_keys=True)) // 4)
    except Exception:
        return max(1, len(str(value or "")) // 4)


def _slim_task_for_bootstrap(task: Dict[str, Any]) -> Dict[str, Any]:
    checklist = task.get("checklist") if isinstance(task.get("checklist"), list) else []
    slim = {
        "id": str(task.get("id") or ""),
        "title": _trim_text(task.get("title"), max_chars=120),
        "outcome": _trim_text(task.get("outcome"), max_chars=160),
        "status": str(task.get("status") or ""),
        "assignee": str(task.get("assignee") or ""),
        "priority": str(task.get("priority") or ""),
        "waiting_on": str(task.get("waiting_on") or "none"),
        "handoff_to": str(task.get("handoff_to") or ""),
        "notes": _trim_text(task.get("notes"), max_chars=240),
        "checklist": [
            {
                "id": str(item.get("id") or ""),
                "text": _trim_text(item.get("text"), max_chars=120),
                "status": str(item.get("status") or "pending"),
            }
            for item in checklist[:3]
            if isinstance(item, dict)
        ],
        "updated_at": task.get("updated_at"),
    }
    blocked_by = task.get("blocked_by") if isinstance(task.get("blocked_by"), list) else []
    if blocked_by:
        slim["blocked_by"] = [str(x) for x in blocked_by[:4]]
    return {key: value for key, value in slim.items() if value not in (None, "", [], {})}


def _build_bootstrap_context(*, context: Dict[str, Any], actor_id: str) -> Dict[str, Any]:
    actor_state = _find_actor_state(context=context, actor_id=actor_id) or {
        "id": str(actor_id or "").strip(),
        "hot": {},
        "warm": {},
        "updated_at": None,
    }
    coordination = context.get("coordination") if isinstance(context.get("coordination"), dict) else {}
    brief = coordination.get("brief") if isinstance(coordination.get("brief"), dict) else {}
    tasks = coordination.get("tasks") if isinstance(coordination.get("tasks"), list) else []
    recent_decisions = coordination.get("recent_decisions") if isinstance(coordination.get("recent_decisions"), list) else []
    recent_handoffs = coordination.get("recent_handoffs") if isinstance(coordination.get("recent_handoffs"), list) else []
    aid = str(actor_id or "").strip()

    assigned_active: List[Dict[str, Any]] = []
    actor_attention: List[Dict[str, Any]] = []
    waiting_user: List[Dict[str, Any]] = []
    global_blocked: List[Dict[str, Any]] = []

    for raw in tasks:
        if not isinstance(raw, dict):
            continue
        status = str(raw.get("status") or "").strip().lower()
        if status in {"done", "archived"}:
            continue
        assignee = str(raw.get("assignee") or "").strip()
        waiting_on = str(raw.get("waiting_on") or "none").strip().lower()
        handoff_to = str(raw.get("handoff_to") or "").strip()
        blocked_by = raw.get("blocked_by") if isinstance(raw.get("blocked_by"), list) else []
        slim = _slim_task_for_bootstrap(raw)
        if assignee == aid and status == "active":
            assigned_active.append(slim)
            continue
        if assignee == aid or handoff_to == aid or waiting_on == "actor":
            actor_attention.append(slim)
            continue
        if waiting_on == "user":
            waiting_user.append(slim)
            continue
        if blocked_by or waiting_on == "external":
            global_blocked.append(slim)

    seen: set[str] = set()

    def _take(items: List[Dict[str, Any]], limit: int) -> List[Dict[str, Any]]:
        picked: List[Dict[str, Any]] = []
        for item in items:
            tid = str(item.get("id") or "")
            if not tid or tid in seen:
                continue
            seen.add(tid)
            picked.append(item)
            if len(picked) >= limit:
                break
        return picked

    primary = _take(assigned_active, 3)
    attention = _take(actor_attention, 2)
    attention.extend(_take(waiting_user, max(0, 3 - len(attention))))
    attention.extend(_take(global_blocked, max(0, 3 - len(attention))))

    raw_hot = actor_state.get("hot") if isinstance(actor_state.get("hot"), dict) else {}
    raw_warm = actor_state.get("warm") if isinstance(actor_state.get("warm"), dict) else {}
    warm = {
        "what_changed": _trim_text(raw_warm.get("what_changed"), max_chars=180),
        "resume_hint": _trim_text(raw_warm.get("resume_hint"), max_chars=180),
        "environment_summary": _trim_text(raw_warm.get("environment_summary"), max_chars=160),
        "user_model": _trim_text(raw_warm.get("user_model"), max_chars=160),
        "persona_notes": _trim_text(raw_warm.get("persona_notes"), max_chars=160),
    }
    warm = {key: value for key, value in warm.items() if value}

    pack = {
        "agent_state": {
            "id": actor_state.get("id"),
            "hot": raw_hot if isinstance(raw_hot, dict) else {},
            "warm": warm,
            "updated_at": actor_state.get("updated_at"),
        },
        "coordination_brief": {
            "objective": _trim_text(brief.get("objective"), max_chars=180),
            "current_focus": _trim_text(brief.get("current_focus"), max_chars=180),
            "constraints": [str(x) for x in (brief.get("constraints") if isinstance(brief.get("constraints"), list) else [])[:6]],
            "project_brief": _trim_text(brief.get("project_brief"), max_chars=260),
            "project_brief_stale": bool(brief.get("project_brief_stale")),
        },
        "tasks": {
            "assigned_active": primary,
            "attention": attention,
        },
        "recent_decisions": [
            {
                "at": item.get("at"),
                "by": item.get("by"),
                "summary": _trim_text(item.get("summary"), max_chars=180),
                "task_id": item.get("task_id"),
            }
            for item in recent_decisions[:2]
            if isinstance(item, dict)
        ],
        "recent_handoffs": [
            {
                "at": item.get("at"),
                "by": item.get("by"),
                "summary": _trim_text(item.get("summary"), max_chars=180),
                "task_id": item.get("task_id"),
            }
            for item in recent_handoffs[:2]
            if isinstance(item, dict)
        ],
    }

    hard_cap = 1100
    optional_warm_drop_order = ("persona_notes", "user_model", "environment_summary")
    while _estimate_payload_tokens(pack) > hard_cap and pack["agent_state"]["warm"]:
        dropped = False
        for field in optional_warm_drop_order:
            if field in pack["agent_state"]["warm"]:
                pack["agent_state"]["warm"].pop(field, None)
                dropped = True
                break
        if not dropped:
            break
    while _estimate_payload_tokens(pack) > hard_cap and pack["recent_handoffs"]:
        pack["recent_handoffs"].pop()
    while _estimate_payload_tokens(pack) > hard_cap and pack["recent_decisions"]:
        pack["recent_decisions"].pop()
    while _estimate_payload_tokens(pack) > hard_cap and len(pack["tasks"]["attention"]) > 1:
        pack["tasks"]["attention"].pop()
    while _estimate_payload_tokens(pack) > hard_cap and len(pack["tasks"]["assigned_active"]) > 1:
        pack["tasks"]["assigned_active"].pop()
    if _estimate_payload_tokens(pack) > hard_cap:
        for bucket in ("assigned_active", "attention"):
            for item in pack["tasks"][bucket]:
                item.pop("notes", None)
                if isinstance(item.get("checklist"), list) and len(item["checklist"]) > 1:
                    item["checklist"] = item["checklist"][:1]
    return pack


def _select_bootstrap_scope(group: Dict[str, Any]) -> Dict[str, Any]:
    active_scope_key = str(group.get("active_scope_key") or "").strip()
    scopes = group.get("scopes") if isinstance(group.get("scopes"), list) else []
    selected: Dict[str, Any] = {}
    for item in scopes:
        if not isinstance(item, dict):
            continue
        if active_scope_key and str(item.get("scope_key") or "").strip() == active_scope_key:
            selected = item
            break
    if not selected:
        for item in scopes:
            if isinstance(item, dict):
                selected = item
                break
    if not selected:
        return {}
    return {
        "scope_key": str(selected.get("scope_key") or "").strip(),
        "path": str(selected.get("url") or "").strip(),
    }


def _build_bootstrap_session(*, group: Dict[str, Any], actors: List[Dict[str, Any]], actor_id: str, project: Dict[str, Any]) -> Dict[str, Any]:
    current_actor = next(
        (item for item in actors if isinstance(item, dict) and str(item.get("id") or "").strip() == str(actor_id or "").strip()),
        {},
    )
    return {
        "group_id": str(group.get("group_id") or "").strip(),
        "group_title": str(group.get("title") or group.get("group_id") or "").strip(),
        "actor_id": str(actor_id or "").strip(),
        "role": str(current_actor.get("role") or "").strip(),
        "runner": str(current_actor.get("runner") or "").strip(),
        "active_scope": _select_bootstrap_scope(group),
        "project_md": {
            "found": bool(project.get("found")),
            "path": project.get("path"),
        },
    }


def _build_bootstrap_recovery(*, pack: Dict[str, Any]) -> Dict[str, Any]:
    agent_state = pack.get("agent_state") if isinstance(pack.get("agent_state"), dict) else {}
    return {
        "coordination_brief": pack.get("coordination_brief") if isinstance(pack.get("coordination_brief"), dict) else {},
        "self_state": {
            "hot": agent_state.get("hot") if isinstance(agent_state.get("hot"), dict) else {},
            "recovery": agent_state.get("warm") if isinstance(agent_state.get("warm"), dict) else {},
            "updated_at": agent_state.get("updated_at"),
        },
        "task_slice": pack.get("tasks") if isinstance(pack.get("tasks"), dict) else {"assigned_active": [], "attention": []},
        "recent_notes": {
            "decisions": pack.get("recent_decisions") if isinstance(pack.get("recent_decisions"), list) else [],
            "handoffs": pack.get("recent_handoffs") if isinstance(pack.get("recent_handoffs"), list) else [],
        },
    }


def _build_bootstrap_inbox_preview(*, inbox: Dict[str, Any], limit: int) -> Dict[str, Any]:
    raw_messages = inbox.get("messages") if isinstance(inbox.get("messages"), list) else []
    has_more = bool(limit > 0 and len(raw_messages) > int(limit))
    source_messages = raw_messages[: max(0, int(limit))] if limit > 0 else raw_messages
    preview: List[Dict[str, Any]] = []
    for item in source_messages:
        if not isinstance(item, dict):
            continue
        data = item.get("data") if isinstance(item.get("data"), dict) else {}
        text_source = (
            data.get("text")
            if isinstance(data.get("text"), str) and str(data.get("text") or "").strip()
            else data.get("message")
            if isinstance(data.get("message"), str) and str(data.get("message") or "").strip()
            else data.get("title")
        )
        preview.append(
            {
                "id": str(item.get("id") or ""),
                "ts": str(item.get("ts") or ""),
                "by": str(item.get("by") or ""),
                "kind": str(item.get("kind") or ""),
                "reply_required": bool(data.get("reply_required") is True or data.get("requires_ack") is True),
                "text_preview": _trim_text(text_source, max_chars=220),
            }
        )
    return {
        "messages": preview,
        "truncated": has_more,
    }


def _append_runtime_skill_digest(markdown: str, *, group_id: str, actor_id: str) -> str:
    base = str(markdown or "")
    if not base.strip():
        return base
    if "## Active Skills (Runtime)" in base or "## Capability Quick Use (Runtime)" in base:
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
    enabled_caps = state.get("enabled_capabilities") if isinstance(state, dict) else []
    hidden_caps = state.get("hidden_capabilities") if isinstance(state, dict) else []
    active = state.get("active_capsule_skills") if isinstance(state, dict) else []
    autoload = state.get("autoload_skills") if isinstance(state, dict) else []
    enabled_list = enabled_caps if isinstance(enabled_caps, list) else []
    hidden_list = hidden_caps if isinstance(hidden_caps, list) else []
    active_list = active if isinstance(active, list) else []
    autoload_list = autoload if isinstance(autoload, list) else []
    sections: List[str] = []

    if active_list or autoload_list:
        lines: List[str] = ["## Active Skills (Runtime)"]
        if autoload_list:
            lines.append("- autoload:")
            for item in autoload_list[:8]:
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
        sections.append("\n".join(lines).rstrip())

    enabled_packs = [
        str(x).strip()
        for x in enabled_list
        if str(x).strip().startswith("pack:")
    ]
    suggested_packs: List[str] = []
    seen: set[str] = set()
    for item in hidden_list:
        if not isinstance(item, dict):
            continue
        cid = str(item.get("capability_id") or "").strip()
        reason = str(item.get("reason") or "").strip().lower()
        if not cid.startswith("pack:"):
            continue
        if reason not in {"not_enabled", "scope_mismatch"}:
            continue
        if cid in seen:
            continue
        seen.add(cid)
        suggested_packs.append(cid)
        if len(suggested_packs) >= 4:
            break
    if not suggested_packs:
        for cid in ("pack:space", "pack:file-im", "pack:group-runtime", "pack:context-advanced"):
            if cid in seen:
                continue
            seen.add(cid)
            suggested_packs.append(cid)
            if len(suggested_packs) >= 4:
                break

    if enabled_packs or suggested_packs:
        lines_cap: List[str] = [
            "## Capability Quick Use (Runtime)",
            '- list packs quickly: `cccc_capability_search(kind="mcp_toolpack")`',
        ]
        if enabled_packs:
            lines_cap.append(
                "- enabled_packs: " + ", ".join(enabled_packs[:8])
            )
        if suggested_packs:
            lines_cap.append("- one-step examples:")
            for cid in suggested_packs:
                example = _PACK_QUICK_USE_EXAMPLES.get(cid)
                if example:
                    lines_cap.append(f"  - {cid}: `{example}`")
                else:
                    lines_cap.append(
                        f'  - {cid}: `cccc_capability_use(capability_id="{cid}", scope="session")`'
                    )
        sections.append("\n".join(lines_cap).rstrip())

    if not sections:
        return base
    return base.rstrip() + "\n\n" + "\n\n".join(sections).rstrip() + "\n"


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
) -> Dict[str, Any]:
    """One-call cold-start bootstrap for agents.

    Default payload is the minimum hot recovery packet:
    - session
    - recovery
    - inbox preview
    - memory recall gate
    - next-call hints for cold data
    """
    gi = _group_actor_mod.group_info(group_id=group_id)
    group = gi.get("group") if isinstance(gi, dict) and isinstance(gi.get("group"), dict) else {}

    al = _group_actor_mod.actor_list(group_id=group_id)
    actors = al.get("actors") if isinstance(al, dict) and isinstance(al.get("actors"), list) else []

    project_full = project_info(group_id=group_id)
    project = {
        "found": bool(project_full.get("found")),
        "path": project_full.get("path"),
    }
    context_full = _context_mod.context_get(group_id=group_id, include_archived=True)
    recovery_pack = _build_bootstrap_context(
        context=context_full if isinstance(context_full, dict) else {},
        actor_id=actor_id,
    )
    preview_limit = max(1, int(inbox_limit or 50))
    inbox = inbox_list(group_id=group_id, actor_id=actor_id, limit=preview_limit + 1, kind_filter=inbox_kind_filter)

    memory_recall_gate = _build_memory_recall_gate(
        group_id=group_id,
        actor_id=actor_id,
        context=recovery_pack if isinstance(recovery_pack, dict) else {},
    )

    return {
        "session": _build_bootstrap_session(
            group=group if isinstance(group, dict) else {},
            actors=[item for item in actors if isinstance(item, dict)],
            actor_id=actor_id,
            project=project,
        ),
        "recovery": _build_bootstrap_recovery(pack=recovery_pack if isinstance(recovery_pack, dict) else {}),
        "inbox_preview": _build_bootstrap_inbox_preview(
            inbox=inbox if isinstance(inbox, dict) else {},
            limit=int(inbox_limit or 50),
        ),
        "memory_recall_gate": memory_recall_gate,
        "next_calls": {
            "help": "cccc_help()",
            "project_info": "cccc_project_info()",
            "context_get": "cccc_context_get()",
            "memory_search": 'cccc_memory(action="search", query=...)',
        },
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
