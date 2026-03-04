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
            name = _trim_text(raw.get("name"), max_chars=max_chars - 28)
            label = f"{tid}: {name}".strip(": ").strip()
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
        row: Dict[str, Any] = {"id": aid}
        active_task_id = _trim_text(raw.get("active_task_id"), max_chars=24)
        if active_task_id:
            row["active_task_id"] = active_task_id
        focus = _trim_text(raw.get("focus"), max_chars=120)
        if focus:
            row["focus"] = focus
        next_action = _trim_text(raw.get("next_action"), max_chars=120)
        if next_action:
            row["next_action"] = next_action
        blockers_raw = raw.get("blockers")
        if isinstance(blockers_raw, list):
            blockers = [_trim_text(x, max_chars=80) for x in blockers_raw if str(x or "").strip()]
            blockers = [x for x in blockers if x][:3]
            if blockers:
                row["blockers"] = blockers
        if len(row) > 1:
            out.append(row)
    return out


def _fit_signal_pack_budget(signal_pack: Dict[str, Any], *, token_budget: int) -> tuple[Dict[str, Any], Dict[str, Any]]:
    """Enforce deterministic truncation order for signal pack."""
    pack = dict(signal_pack or {})
    truncated = False
    budget = max(64, int(token_budget))

    def _tokens() -> int:
        return _estimate_tokens(pack)

    # Deterministic drop order: lower-priority arrays first.
    for path in (
        ("tasks", "done_recent"),
        ("tasks", "planned"),
        ("agents",),
        ("tasks", "active"),
    ):
        while _tokens() > budget:
            if len(path) == 1:
                arr = pack.get(path[0])
            else:
                arr = (pack.get(path[0]) or {}).get(path[1]) if isinstance(pack.get(path[0]), dict) else None
            if not isinstance(arr, list) or not arr:
                break
            arr.pop()
            truncated = True

    # Then trim verbose text fields.
    trim_ops = [
        ("vision", 160),
        ("overview_manual.current_focus", 120),
        ("overview_manual.collaboration_mode", 80),
    ]
    for key, cap in trim_ops:
        if _tokens() <= budget:
            break
        if key == "vision" and isinstance(pack.get("vision"), str) and len(str(pack.get("vision") or "")) > cap:
            pack["vision"] = _trim_text(pack.get("vision"), max_chars=cap)
            truncated = True
        if key == "overview_manual.current_focus":
            om = pack.get("overview_manual")
            if isinstance(om, dict) and len(str(om.get("current_focus") or "")) > cap:
                om["current_focus"] = _trim_text(om.get("current_focus"), max_chars=cap)
                truncated = True
        if key == "overview_manual.collaboration_mode":
            om = pack.get("overview_manual")
            if isinstance(om, dict) and len(str(om.get("collaboration_mode") or "")) > cap:
                om["collaboration_mode"] = _trim_text(om.get("collaboration_mode"), max_chars=cap)
                truncated = True

    # Final hard fallback: keep only high-priority keys.
    if _tokens() > budget:
        compact_pack = {
            "schema": pack.get("schema") or _SIGNAL_PACK_SCHEMA_VERSION,
            "vision": _trim_text(pack.get("vision"), max_chars=100),
            "overview_manual": {
                "current_focus": _trim_text((pack.get("overview_manual") or {}).get("current_focus"), max_chars=80)
                if isinstance(pack.get("overview_manual"), dict)
                else "",
            },
            "tasks": {"active": ((pack.get("tasks") or {}).get("active") or [])[:2]}
            if isinstance(pack.get("tasks"), dict)
            else {"active": []},
            "agents": (pack.get("agents") or [])[:2] if isinstance(pack.get("agents"), list) else [],
        }
        pack = compact_pack
        truncated = True
        while _tokens() > budget and isinstance(pack.get("agents"), list) and pack.get("agents"):
            cast_agents = pack.get("agents")
            if isinstance(cast_agents, list):
                cast_agents.pop()
        while _tokens() > budget and isinstance((pack.get("tasks") or {}).get("active"), list) and (pack.get("tasks") or {}).get("active"):
            task_obj = pack.get("tasks")
            if isinstance(task_obj, dict) and isinstance(task_obj.get("active"), list):
                task_obj["active"].pop()
        if _tokens() > budget and isinstance(pack.get("overview_manual"), dict):
            pack["overview_manual"] = {"current_focus": ""}
        if _tokens() > budget:
            pack["vision"] = _trim_text(pack.get("vision"), max_chars=48)
        if _tokens() > budget:
            pack = {"schema": pack.get("schema") or _SIGNAL_PACK_SCHEMA_VERSION}

    meta = {
        "schema": str(pack.get("schema") or _SIGNAL_PACK_SCHEMA_VERSION),
        "token_budget": budget,
        "token_estimate": _tokens(),
        "truncated": bool(truncated),
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

    overview_raw = raw.get("overview_manual") if isinstance(raw.get("overview_manual"), dict) else {}
    tasks_raw = raw.get("tasks") if isinstance(raw.get("tasks"), dict) else {}
    normalized: Dict[str, Any] = {
        "schema": _SIGNAL_PACK_SCHEMA_VERSION,
        "vision": _trim_text(raw.get("vision"), max_chars=280),
        "overview_manual": {
            "current_focus": _trim_text(overview_raw.get("current_focus"), max_chars=180),
            "collaboration_mode": _trim_text(overview_raw.get("collaboration_mode"), max_chars=120),
            "roles": [
                _trim_text(x, max_chars=32)
                for x in (overview_raw.get("roles") if isinstance(overview_raw.get("roles"), list) else [])
                if str(x or "").strip()
            ][:6],
        },
        "tasks": {
            "active": _sanitize_task_items(tasks_raw.get("active"), max_items=8, max_chars=96),
            "planned": _sanitize_task_items(tasks_raw.get("planned"), max_items=8, max_chars=96),
            "done_recent": _sanitize_task_items(tasks_raw.get("done_recent"), max_items=6, max_chars=96),
        },
        "agents": _sanitize_agents(raw.get("agents"), max_items=8),
    }

    has_payload = bool(
        normalized.get("vision")
        or normalized["overview_manual"].get("current_focus")
        or normalized["overview_manual"].get("collaboration_mode")
        or normalized["overview_manual"].get("roles")
        or normalized["tasks"].get("active")
        or normalized["tasks"].get("planned")
        or normalized["tasks"].get("done_recent")
        or normalized.get("agents")
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
        name = _trim_text(getattr(task, "name", ""), max_chars=64)
        return f"{tid}: {name}".strip(": ").strip()

    active_tasks = [_task_line(t) for t in tasks if getattr(t, "status", TaskStatus.PLANNED) == TaskStatus.ACTIVE]
    planned_tasks = [_task_line(t) for t in tasks if getattr(t, "status", TaskStatus.PLANNED) == TaskStatus.PLANNED]
    done_tasks = [t for t in tasks if getattr(t, "status", TaskStatus.PLANNED) == TaskStatus.DONE]
    done_tasks.sort(key=lambda t: str(getattr(t, "updated_at", "") or ""), reverse=True)
    done_recent = [_task_line(t) for t in done_tasks[:6]]

    agent_rows: List[Dict[str, Any]] = []
    for a in sorted(agents_state.agents, key=lambda x: str(getattr(x, "id", ""))):
        row = {
            "id": str(getattr(a, "id", "") or ""),
            "active_task_id": str(getattr(a, "active_task_id", "") or ""),
            "focus": str(getattr(a, "focus", "") or ""),
            "next_action": str(getattr(a, "next_action", "") or ""),
            "blockers": list(getattr(a, "blockers", []) or []),
        }
        if any(str(v).strip() for k, v in row.items() if k != "id") or row.get("blockers"):
            agent_rows.append(row)

    base = {
        "schema": _SIGNAL_PACK_SCHEMA_VERSION,
        "vision": str(context.vision or ""),
        "overview_manual": {
            "current_focus": str(context.overview.manual.current_focus or "") if context.overview else "",
            "collaboration_mode": str(context.overview.manual.collaboration_mode or "") if context.overview else "",
            "roles": list(context.overview.manual.roles or []) if context.overview else [],
        },
        "tasks": {
            "active": active_tasks,
            "planned": planned_tasks,
            "done_recent": done_recent,
        },
        "agents": agent_rows,
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
