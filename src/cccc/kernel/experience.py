from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any, Dict, List, Optional

from .context import Task, _utc_now_iso
from .experience_sources import (
    SOURCE_KIND_HEADLESS_TURN_SUCCESS,
    build_headless_turn_success_candidate,
)
from .group import Group
from ..util.fs import atomic_write_json, read_json

_EXPERIENCE_SCHEMA = 1
_MAX_CANDIDATES = 500
SOURCE_KIND_ROOT_TASK_DONE = "task.root_done"
SOURCE_KIND_COORDINATION_DECISION = "coordination.decision"
_REQUIRED_FIELDS = (
    "id",
    "source_kind",
    "source_refs",
    "title",
    "summary",
    "applicability",
    "recommended_action",
    "failure_signals",
    "status",
    "proposed_by",
    "created_at",
    "updated_at",
)


def _experience_candidates_path(group: Group) -> Path:
    return group.path / "state" / "experience_candidates.json"


def _normalize_source_refs(raw: List[str]) -> List[str]:
    refs: List[str] = []
    seen: set[str] = set()
    for item in raw:
        value = str(item or "").strip()
        if not value or value in seen:
            continue
        seen.add(value)
        refs.append(value)
    return refs


def load_experience_candidates(group: Group) -> List[Dict[str, Any]]:
    raw = read_json(_experience_candidates_path(group))
    items = raw.get("candidates")
    if not isinstance(items, list):
        return []
    out: List[Dict[str, Any]] = []
    for item in items:
        if isinstance(item, dict):
            out.append(dict(item))
    return out


def append_experience_candidate(group: Group, candidate: Dict[str, Any]) -> Dict[str, Any]:
    path = _experience_candidates_path(group)
    existing = load_experience_candidates(group)
    normalized = _normalize_candidate(candidate)
    candidate_id = str(normalized.get("id") or "").strip()
    if candidate_id:
        for item in existing:
            if str(item.get("id") or "").strip() == candidate_id:
                return item
    existing.insert(0, normalized)
    payload = {
        "schema": _EXPERIENCE_SCHEMA,
        "updated_at": _utc_now_iso(),
        "candidates": existing[:_MAX_CANDIDATES],
    }
    atomic_write_json(path, payload, indent=2)
    return normalized


def build_experience_candidate(*, source_kind: str, payload: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    normalized_source_kind = str(source_kind or "").strip()
    if normalized_source_kind == SOURCE_KIND_ROOT_TASK_DONE:
        task = payload.get("task")
        if not isinstance(task, Task):
            raise ValueError("task.root_done candidate requires task")
        return build_root_task_done_candidate(
            task=task,
            by=str(payload.get("by") or "").strip(),
            source_refs=list(payload.get("source_refs") or []) if isinstance(payload.get("source_refs"), list) else [],
        )
    if normalized_source_kind == SOURCE_KIND_COORDINATION_DECISION:
        return build_decision_candidate(
            summary=str(payload.get("summary") or "").strip(),
            by=str(payload.get("by") or "").strip(),
            task_id=str(payload.get("task_id") or "").strip() or None,
            source_refs=list(payload.get("source_refs") or []) if isinstance(payload.get("source_refs"), list) else [],
        )
    if normalized_source_kind == SOURCE_KIND_HEADLESS_TURN_SUCCESS:
        return build_headless_turn_success_candidate(
            prompt_text=str(payload.get("prompt_text") or "").strip(),
            by=str(payload.get("by") or "").strip(),
            actor_id=str(payload.get("actor_id") or "").strip(),
            runtime=str(payload.get("runtime") or "").strip(),
            turn_id=str(payload.get("turn_id") or "").strip(),
            event_id=str(payload.get("event_id") or "").strip(),
            source_refs=list(payload.get("source_refs") or []) if isinstance(payload.get("source_refs"), list) else [],
            normalize_source_refs=_normalize_source_refs,
            build_candidate_id=_build_candidate_id,
        )
    raise ValueError(f"unsupported experience source_kind: {normalized_source_kind}")


def extract_experience_candidate(
    *,
    group: Group,
    source_kind: str,
    payload: Dict[str, Any],
    dry_run: bool = False,
) -> Optional[Dict[str, Any]]:
    candidate = build_experience_candidate(source_kind=source_kind, payload=payload)
    if not isinstance(candidate, dict):
        return None
    if dry_run:
        return candidate
    return append_experience_candidate(group, candidate)


def _normalize_candidate(candidate: Dict[str, Any]) -> Dict[str, Any]:
    now = _utc_now_iso()
    normalized = {
        "id": str(candidate.get("id") or "").strip(),
        "source_kind": str(candidate.get("source_kind") or "").strip(),
        "source_refs": _normalize_source_refs(
            list(candidate.get("source_refs") or [])
            if isinstance(candidate.get("source_refs"), list)
            else []
        ),
        "title": str(candidate.get("title") or "").strip(),
        "summary": str(candidate.get("summary") or "").strip(),
        "applicability": str(candidate.get("applicability") or "").strip(),
        "recommended_action": str(candidate.get("recommended_action") or "").strip(),
        "failure_signals": [
            str(item or "").strip()
            for item in (candidate.get("failure_signals") if isinstance(candidate.get("failure_signals"), list) else [])
            if str(item or "").strip()
        ],
        "status": str(candidate.get("status") or "proposed").strip() or "proposed",
        "proposed_by": str(candidate.get("proposed_by") or "").strip(),
        "created_at": str(candidate.get("created_at") or now).strip() or now,
        "updated_at": str(candidate.get("updated_at") or now).strip() or now,
    }
    for field in _REQUIRED_FIELDS:
        normalized.setdefault(field, "" if field not in {"source_refs", "failure_signals"} else [])
    for key, value in candidate.items():
        if key not in normalized:
            normalized[key] = value
    return normalized


def _build_candidate_id(
    *,
    source_kind: str,
    task_id: str,
    summary: str,
    source_refs: List[str],
) -> str:
    basis = "|".join(
        [
            source_kind.strip().lower(),
            task_id.strip(),
            summary.strip(),
            ",".join(_normalize_source_refs(source_refs)),
        ]
    )
    digest = hashlib.sha1(basis.encode("utf-8")).hexdigest()[:20]
    return f"exp_{digest}"


def build_root_task_done_candidate(*, task: Task, by: str, source_refs: Optional[List[str]] = None) -> Dict[str, Any]:
    refs = _normalize_source_refs(source_refs or [])
    if f"task:{task.id}" not in refs:
        refs.append(f"task:{task.id}")
    task_title = str(task.title or "").strip() or str(task.id or "").strip()
    task_outcome = str(task.outcome or "").strip()
    summary = task_outcome
    title = task_title
    now = _utc_now_iso()
    return {
        "id": _build_candidate_id(
            source_kind=SOURCE_KIND_ROOT_TASK_DONE,
            task_id=task.id,
            summary=summary,
            source_refs=refs,
        ),
        "status": "proposed",
        "source_kind": SOURCE_KIND_ROOT_TASK_DONE,
        "task_id": str(task.id or "").strip(),
        "source_refs": refs,
        "title": title,
        "summary": summary,
        "applicability": "",
        "recommended_action": "",
        "failure_signals": [],
        "proposed_by": str(by or "").strip(),
        "detail": {
            "title": task_title,
            "outcome": task_outcome,
            "by": str(by or "").strip(),
            "at": str(task.updated_at or now),
        },
        "created_at": now,
        "updated_at": now,
    }


def is_explicit_decision_summary(summary: str) -> bool:
    text = str(summary or "").strip()
    # Hermes-style direction: keep generation permissive and leave
    # long-term governance to the foreman/user promotion step.
    # Only reject obviously too-short or non-informative summaries.
    if len(text) < 8:
        return False
    alpha_count = sum(1 for ch in text if ch.isalnum())
    if alpha_count < 4:
        return False
    return True


def build_decision_candidate(
    *,
    summary: str,
    by: str,
    task_id: Optional[str],
    source_refs: Optional[List[str]] = None,
) -> Optional[Dict[str, Any]]:
    normalized_summary = str(summary or "").strip()
    if not is_explicit_decision_summary(normalized_summary):
        return None
    normalized_task_id = str(task_id or "").strip()
    refs = _normalize_source_refs(source_refs or [])
    if normalized_task_id and f"task:{normalized_task_id}" not in refs:
        refs.append(f"task:{normalized_task_id}")
    now = _utc_now_iso()
    return {
        "id": _build_candidate_id(
            source_kind=SOURCE_KIND_COORDINATION_DECISION,
            task_id=normalized_task_id,
            summary=normalized_summary,
            source_refs=refs,
        ),
        "status": "proposed",
        "source_kind": SOURCE_KIND_COORDINATION_DECISION,
        "task_id": normalized_task_id,
        "source_refs": refs,
        "title": normalized_summary[:72],
        "summary": normalized_summary,
        "applicability": "",
        "recommended_action": "",
        "failure_signals": [],
        "proposed_by": str(by or "").strip(),
        "detail": {
            "by": str(by or "").strip(),
            "at": now,
        },
        "created_at": now,
        "updated_at": now,
    }

