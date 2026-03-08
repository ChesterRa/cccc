"""
ReMe file-first memory operations for daemon.

Hard-cut semantics:
- no legacy sqlite path in runtime
- all active ops route to memory_reme_* family
"""

from __future__ import annotations

import hashlib
import json
import time
from contextlib import nullcontext
from typing import Any, Dict, List, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.context import ContextStorage, TaskStatus
from ...kernel.group import load_group
from ...kernel.ledger import read_last_lines
from ...kernel.memory_reme import (
    append_daily_entry,
    append_memory_entry,
    build_memory_entry,
    compact_messages,
    context_check_messages,
    get_file_slice,
    get_runtime,
    index_sync,
    resolve_memory_layout,
    search as reme_search,
    summarize_daily_messages,
    write_raw_content,
)
from ...util.conv import coerce_bool
from ...util.time import utc_now_iso

_SIGNAL_PACK_SCHEMA_VERSION = "v1"
_DEFAULT_SIGNAL_PACK_TOKEN_BUDGET = 320
_DEFAULT_AUTO_CONTEXT_WINDOW_TOKENS = 128000
_DEFAULT_AUTO_RESERVE_TOKENS = 36000
_DEFAULT_AUTO_KEEP_RECENT_TOKENS = 20000
_DEFAULT_AUTO_MAX_MESSAGES = 400


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _summary_digest(text: str) -> str:
    return hashlib.sha256(str(text or "").encode("utf-8")).hexdigest()


def _parse_int(value: Any, *, default: int, min_value: int, max_value: int, field: str) -> int:
    if value is None:
        return default
    try:
        num = int(value)
    except (TypeError, ValueError):
        raise ValueError(f"{field} must be integer")
    if num < min_value or num > max_value:
        raise ValueError(f"{field} must be in [{min_value}, {max_value}]")
    return num


def _parse_float(value: Any, *, default: float, min_value: float, max_value: float, field: str) -> float:
    if value is None:
        return default
    try:
        num = float(value)
    except (TypeError, ValueError):
        raise ValueError(f"{field} must be float")
    if num < min_value or num > max_value:
        raise ValueError(f"{field} must be in [{min_value}, {max_value}]")
    return num


def _estimate_tokens(value: Any) -> int:
    try:
        payload = json.dumps(value, ensure_ascii=False, sort_keys=True)
    except Exception:
        payload = str(value or "")
    return max(1, len(payload) // 4)


def _trim_text(value: Any, *, max_chars: int) -> str:
    text = str(value or "").strip()
    if len(text) <= max_chars:
        return text
    if max_chars <= 3:
        return text[:max_chars]
    return text[: max_chars - 3].rstrip() + "..."


def _sanitize_task_items(items: Any, *, max_items: int, max_chars: int) -> List[str]:
    out: List[str] = []
    if not isinstance(items, list):
        return out
    for raw in items:
        if len(out) >= max_items:
            break
        if isinstance(raw, dict):
            tid = _trim_text(raw.get("id"), max_chars=24)
            title = _trim_text(raw.get("title") or raw.get("name"), max_chars=max_chars - 28)
            label = f"{tid}: {title}".strip(": ").strip()
        else:
            label = _trim_text(raw, max_chars=max_chars)
        if label:
            out.append(label)
    return out


def _sanitize_agents(items: Any, *, max_items: int) -> List[Dict[str, Any]]:
    out: List[Dict[str, Any]] = []
    if not isinstance(items, list):
        return out
    for raw in items:
        if len(out) >= max_items:
            break
        if not isinstance(raw, dict):
            continue
        aid = _trim_text(raw.get("id"), max_chars=32)
        if not aid:
            continue
        hot = raw.get("hot") if isinstance(raw.get("hot"), dict) else {}
        warm = raw.get("warm") if isinstance(raw.get("warm"), dict) else {}
        row: Dict[str, Any] = {"id": aid}
        active_task_id = _trim_text(hot.get("active_task_id") if hot else raw.get("active_task_id"), max_chars=24)
        if active_task_id:
            row["active_task_id"] = active_task_id
        focus = _trim_text(hot.get("focus") if hot else raw.get("focus"), max_chars=120)
        if focus:
            row["focus"] = focus
        next_action = _trim_text(hot.get("next_action") if hot else raw.get("next_action"), max_chars=120)
        if next_action:
            row["next_action"] = next_action
        blockers_raw = hot.get("blockers") if isinstance(hot.get("blockers"), list) else raw.get("blockers") if isinstance(raw.get("blockers"), list) else []
        blockers = [_trim_text(x, max_chars=80) for x in blockers_raw if str(x or "").strip()][:3]
        if blockers:
            row["blockers"] = blockers
        what_changed = _trim_text(warm.get("what_changed") if warm else raw.get("what_changed"), max_chars=140)
        if what_changed:
            row["what_changed"] = what_changed
        resume_hint = _trim_text(warm.get("resume_hint") if warm else raw.get("resume_hint"), max_chars=140)
        if resume_hint:
            row["resume_hint"] = resume_hint
        environment_summary = _trim_text(warm.get("environment_summary") if warm else raw.get("environment_summary"), max_chars=120)
        if environment_summary:
            row["environment_summary"] = environment_summary
        user_model = _trim_text(warm.get("user_model") if warm else raw.get("user_model"), max_chars=120)
        if user_model:
            row["user_model"] = user_model
        persona_notes = _trim_text(warm.get("persona_notes") if warm else raw.get("persona_notes"), max_chars=120)
        if persona_notes:
            row["persona_notes"] = persona_notes
        if len(row) > 1:
            out.append(row)
    return out


def _fit_signal_pack_budget(signal_pack: Dict[str, Any], *, token_budget: int) -> tuple[Dict[str, Any], Dict[str, Any]]:
    pack = dict(signal_pack or {})
    truncated = False
    budget = max(64, int(token_budget))

    def _tokens() -> int:
        return _estimate_tokens(pack)

    def _drop_optional_agent_fields() -> bool:
        changed = False
        rows = pack.get("agent_states") if isinstance(pack.get("agent_states"), list) else []
        for field in ("persona_notes", "user_model", "environment_summary"):
            field_dropped = False
            for row in rows:
                if not isinstance(row, dict):
                    continue
                if field in row:
                    row.pop(field, None)
                    changed = True
                    field_dropped = True
                    if _tokens() <= budget:
                        return True
            if field_dropped and _tokens() <= budget:
                return True
        return changed

    for path in (
        ("tasks", "done_recent"),
        ("tasks", "planned"),
        ("tasks", "blocked"),
        ("tasks", "waiting_user"),
        ("agent_states_optional",),
        ("agent_states",),
        ("tasks", "active"),
    ):
        while _tokens() > budget:
            if path == ("agent_states_optional",):
                if not _drop_optional_agent_fields():
                    break
                truncated = True
                continue
            if len(path) == 1:
                arr = pack.get(path[0])
            else:
                arr = (pack.get(path[0]) or {}).get(path[1]) if isinstance(pack.get(path[0]), dict) else None
            if not isinstance(arr, list) or not arr:
                break
            arr.pop()
            truncated = True

    brief = pack.get("coordination_brief") if isinstance(pack.get("coordination_brief"), dict) else {}
    if _tokens() > budget and isinstance(brief.get("project_brief"), str) and len(str(brief.get("project_brief") or "")) > 180:
        brief["project_brief"] = _trim_text(brief.get("project_brief"), max_chars=180)
        truncated = True
    if _tokens() > budget and isinstance(brief.get("current_focus"), str) and len(str(brief.get("current_focus") or "")) > 120:
        brief["current_focus"] = _trim_text(brief.get("current_focus"), max_chars=120)
        truncated = True
    if _tokens() > budget and isinstance(brief.get("objective"), str) and len(str(brief.get("objective") or "")) > 120:
        brief["objective"] = _trim_text(brief.get("objective"), max_chars=120)
        truncated = True
    while _tokens() > budget and isinstance(brief.get("constraints"), list) and brief.get("constraints"):
        brief["constraints"].pop()
        truncated = True
    while _tokens() > budget and isinstance(brief.get("project_brief"), str) and brief.get("project_brief"):
        brief["project_brief"] = _trim_text(brief.get("project_brief"), max_chars=max(0, len(str(brief.get("project_brief") or "")) - 40))
        truncated = True
        if not brief["project_brief"]:
            break
    while _tokens() > budget and isinstance(brief.get("current_focus"), str) and brief.get("current_focus"):
        brief["current_focus"] = _trim_text(brief.get("current_focus"), max_chars=max(0, len(str(brief.get("current_focus") or "")) - 20))
        truncated = True
        if not brief["current_focus"]:
            break
    while _tokens() > budget and isinstance(brief.get("objective"), str) and brief.get("objective"):
        brief["objective"] = _trim_text(brief.get("objective"), max_chars=max(0, len(str(brief.get("objective") or "")) - 20))
        truncated = True
        if not brief["objective"]:
            break

    meta = {
        "schema": _SIGNAL_PACK_SCHEMA_VERSION,
        "token_budget": budget,
        "token_estimate": _tokens(),
        "truncated": truncated,
    }
    return pack, meta


def _normalize_signal_pack(signal_pack: Optional[Dict[str, Any]], *, token_budget: int) -> tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
    raw = signal_pack if isinstance(signal_pack, dict) else None
    if raw is None:
        return None, {
            "schema": _SIGNAL_PACK_SCHEMA_VERSION,
            "token_budget": max(64, int(token_budget)),
            "token_estimate": 0,
            "truncated": False,
        }

    brief_raw = raw.get("coordination_brief") if isinstance(raw.get("coordination_brief"), dict) else {}
    tasks_raw = raw.get("tasks") if isinstance(raw.get("tasks"), dict) else {}
    normalized: Dict[str, Any] = {
        "schema": _SIGNAL_PACK_SCHEMA_VERSION,
        "coordination_brief": {
            "objective": _trim_text(brief_raw.get("objective"), max_chars=220),
            "current_focus": _trim_text(brief_raw.get("current_focus"), max_chars=180),
            "constraints": [
                _trim_text(item, max_chars=64)
                for item in (brief_raw.get("constraints") if isinstance(brief_raw.get("constraints"), list) else [])
                if str(item or "").strip()
            ][:6],
            "project_brief": _trim_text(brief_raw.get("project_brief"), max_chars=280),
        },
        "tasks": {
            "active": _sanitize_task_items(tasks_raw.get("active"), max_items=8, max_chars=96),
            "planned": _sanitize_task_items(tasks_raw.get("planned"), max_items=8, max_chars=96),
            "done_recent": _sanitize_task_items(tasks_raw.get("done_recent"), max_items=6, max_chars=96),
            "blocked": _sanitize_task_items(tasks_raw.get("blocked"), max_items=6, max_chars=96),
            "waiting_user": _sanitize_task_items(tasks_raw.get("waiting_user"), max_items=4, max_chars=96),
        },
        "agent_states": _sanitize_agents(raw.get("agent_states"), max_items=8),
    }

    brief = normalized["coordination_brief"]
    tasks = normalized["tasks"]
    has_payload = bool(
        brief.get("objective")
        or brief.get("current_focus")
        or brief.get("constraints")
        or brief.get("project_brief")
        or tasks.get("active")
        or tasks.get("planned")
        or tasks.get("done_recent")
        or tasks.get("blocked")
        or tasks.get("waiting_user")
        or normalized.get("agent_states")
    )
    if not has_payload:
        return None, {
            "schema": _SIGNAL_PACK_SCHEMA_VERSION,
            "token_budget": max(64, int(token_budget)),
            "token_estimate": 0,
            "truncated": False,
        }
    return _fit_signal_pack_budget(normalized, token_budget=token_budget)


def _build_group_signal_pack(group_id: str, *, token_budget: int) -> tuple[Optional[Dict[str, Any]], Dict[str, Any]]:
    group = load_group(group_id)
    if group is None:
        return None, {
            "schema": _SIGNAL_PACK_SCHEMA_VERSION,
            "token_budget": max(64, int(token_budget)),
            "token_estimate": 0,
            "truncated": False,
        }
    storage = ContextStorage(group)
    context = storage.load_context()
    tasks = storage.list_tasks()
    agents_state = storage.load_agents()

    def _task_line(task: Any) -> str:
        tid = _trim_text(getattr(task, "id", ""), max_chars=24)
        title = _trim_text(getattr(task, "title", ""), max_chars=64)
        return f"{tid}: {title}".strip(": ").strip()

    active_task_objs = [t for t in tasks if getattr(t, "status", TaskStatus.PLANNED) == TaskStatus.ACTIVE]
    active_task_objs.sort(key=lambda t: str(getattr(t, "updated_at", "") or getattr(t, "created_at", "") or ""), reverse=True)
    active_tasks = [_task_line(t) for t in active_task_objs]
    planned_tasks = [_task_line(t) for t in tasks if getattr(t, "status", TaskStatus.PLANNED) == TaskStatus.PLANNED]
    blocked_tasks = [
        _task_line(t)
        for t in tasks
        if getattr(t, "status", TaskStatus.PLANNED) not in {TaskStatus.DONE, TaskStatus.ARCHIVED}
        and (list(getattr(t, "blocked_by", []) or []) or str(getattr(t, "waiting_on", "none")).strip().lower() in {"actor", "external"})
    ]
    waiting_user = [
        _task_line(t)
        for t in tasks
        if getattr(t, "status", TaskStatus.PLANNED) not in {TaskStatus.DONE, TaskStatus.ARCHIVED}
        and str(getattr(t, "waiting_on", "none")).strip().lower() == "user"
    ]
    done_tasks = [t for t in tasks if getattr(t, "status", TaskStatus.PLANNED) == TaskStatus.DONE]
    done_tasks.sort(key=lambda t: str(getattr(t, "updated_at", "") or ""), reverse=True)
    done_recent = [_task_line(t) for t in done_tasks[:6]]

    prioritized_actor_ids: List[str] = []
    seen_actor_ids: set[str] = set()

    def _push_actor(actor_id: Any) -> None:
        aid = str(actor_id or "").strip()
        if not aid or aid in seen_actor_ids:
            return
        seen_actor_ids.add(aid)
        prioritized_actor_ids.append(aid)

    for task in active_task_objs:
        _push_actor(getattr(task, "assignee", None))
        _push_actor(getattr(task, "handoff_to", None))
    for agent in sorted(agents_state.agents, key=lambda item: str(getattr(item, "updated_at", "") or ""), reverse=True):
        hot = getattr(agent, "hot", None)
        if getattr(hot, "active_task_id", None) or list(getattr(hot, "blockers", []) or []):
            _push_actor(getattr(agent, "id", ""))
    for agent in sorted(agents_state.agents, key=lambda item: str(getattr(item, "id", ""))):
        _push_actor(getattr(agent, "id", ""))

    agent_by_id = {str(getattr(agent, "id", "") or ""): agent for agent in agents_state.agents}
    ordered_agents = [agent_by_id[aid] for aid in prioritized_actor_ids if aid in agent_by_id]

    base = {
        "schema": _SIGNAL_PACK_SCHEMA_VERSION,
        "coordination_brief": {
            "objective": str(context.coordination.brief.objective or ""),
            "current_focus": str(context.coordination.brief.current_focus or ""),
            "constraints": list(context.coordination.brief.constraints or []),
            "project_brief": str(context.coordination.brief.project_brief or ""),
        },
        "tasks": {
            "active": active_tasks,
            "planned": planned_tasks,
            "done_recent": done_recent,
            "blocked": blocked_tasks,
            "waiting_user": waiting_user,
        },
        "agent_states": [
            {
                "id": getattr(agent, "id", ""),
                "hot": {
                    "active_task_id": getattr(getattr(agent, "hot", None), "active_task_id", None),
                    "focus": getattr(getattr(agent, "hot", None), "focus", ""),
                    "next_action": getattr(getattr(agent, "hot", None), "next_action", ""),
                    "blockers": list(getattr(getattr(agent, "hot", None), "blockers", []) or []),
                },
                "warm": {
                    "what_changed": getattr(getattr(agent, "warm", None), "what_changed", ""),
                    "resume_hint": getattr(getattr(agent, "warm", None), "resume_hint", ""),
                    "environment_summary": getattr(getattr(agent, "warm", None), "environment_summary", ""),
                    "user_model": getattr(getattr(agent, "warm", None), "user_model", ""),
                    "persona_notes": getattr(getattr(agent, "warm", None), "persona_notes", ""),
                },
            }
            for agent in ordered_agents
        ],
    }
    return _normalize_signal_pack(base, token_budget=token_budget)

def _normalize_dedup_intent(value: Any, *, default: str) -> str:
    intent = str(value or default).strip().lower()
    if intent not in {"new", "update", "supersede", "silent"}:
        return default
    return intent


def _resolve_precheck_decision(*, dedup_intent: str, candidate_count: int) -> str:
    if dedup_intent == "silent" and candidate_count > 0:
        return "silent"
    if dedup_intent in {"new", "update", "supersede"}:
        return dedup_intent
    return "new"


def _build_dedup_meta(*, dedup_intent: str, precheck: Dict[str, Any]) -> Dict[str, Any]:
    candidate_count = int(precheck.get("candidate_count") or 0)
    precheck_decision = _resolve_precheck_decision(
        dedup_intent=dedup_intent,
        candidate_count=candidate_count,
    )
    dedup_meta: Dict[str, Any] = {
        "intent": dedup_intent,
        "query": str(precheck.get("query") or ""),
        "candidate_count": candidate_count,
        "top_score": float(precheck.get("top_score") or 0.0),
        "hits": precheck.get("hits") if isinstance(precheck.get("hits"), list) else [],
        "precheck_decision": precheck_decision,
        "final_decision": precheck_decision,
        "final_reason": "accepted",
        # keep compatibility for existing consumers reading dedup.decision
        "decision": precheck_decision,
    }
    if "error" in precheck:
        dedup_meta["error"] = str(precheck.get("error") or "")
    return dedup_meta


def _finalize_dedup_meta(
    dedup_meta: Dict[str, Any],
    *,
    status: str,
    final_reason: str = "",
) -> Dict[str, Any]:
    final = dict(dedup_meta or {})
    normalized_status = str(status or "").strip().lower()
    normalized_reason = str(final_reason or "").strip().lower()
    if normalized_status == "silent":
        if not normalized_reason:
            normalized_reason = "persistence_content_hash"
        final["final_decision"] = "silent"
        final["final_reason"] = normalized_reason
    else:
        final["final_decision"] = str(final.get("precheck_decision") or "new")
        final["final_reason"] = "accepted"
    final["decision"] = final["final_decision"]
    return final


def _dedup_precheck(
    *,
    group_id: str,
    query: str,
    max_results: int = 3,
    min_score: float = 0.92,
) -> Dict[str, Any]:
    q = _trim_text(str(query or "").replace("\n", " "), max_chars=260).strip()
    if not q:
        return {"query": "", "candidate_count": 0, "top_score": 0.0, "hits": []}
    try:
        found = reme_search(
            group_id,
            query=q,
            max_results=max(1, min(int(max_results), 10)),
            min_score=float(min_score),
            sources=["memory"],
        )
    except Exception as e:
        return {"query": q, "candidate_count": 0, "top_score": 0.0, "hits": [], "error": str(e)}
    hits_raw = found.get("hits") if isinstance(found, dict) else []
    hits = hits_raw if isinstance(hits_raw, list) else []
    top_score = float((hits[0] or {}).get("score") or 0.0) if hits and isinstance(hits[0], dict) else 0.0
    compact_hits: List[Dict[str, Any]] = []
    for item in hits[:3]:
        if not isinstance(item, dict):
            continue
        compact_hits.append(
            {
                "path": str(item.get("path") or ""),
                "start_line": int(item.get("start_line") or 1),
                "score": float(item.get("score") or 0.0),
            }
        )
    return {
        "query": q,
        "candidate_count": len(compact_hits),
        "top_score": top_score,
        "hits": compact_hits,
    }


def _collect_recent_chat_messages(
    *,
    group_id: str,
    max_messages: int,
) -> List[Dict[str, Any]]:
    group = load_group(group_id)
    if group is None:
        return []
    lines = read_last_lines(group.ledger_path, min(4000, max(200, int(max_messages) * 8)))
    out: List[Dict[str, Any]] = []
    for raw in lines:
        try:
            ev = json.loads(raw)
        except Exception:
            continue
        if not isinstance(ev, dict) or str(ev.get("kind") or "") != "chat.message":
            continue
        data = ev.get("data")
        if not isinstance(data, dict):
            continue
        text = str(data.get("text") or "").strip()
        if not text:
            continue
        by = str(ev.get("by") or "").strip()
        role = "assistant"
        if by == "user":
            role = "user"
        elif by == "system":
            role = "system"
        out.append(
            {
                "role": role,
                "name": by,
                "content": _trim_text(text, max_chars=1000),
            }
        )
    if len(out) > max_messages:
        out = out[-max_messages:]
    return out


def run_auto_conversation_memory_cycle(
    *,
    group_id: str,
    actor_id: str = "system",
    max_messages: int = _DEFAULT_AUTO_MAX_MESSAGES,
    context_window_tokens: int = _DEFAULT_AUTO_CONTEXT_WINDOW_TOKENS,
    reserve_tokens: int = _DEFAULT_AUTO_RESERVE_TOKENS,
    keep_recent_tokens: int = _DEFAULT_AUTO_KEEP_RECENT_TOKENS,
    signal_pack_token_budget: int = _DEFAULT_SIGNAL_PACK_TOKEN_BUDGET,
) -> Dict[str, Any]:
    """Daemon-owned conversation lane: context_check -> daily_flush with dedup."""
    messages = _collect_recent_chat_messages(group_id=group_id, max_messages=max_messages)
    if not messages:
        return {"status": "silent", "reason": "no_chat_messages"}

    check = context_check_messages(
        messages=messages,
        context_window_tokens=context_window_tokens,
        reserve_tokens=reserve_tokens,
        keep_recent_tokens=keep_recent_tokens,
    )
    if not bool(check.get("needs_compaction")):
        return {
            "status": "silent",
            "reason": "no_context_pressure",
            "token_count": int(check.get("token_count") or 0),
            "threshold": int(check.get("threshold") or 0),
        }

    to_summarize = check.get("messages_to_summarize")
    msgs_to_summarize = [x for x in to_summarize if isinstance(x, dict)] if isinstance(to_summarize, list) else []
    if not msgs_to_summarize:
        return {
            "status": "silent",
            "reason": "empty_compaction_slice",
            "token_count": int(check.get("token_count") or 0),
            "threshold": int(check.get("threshold") or 0),
        }

    signal_pack, signal_meta = _build_group_signal_pack(group_id, token_budget=signal_pack_token_budget)
    flush_resp = handle_memory_reme_daily_flush(
        {
            "group_id": group_id,
            "messages": msgs_to_summarize,
            "actor_id": actor_id,
            "signal_pack": signal_pack,
            "signal_pack_token_budget": signal_pack_token_budget,
            "dedup_intent": "silent",
            "dedup_query": _trim_text(msgs_to_summarize[-1].get("content") if msgs_to_summarize else "", max_chars=220),
        }
    )
    if not flush_resp.ok:
        err = flush_resp.error
        return {
            "status": "failed",
            "reason": str(err.code if err else "memory_runtime_error"),
            "message": str(err.message if err else "auto flush failed"),
            "token_count": int(check.get("token_count") or 0),
            "threshold": int(check.get("threshold") or 0),
        }
    result = flush_resp.result if isinstance(flush_resp.result, dict) else {}
    return {
        "status": str(result.get("status") or "silent"),
        "reason": str(result.get("reason") or ""),
        "token_count": int(check.get("token_count") or 0),
        "threshold": int(check.get("threshold") or 0),
        "messages_considered": len(messages),
        "messages_summarized": len(msgs_to_summarize),
        "target_file": str(result.get("target_file") or ""),
        "signal_pack": signal_meta,
        "dedup": result.get("dedup") if isinstance(result.get("dedup"), dict) else {},
    }


def handle_memory_reme_layout_get(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    try:
        layout = resolve_memory_layout(group_id, ensure_files=True)
    except ValueError as e:
        return _error("group_not_found", str(e))
    return DaemonResponse(
        ok=True,
        result={
            "group_label": layout.group_label,
            "memory_root": str(layout.memory_root),
            "memory_file": str(layout.memory_file),
            "daily_dir": str(layout.daily_dir),
            "today_daily_file": str(layout.today_daily_file),
            "backend": {"name": "local", "vector_enabled": False, "fts_enabled": True},
        },
    )


def handle_memory_reme_index_sync(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    mode = str(args.get("mode") or "scan").strip().lower()
    try:
        result = index_sync(group_id, mode=mode)
    except ValueError as e:
        msg = str(e)
        if "group not found" in msg:
            return _error("group_not_found", msg)
        return _error("validation_error", msg)
    except Exception as e:
        return _error("memory_runtime_error", str(e))
    return DaemonResponse(ok=True, result=result)


def handle_memory_reme_search(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    query = str(args.get("query") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not query:
        return _error("missing_query", "missing query")
    try:
        max_results = _parse_int(args.get("max_results"), default=5, min_value=1, max_value=50, field="max_results")
        min_score = _parse_float(args.get("min_score"), default=0.1, min_value=0.0, max_value=1.0, field="min_score")
        vector_weight = None
        if "vector_weight" in args:
            vector_weight = _parse_float(
                args.get("vector_weight"),
                default=0.7,
                min_value=0.0,
                max_value=1.0,
                field="vector_weight",
            )
        candidate_multiplier = None
        if "candidate_multiplier" in args:
            candidate_multiplier = _parse_float(
                args.get("candidate_multiplier"),
                default=3.0,
                min_value=1.0,
                max_value=20.0,
                field="candidate_multiplier",
            )
        raw_sources = args.get("sources")
        sources = [str(x) for x in raw_sources] if isinstance(raw_sources, list) else ["memory"]
    except ValueError as e:
        return _error("validation_error", str(e))

    start = time.perf_counter()
    try:
        result = reme_search(
            group_id,
            query=query,
            max_results=max_results,
            min_score=min_score,
            sources=sources,
            vector_weight=vector_weight,
            candidate_multiplier=candidate_multiplier,
        )
    except ValueError as e:
        msg = str(e)
        if "group not found" in msg:
            return _error("group_not_found", msg)
        return _error("validation_error", msg)
    except Exception as e:
        return _error("memory_runtime_error", str(e))

    took_ms = int((time.perf_counter() - start) * 1000)
    hits = result.get("hits") if isinstance(result.get("hits"), list) else []
    return DaemonResponse(ok=True, result={"hits": hits, "count": len(hits), "took_ms": took_ms})


def handle_memory_reme_get(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    path = str(args.get("path") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not path:
        return _error("missing_path", "missing path")
    try:
        offset = _parse_int(args.get("offset"), default=1, min_value=1, max_value=1_000_000, field="offset")
        limit = _parse_int(args.get("limit"), default=200, min_value=1, max_value=5000, field="limit")
    except ValueError as e:
        return _error("validation_error", str(e))
    try:
        result = get_file_slice(group_id, path=path, offset=offset, limit=limit)
    except ValueError as e:
        msg = str(e)
        if "group not found" in msg:
            return _error("group_not_found", msg)
        return _error("validation_error", msg)
    except Exception as e:
        return _error("memory_runtime_error", str(e))
    return DaemonResponse(ok=True, result=result)


def handle_memory_reme_context_check(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    raw_messages = args.get("messages")
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not isinstance(raw_messages, list):
        return _error("validation_error", "messages must be an array")
    try:
        cwt = _parse_int(
            args.get("context_window_tokens"),
            default=128000,
            min_value=1024,
            max_value=2_000_000,
            field="context_window_tokens",
        )
        reserve = _parse_int(args.get("reserve_tokens"), default=36000, min_value=0, max_value=2_000_000, field="reserve_tokens")
        keep = _parse_int(
            args.get("keep_recent_tokens"),
            default=20000,
            min_value=256,
            max_value=2_000_000,
            field="keep_recent_tokens",
        )
    except ValueError as e:
        return _error("validation_error", str(e))

    result = context_check_messages(
        messages=[x for x in raw_messages if isinstance(x, dict)],
        context_window_tokens=cwt,
        reserve_tokens=reserve,
        keep_recent_tokens=keep,
    )
    return DaemonResponse(ok=True, result=result)


def handle_memory_reme_compact(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    raw_messages = args.get("messages_to_summarize")
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not isinstance(raw_messages, list):
        return _error("validation_error", "messages_to_summarize must be an array")
    turn_prefix = args.get("turn_prefix_messages")
    if turn_prefix is not None and not isinstance(turn_prefix, list):
        return _error("validation_error", "turn_prefix_messages must be an array when provided")

    result = compact_messages(
        messages_to_summarize=[x for x in raw_messages if isinstance(x, dict)],
        turn_prefix_messages=[x for x in (turn_prefix or []) if isinstance(x, dict)],
        previous_summary=str(args.get("previous_summary") or ""),
        language=str(args.get("language") or "en"),
        return_prompt=coerce_bool(args.get("return_prompt"), default=False),
    )
    return DaemonResponse(ok=True, result=result)


def handle_memory_reme_daily_flush(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    raw_messages = args.get("messages")
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not isinstance(raw_messages, list):
        return _error("validation_error", "messages must be an array")

    date = str(args.get("date") or "").strip() or None
    return_prompt = coerce_bool(args.get("return_prompt"), default=False)
    language = str(args.get("language") or "en")
    signal_pack = args.get("signal_pack") if isinstance(args.get("signal_pack"), dict) else None
    dedup_intent = _normalize_dedup_intent(args.get("dedup_intent"), default="new")
    dedup_query = str(args.get("dedup_query") or "").strip()
    try:
        signal_pack_token_budget = _parse_int(
            args.get("signal_pack_token_budget"),
            default=_DEFAULT_SIGNAL_PACK_TOKEN_BUDGET,
            min_value=64,
            max_value=4096,
            field="signal_pack_token_budget",
        )
    except ValueError as e:
        return _error("validation_error", str(e))
    signal_pack_normalized, signal_meta = _normalize_signal_pack(
        signal_pack,
        token_budget=signal_pack_token_budget,
    )

    filtered = [x for x in raw_messages if isinstance(x, dict)]
    if return_prompt:
        prompt = compact_messages(
            messages_to_summarize=filtered,
            turn_prefix_messages=[],
            previous_summary="",
            language=language,
            return_prompt=True,
        )
        return DaemonResponse(ok=True, result=prompt)

    rt = get_runtime(group_id)
    write_lock = rt.lock if rt is not None else nullcontext()
    try:
        with write_lock:
            layout = resolve_memory_layout(group_id, date=date, ensure_files=True)

            summary = summarize_daily_messages(filtered, signal_pack=signal_pack_normalized)
            if not summary:
                return DaemonResponse(
                    ok=True,
                    result={
                        "status": "silent",
                        "reason": "empty_summary",
                        "target_file": str(layout.today_daily_file),
                        "content_hash": "",
                        "bytes_written": 0,
                        "signal_pack": signal_meta,
                        "dedup": {
                            "intent": dedup_intent,
                            "query": "",
                            "candidate_count": 0,
                            "top_score": 0.0,
                            "hits": [],
                            "precheck_decision": "silent",
                            "final_decision": "silent",
                            "final_reason": "empty_summary",
                            "decision": "silent",
                        },
                    },
                )

            precheck = _dedup_precheck(group_id=group_id, query=(dedup_query or summary))
            dedup_meta = _build_dedup_meta(
                dedup_intent=dedup_intent,
                precheck=precheck,
            )
            candidate_count = int(dedup_meta.get("candidate_count") or 0)
            if str(dedup_meta.get("precheck_decision") or "") == "silent" and candidate_count > 0:
                final_dedup = _finalize_dedup_meta(
                    dedup_meta,
                    status="silent",
                    final_reason="precheck_silent",
                )
                return DaemonResponse(
                    ok=True,
                    result={
                        "status": "silent",
                        "reason": "precheck_silent",
                        "target_file": str(layout.today_daily_file),
                        "content_hash": "",
                        "bytes_written": 0,
                        "signal_pack": signal_meta,
                        "dedup": final_dedup,
                    },
                )

            created_at = utc_now_iso()
            entry = build_memory_entry(
                group_label=layout.group_label,
                kind="conversation",
                summary=summary,
                actor_id=str(args.get("actor_id") or ""),
                source_refs=[f"chat:{i}" for i in range(len(filtered))][:20],
                tags=["daily_flush"],
                created_at=created_at,
                date=(date or created_at[:10]),
            )
            flush_key = f"daily_flush:{group_id}:{entry.get('date')}:{_summary_digest(summary)}"
            write_result = append_daily_entry(group_id, entry=entry, date=date, idempotency_key=flush_key)
            index_sync(group_id, mode="scan")
    except ValueError as e:
        msg = str(e)
        if "group not found" in msg:
            return _error("group_not_found", msg)
        return _error("validation_error", msg)
    except Exception as e:
        return _error("memory_runtime_error", str(e))

    status = str(write_result.get("status") or "silent")
    persistence_reason = str(write_result.get("reason") or "")
    final_dedup = _finalize_dedup_meta(
        dedup_meta,
        status=status,
        final_reason=persistence_reason,
    )
    return DaemonResponse(
        ok=True,
        result={
            "status": status,
            "reason": str(final_dedup.get("final_reason") or "") if status == "silent" else "",
            "target_file": str(write_result.get("file_path") or layout.today_daily_file),
            "content_hash": str(write_result.get("content_hash") or ""),
            "bytes_written": int(write_result.get("bytes_written") or 0),
            "signal_pack": signal_meta,
            "dedup": final_dedup,
        },
    )


def handle_memory_reme_write(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    target = str(args.get("target") or "").strip().lower()
    content = str(args.get("content") or "")
    if target not in {"memory", "daily"}:
        return _error("validation_error", "target must be one of: memory, daily")
    if not content.strip():
        return _error("validation_error", "content is required")

    mode = str(args.get("mode") or "append").strip().lower()
    if mode not in {"append", "replace"}:
        return _error("validation_error", "mode must be one of: append, replace")
    date = str(args.get("date") or "").strip() or None
    if target == "daily" and not date:
        return _error("validation_error", "date is required when target=daily")

    idempotency_key = str(args.get("idempotency_key") or "").strip()
    dedup_intent = _normalize_dedup_intent(args.get("dedup_intent"), default="new")
    dedup_query = str(args.get("dedup_query") or "").strip()
    rt = get_runtime(group_id)
    write_lock = rt.lock if rt is not None else nullcontext()
    dedup_meta: Dict[str, Any] = {
        "intent": dedup_intent,
        "query": "",
        "candidate_count": 0,
        "top_score": 0.0,
        "hits": [],
        "precheck_decision": "new",
        "final_decision": "new",
        "final_reason": "accepted",
        "decision": "new",
    }
    try:
        with write_lock:
            precheck = _dedup_precheck(group_id=group_id, query=(dedup_query or content))
            dedup_meta = _build_dedup_meta(
                dedup_intent=dedup_intent,
                precheck=precheck,
            )
            candidate_count = int(dedup_meta.get("candidate_count") or 0)
            if str(dedup_meta.get("precheck_decision") or "") == "silent" and candidate_count > 0:
                layout = resolve_memory_layout(group_id, date=date, ensure_files=True)
                target_file = layout.memory_file if target == "memory" else layout.today_daily_file
                final_dedup = _finalize_dedup_meta(
                    dedup_meta,
                    status="silent",
                    final_reason="precheck_silent",
                )
                return DaemonResponse(
                    ok=True,
                    result={
                        "file_path": str(target_file),
                        "line_count": 0,
                        "content_hash": "",
                        "status": "silent",
                        "reason": "precheck_silent",
                        "dedup": final_dedup,
                    },
                )
            if mode == "append":
                layout = resolve_memory_layout(group_id, date=date, ensure_files=True)
                supersedes = [str(x) for x in (args.get("supersedes") or []) if isinstance(x, str)]
                if dedup_intent == "supersede" and not supersedes:
                    auto_refs = []
                    for hit in dedup_meta.get("hits") if isinstance(dedup_meta.get("hits"), list) else []:
                        if not isinstance(hit, dict):
                            continue
                        path = str(hit.get("path") or "").strip()
                        if not path:
                            continue
                        start_line = int(hit.get("start_line") or 1)
                        auto_refs.append(f"{path}#L{start_line}")
                    supersedes = auto_refs[:3]
                entry = build_memory_entry(
                    group_label=layout.group_label,
                    kind="stable_knowledge" if target == "memory" else "daily_note",
                    summary=content,
                    actor_id=str(args.get("actor_id") or ""),
                    source_refs=[str(x) for x in (args.get("source_refs") or []) if isinstance(x, str)],
                    tags=[str(x) for x in (args.get("tags") or []) if isinstance(x, str)],
                    supersedes=supersedes,
                    date=(date or utc_now_iso()[:10]),
                )
                if target == "memory":
                    write_result = append_memory_entry(group_id, entry=entry, idempotency_key=idempotency_key)
                else:
                    write_result = append_daily_entry(group_id, entry=entry, date=date, idempotency_key=idempotency_key)
            else:
                write_result = write_raw_content(
                    group_id,
                    target=target,
                    content=content,
                    mode=mode,
                    date=date,
                    idempotency_key=idempotency_key,
                )
            index_sync(group_id, mode="scan")
    except ValueError as e:
        msg = str(e)
        if "group not found" in msg:
            return _error("group_not_found", msg)
        return _error("validation_error", msg)
    except Exception as e:
        return _error("memory_runtime_error", str(e))

    status = str(write_result.get("status") or "written")
    persistence_reason = str(write_result.get("reason") or "")
    final_dedup = _finalize_dedup_meta(
        dedup_meta,
        status=status,
        final_reason=persistence_reason,
    )
    return DaemonResponse(
        ok=True,
        result={
            "file_path": str(write_result.get("file_path") or ""),
            "line_count": int(write_result.get("line_count") or 0),
            "content_hash": str(write_result.get("content_hash") or ""),
            "status": status,
            "reason": str(final_dedup.get("final_reason") or "") if status == "silent" else "",
            "dedup": final_dedup,
        },
    )


def try_handle_memory_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "memory_reme_layout_get":
        return handle_memory_reme_layout_get(args)
    if op == "memory_reme_index_sync":
        return handle_memory_reme_index_sync(args)
    if op == "memory_reme_search":
        return handle_memory_reme_search(args)
    if op == "memory_reme_get":
        return handle_memory_reme_get(args)
    if op == "memory_reme_context_check":
        return handle_memory_reme_context_check(args)
    if op == "memory_reme_compact":
        return handle_memory_reme_compact(args)
    if op == "memory_reme_daily_flush":
        return handle_memory_reme_daily_flush(args)
    if op == "memory_reme_write":
        return handle_memory_reme_write(args)
    return None
