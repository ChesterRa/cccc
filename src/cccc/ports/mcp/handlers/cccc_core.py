"""MCP handler functions for core tools (help, inbox, bootstrap, project_info)."""

from __future__ import annotations

import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

from ....daemon.memory.experience_assets import query_experience_recall
from ....kernel.agent_state_hygiene import build_mind_context_mini, evaluate_agent_state_hygiene
from ....kernel.experience_assets import select_experience_assets_for_consumption
from ....kernel.group import load_group
from ....kernel.procedural_skills import select_procedural_skills_for_consumption
from ....kernel.group_space import get_group_space_prompt_state
from ....kernel.prompt_files import load_builtin_help_markdown as _load_builtin_help_markdown
from ....util.fs import read_json
from ..common import MCPError, _call_daemon_or_raise
from . import cccc_group_actor as _group_actor_mod
from . import context as _context_mod


_CCCC_HELP_BUILTIN = _load_builtin_help_markdown().strip()
_RUNTIME_HELP_SECTION_HEADERS = {
    "## Active Skills (Runtime)",
    "## Group Space (Runtime)",
}

def _trim_text(value: Any, *, max_chars: int) -> str:
    text = str(value or "").strip()
    if len(text) <= max_chars:
        return text
    if max_chars <= 3:
        return text[:max_chars]
    return text[: max_chars - 3].rstrip() + "..."


_BOOTSTRAP_INTERRUPT_NOTIFY_KINDS = {
    "actor_idle",
    "auto_idle",
    "automation",
    "help_nudge",
    "keepalive",
    "nudge",
    "pet_review",
    "silence_check",
    "standup",
}


def _bootstrap_signal_family(*, item: Dict[str, Any], data: Dict[str, Any]) -> str:
    kind = str(item.get("kind") or "").strip()
    if kind == "chat.message":
        return "work_chat"
    if kind == "system.notify":
        notify_kind = str(data.get("kind") or "").strip().lower()
        if data.get("requires_ack") is True or notify_kind in _BOOTSTRAP_INTERRUPT_NOTIFY_KINDS:
            return "interrupt"
        return "notify"
    return "other"


def _strip_reserved_runtime_help_sections(markdown: str) -> str:
    raw = str(markdown or "")
    if not raw:
        return raw
    keep_trailing_newline = raw.endswith("\n")
    lines = raw.splitlines()
    out: List[str] = []
    current: List[str] = []
    skip_current = False

    def _flush() -> None:
        nonlocal current
        if current and not skip_current:
            out.extend(current)
        current = []

    for line in lines:
        stripped = str(line or "").strip()
        is_h2 = stripped.startswith("## ") and not stripped.startswith("###")
        if is_h2:
            _flush()
            skip_current = stripped in _RUNTIME_HELP_SECTION_HEADERS
        current.append(line)
    _flush()

    result = "\n".join(out)
    if keep_trailing_newline:
        result += "\n"
    return result


def _find_actor_state(*, context: Dict[str, Any], actor_id: str) -> Optional[Dict[str, Any]]:
    states = context.get("agent_states") if isinstance(context.get("agent_states"), list) else []
    target = str(actor_id or "").strip().lower()
    for item in states:
        if not isinstance(item, dict):
            continue
        if str(item.get("id") or "").strip().lower() == target:
            return item
    return None


def _load_actor_mind_context_runtime(*, group_id: str, actor_id: str) -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    if not gid or not aid:
        return {}
    group = load_group(gid)
    if group is None:
        return {}
    state = read_json(group.path / "state" / "automation.json")
    actors = state.get("actors") if isinstance(state.get("actors"), dict) else {}
    actor_state = actors.get(aid)
    return actor_state if isinstance(actor_state, dict) else {}


def _memory_recall_query_from_context(*, context: Dict[str, Any], actor_id: str) -> str:
    actor_state = context.get("agent_state") if isinstance(context.get("agent_state"), dict) else {}
    hot = actor_state.get("hot") if isinstance(actor_state.get("hot"), dict) else {}
    warm = actor_state.get("warm") if isinstance(actor_state.get("warm"), dict) else {}
    mini = actor_state.get("mind_context_mini") if isinstance(actor_state.get("mind_context_mini"), dict) else {}
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
    _add(10, warm.get("environment_summary") or mini.get("environment_summary"), max_chars=100)
    _add(11, warm.get("user_model") or mini.get("user_model"), max_chars=100)
    _add(12, warm.get("persona_notes") or mini.get("persona_notes"), max_chars=100)

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
        "task_refs": [],
        "decision_refs": [],
        "experience": {"promoted": [], "candidates": [], "promoted_top_score": 0.0},
        "experience_assets": [],
        "procedural_skills": [],
        "note": (
            "Recall gate: read this before planning or implementation. "
            "If it is empty, expand with local cccc_memory(search/get)."
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
        recall = query_experience_recall(
            group_id=group_id,
            query=query,
            context=context if isinstance(context, dict) else {},
            memory_hits=hits,
        )
        memory_hits = recall.get("memory_hits") if isinstance(recall.get("memory_hits"), list) else []
        gate["hits"] = memory_hits
        gate["task_refs"] = recall.get("task_refs") if isinstance(recall.get("task_refs"), list) else []
        gate["decision_refs"] = recall.get("decision_refs") if isinstance(recall.get("decision_refs"), list) else []
        gate["experience"] = recall.get("experience") if isinstance(recall.get("experience"), dict) else {"promoted": [], "candidates": [], "promoted_top_score": 0.0}
        group = load_group(group_id)
        if group is not None:
            assets = select_experience_assets_for_consumption(group, query=query, limit=3)
            gate["experience_assets"] = [
                {
                    "asset_id": str(item.get("asset_id") or "").strip(),
                    "candidate_id": str(item.get("candidate_id") or "").strip(),
                    "title": str(item.get("title") or "").strip(),
                    "summary": str(item.get("summary") or "").strip(),
                    "recommended_action": str(item.get("recommended_action") or "").strip(),
                    "memory_entry_id": str(item.get("memory_entry_id") or "").strip(),
                    "score": float(item.get("score") or 0.0),
                }
                for item in assets
            ]
            skills = select_procedural_skills_for_consumption(group, query=query, limit=3)
            gate["procedural_skills"] = [
                {
                    "skill_id": str(item.get("skill_id") or "").strip(),
                    "source_experience_candidate_id": str(item.get("source_experience_candidate_id") or "").strip(),
                    "title": str(item.get("title") or "").strip(),
                    "goal": str(item.get("goal") or "").strip(),
                    "steps": [str(step or "").strip() for step in (item.get("steps") if isinstance(item.get("steps"), list) else []) if str(step or "").strip()],
                    "memory_entry_id": str(item.get("memory_entry_id") or "").strip(),
                    "score": float(item.get("score") or 0.0),
                }
                for item in skills
            ]
        has_any = bool(recall.get("has_any"))
        high_promoted = bool(recall.get("has_high_relevance_promoted"))
        has_assets = bool(gate.get("experience_assets"))
        has_skills = bool(gate.get("procedural_skills"))
        gate["status"] = "ready" if (has_any or high_promoted or has_assets or has_skills) else "empty"
    except Exception as e:
        gate["status"] = "error"
        gate["error"] = str(e)
    return gate


def _build_context_hygiene_hint(*, context: Dict[str, Any], actor_id: str, group_id: str = "") -> Dict[str, Any]:
    aid = str(actor_id or "").strip()
    hint: Dict[str, Any] = evaluate_agent_state_hygiene(
        actor_id=aid,
        hot={},
        warm={},
        updated_at=None,
        present=False,
    )
    if not aid or not isinstance(context, dict):
        return hint
    target = _find_actor_state(context=context, actor_id=aid)
    if target is None:
        return hint
    hot = target.get("hot") if isinstance(target.get("hot"), dict) else {}
    warm = target.get("warm") if isinstance(target.get("warm"), dict) else {}
    runtime_meta = _load_actor_mind_context_runtime(group_id=group_id, actor_id=aid)
    return evaluate_agent_state_hygiene(
        actor_id=aid,
        hot=hot,
        warm=warm,
        updated_at=target.get("updated_at"),
        mind_touched_at=runtime_meta.get("mind_context_touched_at"),
        hot_only_updates_since_mind_touch=int(runtime_meta.get("hot_only_updates_since_mind_touch") or 0),
        present=True,
        now=datetime.now(timezone.utc),
    )


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
        "parent_id": str(task.get("parent_id") or ""),
        "status": str(task.get("status") or ""),
        "assignee": str(task.get("assignee") or ""),
        "priority": str(task.get("priority") or ""),
        "waiting_on": str(task.get("waiting_on") or "none"),
        "handoff_to": str(task.get("handoff_to") or ""),
        "task_type": str(task.get("task_type") or "").strip(),
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


def _build_bootstrap_context(*, context: Dict[str, Any], actor_id: str, group_id: str = "") -> Dict[str, Any]:
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
    runtime_meta = _load_actor_mind_context_runtime(
        group_id=group_id,
        actor_id=str(actor_state.get("id") or actor_id or "").strip(),
    )
    mind_context_mini = build_mind_context_mini(raw_warm, max_chars=84)
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
            "mind_context_mini": mind_context_mini,
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
        "context_hygiene": evaluate_agent_state_hygiene(
            actor_id=str(actor_state.get("id") or actor_id or "").strip(),
            hot=raw_hot,
            warm=raw_warm,
            updated_at=actor_state.get("updated_at"),
            mind_touched_at=runtime_meta.get("mind_context_touched_at"),
            hot_only_updates_since_mind_touch=int(runtime_meta.get("hot_only_updates_since_mind_touch") or 0),
            present=True,
            now=datetime.now(timezone.utc),
        ),
    }

    hard_cap = 1100
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
    if _estimate_payload_tokens(pack) > hard_cap:
        brief_pack = pack.get("coordination_brief") if isinstance(pack.get("coordination_brief"), dict) else {}
        brief_pack.pop("project_brief", None)
    if _estimate_payload_tokens(pack) > hard_cap:
        compact_warm = {
            "what_changed": _trim_text(raw_warm.get("what_changed"), max_chars=96),
            "resume_hint": _trim_text(raw_warm.get("resume_hint"), max_chars=96),
            "environment_summary": _trim_text(raw_warm.get("environment_summary"), max_chars=72),
            "user_model": _trim_text(raw_warm.get("user_model"), max_chars=72),
            "persona_notes": _trim_text(raw_warm.get("persona_notes"), max_chars=72),
        }
        pack["agent_state"]["warm"] = {key: value for key, value in compact_warm.items() if value}
    optional_warm_drop_order = ("resume_hint", "what_changed", "persona_notes", "user_model", "environment_summary")
    while _estimate_payload_tokens(pack) > hard_cap and pack["agent_state"]["warm"]:
        dropped = False
        for field in optional_warm_drop_order:
            if field in pack["agent_state"]["warm"]:
                pack["agent_state"]["warm"].pop(field, None)
                dropped = True
                break
        if not dropped:
            break
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
            "mind_context_mini": (
                agent_state.get("mind_context_mini") if isinstance(agent_state.get("mind_context_mini"), dict) else {}
            ),
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
        entry = {
            "id": str(item.get("id") or ""),
            "ts": str(item.get("ts") or ""),
            "by": str(item.get("by") or ""),
            "kind": str(item.get("kind") or ""),
            "signal_family": _bootstrap_signal_family(item=item, data=data),
            "reply_required": bool(data.get("reply_required") is True or data.get("requires_ack") is True),
            "text_preview": _trim_text(text_source, max_chars=220),
        }
        notify_kind = str(data.get("kind") or "").strip()
        if notify_kind:
            entry["notify_kind"] = notify_kind
        preview.append(entry)
    return {
        "messages": preview,
        "truncated": has_more,
    }


def _append_runtime_help_addenda(markdown: str, *, group_id: str, actor_id: str) -> str:
    base = _strip_reserved_runtime_help_sections(markdown)
    if not base.strip():
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
    active = state.get("active_capsule_skills") if isinstance(state, dict) else []
    autoload = state.get("autoload_skills") if isinstance(state, dict) else []
    enabled_list = enabled_caps if isinstance(enabled_caps, list) else []
    active_list = active if isinstance(active, list) else []
    autoload_list = autoload if isinstance(autoload, list) else []
    sections: List[str] = []
    enabled_pack_ids = {
        str(item).strip()
        for item in enabled_list
        if str(item).strip().startswith("pack:")
    }

    try:
        space_state = get_group_space_prompt_state(gid, provider="notebooklm")
    except Exception:
        space_state = {}
    if isinstance(space_state, dict):
        provider = str(space_state.get("provider") or "notebooklm")
        mode = str(space_state.get("mode") or "disabled")
        work_bound = bool(space_state.get("work_bound"))
        memory_bound = bool(space_state.get("memory_bound"))
        if work_bound or memory_bound:
            lines_space: List[str] = [
                "## Group Space (Runtime)",
                f"- NotebookLM provider: {provider} ({mode}); work_bound={str(work_bound).lower()} memory_bound={str(memory_bound).lower()}.",
            ]
            if "pack:space" not in enabled_pack_ids:
                lines_space.append(
                    '- If `cccc_space` is hidden in this session, use `cccc_capability_use(tool_name="cccc_space", tool_arguments={"action":"status"})` to expose it.'
                )
            if work_bound:
                lines_space.append(
                    '- Use `cccc_space(action="query", lane="work")` for shared/project knowledge lookup.'
                )
                lines_space.append(
                    '- For long artifact jobs that return `accepted=true` with `status="pending"` or `status="queued"`, do not poll. Wait for the later `system.notify`, continue other work or standby, and use a one-shot reminder only if the result blocks delivery and nothing else can proceed.'
                )
            if memory_bound:
                lines_space.append(
                    '- Keep local memory first; use `cccc_space(action="query", lane="memory")` only as a deeper recall fallback.'
                )
            if mode != "active":
                lines_space.append(
                    "- If the provider is degraded, continue with Context + local memory and report the fallback explicitly."
                )
            sections.append("\n".join(lines_space).rstrip())

    if active_list or autoload_list:
        def _skill_scope_label(item: Dict[str, Any]) -> str:
            sources = item.get("activation_sources") if isinstance(item.get("activation_sources"), list) else []
            parts: List[str] = []
            for source in sources:
                if not isinstance(source, dict):
                    continue
                scope = str(source.get("scope") or "").strip()
                if not scope:
                    continue
                if scope == "session":
                    ttl = source.get("ttl_seconds")
                    try:
                        ttl_i = int(ttl)
                    except (TypeError, ValueError):
                        ttl_i = -1
                    if ttl_i >= 0:
                        parts.append(f"session ttl={ttl_i}s")
                    else:
                        parts.append("session")
                else:
                    parts.append(scope)
            return ", ".join(parts)

        def _append_skill_preview(lines_ref: List[str], item: Dict[str, Any]) -> None:
            preview = str(item.get("capsule_preview") or "").strip()
            if not preview:
                return
            preview_lines = [str(raw).strip() for raw in preview.splitlines() if str(raw).strip()]
            if not preview_lines:
                return
            lines_ref.append("    working_rules:")
            for raw in preview_lines[:4]:
                lines_ref.append(f"      - {_trim_text(raw, max_chars=140)}")

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
                scope_label = _skill_scope_label(item)
                if scope_label:
                    line += f" [scope: {scope_label}]"
                if desc:
                    line += f": {desc[:120]}"
                lines.append(line)
                _append_skill_preview(lines, item)
        lines.extend(
            [
                "- Capsule skill is runtime capsule activation, not a full local skill-package install.",
                "- Runtime success is mainly visible via `capability_state.active_capsule_skills`; `dynamic_tools` may stay unchanged.",
                "- If you need full local skill scripts or assets, install a normal skill package into Codex's skills directory (`$CODEX_HOME/skills` if `CODEX_HOME` is explicitly set).",
            ]
        )
        sections.append("\n".join(lines).rstrip())

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
        group_id=group_id,
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
        "context_hygiene": (
            recovery_pack.get("context_hygiene") if isinstance(recovery_pack.get("context_hygiene"), dict) else {}
        ),
        "memory_recall_gate": memory_recall_gate,
        "next_calls": {
            "help": "cccc_help()",
            "project_info": "cccc_project_info()",
            "context_get": "cccc_context_get()",
            "inbox_list": 'cccc_inbox_list(kind_filter="all")',
            "memory_search": 'cccc_memory(action="search", query=...)',
            "interrupt_triage": (
                'If inbox_preview messages have signal_family="interrupt", treat them as coordination interrupts: '
                'refresh or reply, then resume the current task unless priority changed.'
            ),
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
