from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, List, Optional

from ...kernel.actors import list_actors
from ...kernel.experience import load_experience_candidates
from ...kernel.experience_assets import (
    delete_experience_asset_mirror,
    select_experience_assets_for_consumption,
    write_experience_asset_mirror,
)
from ...kernel.group import load_group
from ...kernel.procedural_skills import (
    delete_procedural_skill_asset_mirror,
    select_procedural_skills_for_consumption,
    write_procedural_skill_asset_mirror,
)
from ...kernel.system_prompt import render_system_prompt
from ...util.conv import coerce_bool
from . import experience_memory_lane as memory_lane
from .experience_common import _PROMOTED_STATUSES, _RETIRED_STATUSES, _string_list, _tokenize, _trim_text


def sync_procedural_skill_mirror(
    *,
    group: Any,
    candidate: Dict[str, Any],
    memory_entry: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    status = str(candidate.get("status") or "").strip()
    candidate_id = str(candidate.get("id") or "").strip()
    if not candidate_id:
        return {"status": "skipped", "file_path": "", "skill_id": ""}
    if status == "promoted_to_memory":
        return write_procedural_skill_asset_mirror(
            group,
            candidate=candidate,
            memory_entry=memory_entry,
        )
    if status in _RETIRED_STATUSES:
        return delete_procedural_skill_asset_mirror(group, candidate_id=candidate_id)
    return {"status": "skipped", "file_path": "", "skill_id": f"procskill_{candidate_id}"}


def sync_experience_asset_mirror(
    *,
    group: Any,
    candidate: Dict[str, Any],
    memory_entry: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    status = str(candidate.get("status") or "").strip()
    candidate_id = str(candidate.get("id") or "").strip()
    if not candidate_id:
        return {"status": "skipped", "file_path": "", "asset_id": ""}
    if status == "promoted_to_memory":
        return write_experience_asset_mirror(
            group,
            candidate=candidate,
            memory_entry=memory_entry,
        )
    if status in _RETIRED_STATUSES:
        return delete_experience_asset_mirror(group, candidate_id=candidate_id)
    return {"status": "skipped", "file_path": "", "asset_id": f"expasset_{candidate_id}"}


def refresh_runtime_prompt_consumption(
    *,
    group: Any,
    candidate_id: str,
    selected_assets: Optional[List[Dict[str, Any]]] = None,
) -> Dict[str, Any]:
    from ..claude_app_sessions import SUPERVISOR as claude_app_supervisor
    from ..codex_app_sessions import SUPERVISOR as codex_app_supervisor
    from ..messaging.delivery import render_headless_control_text

    deliveries: List[Dict[str, Any]] = []
    for actor in list_actors(group):
        if not isinstance(actor, dict):
            continue
        actor_id = str(actor.get("id") or "").strip()
        if not actor_id or actor_id == "user":
            continue
        if not coerce_bool(actor.get("enabled"), default=True):
            continue
        if str(actor.get("runner") or "pty").strip().lower() != "headless":
            continue

        runtime = str(actor.get("runtime") or "").strip().lower()
        delivered = False
        reason = "unsupported_runtime"
        if runtime == "codex":
            if not codex_app_supervisor.actor_running(group.group_id, actor_id):
                reason = "not_running"
            else:
                runtime_actor = dict(actor)
                runtime_actor["experience_assets"] = [dict(item) for item in (selected_assets or []) if isinstance(item, dict)]
                runtime_actor["procedural_skills"] = select_procedural_skills_for_consumption(group, limit=3)
                prompt = render_system_prompt(group=group, actor=runtime_actor)
                control_text = render_headless_control_text(control_kind="bootstrap", body=prompt)
                delivered = bool(
                    control_text
                    and codex_app_supervisor.submit_control_message(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        text=control_text,
                        control_kind="bootstrap",
                    )
                )
                reason = "queued" if delivered else "submit_failed"
        elif runtime == "claude":
            if not claude_app_supervisor.actor_running(group.group_id, actor_id):
                reason = "not_running"
            else:
                runtime_actor = dict(actor)
                runtime_actor["experience_assets"] = [dict(item) for item in (selected_assets or []) if isinstance(item, dict)]
                runtime_actor["procedural_skills"] = select_procedural_skills_for_consumption(group, limit=3)
                prompt = render_system_prompt(group=group, actor=runtime_actor)
                control_text = render_headless_control_text(control_kind="bootstrap", body=prompt)
                delivered = bool(
                    control_text
                    and claude_app_supervisor.submit_control_message(
                        group_id=group.group_id,
                        actor_id=actor_id,
                        text=control_text,
                        control_kind="bootstrap",
                    )
                )
                reason = "queued" if delivered else "submit_failed"

        deliveries.append(
            {
                "actor_id": actor_id,
                "runtime": runtime,
                "delivered": delivered,
                "reason": reason,
                "candidate_id": candidate_id,
            }
        )
    return {
        "status": "queued" if any(bool(item.get("delivered")) for item in deliveries) else "skipped",
        "deliveries": deliveries,
    }


def _candidate_status_map(group_id: str, fallback_experience: Dict[str, Any]) -> Dict[str, str]:
    statuses: Dict[str, str] = {}
    group = load_group(group_id)
    if group is not None:
        for item in load_experience_candidates(group):
            candidate_id = str(item.get("id") or "").strip()
            if candidate_id:
                statuses[candidate_id] = str(item.get("status") or "").strip()
        return statuses
    for key in ("promoted", "candidates"):
        raw_items = fallback_experience.get(key)
        if not isinstance(raw_items, list):
            continue
        for item in raw_items:
            if not isinstance(item, dict):
                continue
            candidate_id = str(item.get("id") or "").strip()
            if candidate_id:
                statuses[candidate_id] = str(item.get("status") or "").strip()
    return statuses


def _sanitize_candidate(item: Dict[str, Any], *, score: float) -> Dict[str, Any]:
    return {
        "id": str(item.get("id") or "").strip(),
        "status": str(item.get("status") or "").strip(),
        "source_kind": str(item.get("source_kind") or "").strip(),
        "title": _trim_text(item.get("title"), max_chars=120),
        "summary": _trim_text(item.get("summary"), max_chars=220),
        "applicability": _trim_text(item.get("applicability"), max_chars=180),
        "score": round(float(score or 0.0), 4),
        "source_refs": [str(x).strip() for x in (item.get("source_refs") or []) if str(x).strip()][:5],
    }


def _score_candidate(item: Dict[str, Any], *, query_tokens: List[str]) -> float:
    explicit = item.get("score")
    if isinstance(explicit, (int, float)):
        try:
            return float(explicit)
        except Exception:
            pass
    haystack_parts = [
        item.get("title"),
        item.get("summary"),
        item.get("applicability"),
        item.get("recommended_action"),
        " ".join(str(x).strip() for x in (item.get("source_refs") or []) if str(x).strip()),
    ]
    haystack = " ".join(str(part or "") for part in haystack_parts).lower()
    if not haystack or not query_tokens:
        return 0.0
    overlap = sum(1 for token in query_tokens if token in haystack)
    if overlap <= 0:
        return 0.0
    return min(0.99, overlap / max(1, len(query_tokens)))


def _task_refs_from_context(context: Dict[str, Any]) -> List[Dict[str, Any]]:
    coordination = context.get("coordination") if isinstance(context.get("coordination"), dict) else {}
    raw_tasks = coordination.get("tasks") if isinstance(coordination.get("tasks"), list) else []
    if not raw_tasks:
        task_slice = context.get("tasks") if isinstance(context.get("tasks"), dict) else {}
        if not task_slice:
            task_slice = context.get("task_slice") if isinstance(context.get("task_slice"), dict) else {}
        for key in ("assigned_active", "attention"):
            items = task_slice.get(key)
            if isinstance(items, list) and items:
                raw_tasks = items
                break
    refs: List[Dict[str, Any]] = []
    for item in raw_tasks[:3]:
        if not isinstance(item, dict):
            continue
        refs.append(
            {
                "id": str(item.get("id") or "").strip(),
                "title": _trim_text(item.get("title"), max_chars=100),
                "status": str(item.get("status") or "").strip(),
            }
        )
    return [item for item in refs if item.get("id")]


def _decision_refs_from_context(context: Dict[str, Any]) -> List[Dict[str, Any]]:
    coordination = context.get("coordination") if isinstance(context.get("coordination"), dict) else {}
    raw_notes = coordination.get("recent_decisions") if isinstance(coordination.get("recent_decisions"), list) else []
    if not raw_notes:
        recent_notes = context.get("recent_notes") if isinstance(context.get("recent_notes"), dict) else {}
        raw_notes = recent_notes.get("decisions") if isinstance(recent_notes.get("decisions"), list) else []
    refs: List[Dict[str, Any]] = []
    for item in raw_notes[:3]:
        if not isinstance(item, dict):
            continue
        refs.append(
            {
                "summary": _trim_text(item.get("summary"), max_chars=160),
                "task_id": str(item.get("task_id") or "").strip(),
                "by": str(item.get("by") or "").strip(),
            }
        )
    return [item for item in refs if item.get("summary")]


def _compact_memory_hits(memory_hits: Any) -> List[Dict[str, Any]]:
    out: List[Dict[str, Any]] = []
    if not isinstance(memory_hits, list):
        return out
    for item in memory_hits[:3]:
        if not isinstance(item, dict):
            continue
        out.append(
            {
                "path": str(item.get("path") or "").strip(),
                "start_line": int(item.get("start_line") or 1),
                "score": float(item.get("score") or 0.0),
                "snippet": _trim_text(item.get("snippet"), max_chars=220),
            }
        )
    return [item for item in out if item.get("path")]


def _filter_memory_hits(
    *,
    group_id: str,
    fallback_experience: Dict[str, Any],
    compact_memory_hits: List[Dict[str, Any]],
) -> List[Dict[str, Any]]:
    candidate_statuses = _candidate_status_map(group_id, fallback_experience)
    group = load_group(group_id)
    memory_blocks = memory_lane.load_memory_blocks_by_start_line(group_id=group_id, group=group)
    if group is not None:
        memory_file_path = str(Path(group.path) / "state" / "memory" / "MEMORY.md")
    else:
        try:
            memory_file_path = str(memory_lane.resolve_memory_layout(group_id, ensure_files=False).memory_file)
        except Exception:
            memory_file_path = ""
    out: List[Dict[str, Any]] = []
    for item in compact_memory_hits:
        path = str(item.get("path") or "").strip()
        if not path or not memory_blocks or path != memory_file_path:
            out.append(item)
            continue
        sanitized_item = dict(item)
        block = memory_lane.memory_block_for_line(memory_blocks, int(item.get("start_line") or 0))
        if block is None:
            sanitized_item.pop("snippet", None)
            out.append(sanitized_item)
            continue
        meta = block.get("meta") if isinstance(block.get("meta"), dict) else {}
        kind = str(meta.get("kind") or block.get("kind") or "").strip()
        lifecycle_state = str(meta.get("lifecycle_state") or "").strip()
        if kind == "experience_retired" or (lifecycle_state and lifecycle_state != "active"):
            continue
        source_refs = _string_list(meta.get("source_refs"))
        candidate_refs = [ref.split(":", 1)[1] for ref in source_refs if ref.startswith("experience:")]
        if any(candidate_statuses.get(candidate_id) in _RETIRED_STATUSES for candidate_id in candidate_refs):
            continue
        body = str(block.get("body") or "").strip()
        safe_snippet = _trim_text(body, max_chars=220)
        if safe_snippet:
            sanitized_item["snippet"] = safe_snippet
        else:
            sanitized_item.pop("snippet", None)
        out.append(sanitized_item)
    return out


def query_experience_recall(
    *,
    group_id: str,
    query: str,
    context: Dict[str, Any],
    memory_hits: Any,
) -> Dict[str, Any]:
    coordination = context.get("coordination") if isinstance(context.get("coordination"), dict) else {}
    fallback_experience = coordination.get("experience") if isinstance(coordination.get("experience"), dict) else {}
    compact_memory_hits = _filter_memory_hits(
        group_id=group_id,
        fallback_experience=fallback_experience,
        compact_memory_hits=_compact_memory_hits(memory_hits),
    )
    task_refs = _task_refs_from_context(context if isinstance(context, dict) else {})
    decision_refs = _decision_refs_from_context(context if isinstance(context, dict) else {})
    promoted: List[Dict[str, Any]] = []
    candidates: List[Dict[str, Any]] = []
    top_promoted_score = 0.0
    query_tokens = _tokenize(query)

    group = load_group(group_id)
    if group is not None:
        for item in load_experience_candidates(group):
            score = _score_candidate(item, query_tokens=query_tokens)
            if score <= 0:
                continue
            sanitized = _sanitize_candidate(item, score=score)
            if sanitized["status"] in _PROMOTED_STATUSES:
                promoted.append(sanitized)
                top_promoted_score = max(top_promoted_score, float(sanitized.get("score") or 0.0))
            elif sanitized["status"] in _RETIRED_STATUSES:
                continue
            else:
                candidates.append(sanitized)
    elif isinstance(fallback_experience, dict):
        for raw in fallback_experience.get("promoted") if isinstance(fallback_experience.get("promoted"), list) else []:
            if not isinstance(raw, dict):
                continue
            score = _score_candidate(raw, query_tokens=query_tokens)
            if score <= 0:
                continue
            promoted.append(_sanitize_candidate(raw, score=score))
        for raw in fallback_experience.get("candidates") if isinstance(fallback_experience.get("candidates"), list) else []:
            if not isinstance(raw, dict):
                continue
            if str(raw.get("status") or "").strip() in _RETIRED_STATUSES:
                continue
            score = _score_candidate(raw, query_tokens=query_tokens)
            if score <= 0:
                continue
            candidates.append(_sanitize_candidate(raw, score=score))

    promoted.sort(key=lambda item: float(item.get("score") or 0.0), reverse=True)
    candidates.sort(key=lambda item: float(item.get("score") or 0.0), reverse=True)
    promoted = promoted[:3]
    candidates = candidates[:3]
    top_promoted_score = max([float(item.get("score") or 0.0) for item in promoted] + [top_promoted_score])

    return {
        "query": str(query or "").strip(),
        "task_refs": task_refs,
        "decision_refs": decision_refs,
        "memory_hits": compact_memory_hits,
        "experience": {
            "promoted": promoted,
            "candidates": candidates,
            "promoted_top_score": round(top_promoted_score, 4),
        },
        "has_any": bool(compact_memory_hits or promoted or candidates),
        "has_high_relevance_promoted": bool(top_promoted_score >= 0.75),
    }


# Backward-compatible aliases for existing tests/callers while T345 removes private cross-module calls.
_sync_procedural_skill_mirror = sync_procedural_skill_mirror
_refresh_runtime_prompt_consumption = refresh_runtime_prompt_consumption
