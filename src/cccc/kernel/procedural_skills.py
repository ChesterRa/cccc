from __future__ import annotations

import hashlib
import re
from pathlib import Path
from typing import Any, Dict, List, Optional

from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso
from .group import Group

_SKILL_SCHEMA = 1
_USAGE_SCHEMA = 1
_PATCH_CANDIDATE_SCHEMA = 1
_TOKEN_RE = re.compile(r"[\w\u4e00-\u9fff]+", re.UNICODE)
_PATCH_CANDIDATE_SCORE_THRESHOLD = 0.6
_POST_MERGE_OBSERVATION_WINDOW_SECONDS = 24 * 60 * 60
_MANUAL_SKILL_ID_RE = re.compile(r"[^a-z0-9_]+")


def patch_candidate_score_threshold() -> float:
    return _PATCH_CANDIDATE_SCORE_THRESHOLD


def _procedural_skills_dir(group: Group) -> Path:
    return group.path / "state" / "procedural_skills"


def _skill_path(group: Group, skill_id: str) -> Path:
    safe_skill_id = str(skill_id or "").strip()
    return _procedural_skills_dir(group) / f"{safe_skill_id}.json"


def _usage_events_path(group: Group) -> Path:
    return group.path / "state" / "procedural_skill_usage.json"


def _patch_candidates_path(group: Group) -> Path:
    return group.path / "state" / "procedural_skill_patch_candidates.json"


def _string_list(raw: Any) -> List[str]:
    if not isinstance(raw, list):
        return []
    out: List[str] = []
    for item in raw:
        text = str(item or "").strip()
        if text:
            out.append(text)
    return out


def _trim_text(text: Any, *, max_chars: int) -> str:
    value = str(text or "").strip()
    if len(value) <= max_chars:
        return value
    return value[: max_chars - 1].rstrip() + "…"


def _normalize_skill_id(value: Any) -> str:
    raw = str(value or "").strip().lower().replace("-", "_").replace(" ", "_")
    normalized = _MANUAL_SKILL_ID_RE.sub("_", raw).strip("_")
    return normalized


def _tokenize(text: str) -> List[str]:
    return [token.lower() for token in _TOKEN_RE.findall(str(text or ""))]


def _score_skill(skill: Dict[str, Any], *, query_tokens: List[str]) -> float:
    if not query_tokens:
        return 0.0
    haystack = " ".join(
        [
            str(skill.get("title") or ""),
            str(skill.get("goal") or ""),
            " ".join(_string_list(skill.get("steps"))),
            " ".join(_string_list(skill.get("trigger_refs"))),
        ]
    ).lower()
    if not haystack.strip():
        return 0.0
    matches = sum(1 for token in query_tokens if token and token in haystack)
    if matches <= 0:
        return 0.0
    return round(matches / max(len(query_tokens), 1), 4)


def _normalize_skill_record(raw: Dict[str, Any], *, file_path: str = "", query_tokens: Optional[List[str]] = None) -> Optional[Dict[str, Any]]:
    if not isinstance(raw, dict):
        return None
    skill_id = str(raw.get("skill_id") or "").strip()
    if not skill_id:
        return None
    item = dict(raw)
    if file_path:
        item["file_path"] = str(file_path)
    item["score"] = _score_skill(item, query_tokens=query_tokens or [])
    return item


def _select_skills(raw_skills: List[Dict[str, Any]], *, query: str, limit: int) -> List[Dict[str, Any]]:
    capped_limit = max(int(limit or 0), 0)
    if capped_limit <= 0:
        return []
    query_tokens = _tokenize(query)
    scored: List[Dict[str, Any]] = []
    for item in raw_skills:
        normalized = _normalize_skill_record(
            item,
            file_path=str(item.get("file_path") or ""),
            query_tokens=query_tokens,
        )
        if normalized is None:
            continue
        scored.append(normalized)
    scored.sort(
        key=lambda item: (
            float(item.get("score") or 0.0),
            str(item.get("promoted_at") or ""),
            str(item.get("updated_at") or ""),
        ),
        reverse=True,
    )
    return scored[:capped_limit]


def _unique_lines(*values: str) -> List[str]:
    out: List[str] = []
    for value in values:
        text = _trim_text(value, max_chars=220)
        if text and text not in out:
            out.append(text)
    return out


def _load_doc_list(path: Path, *, field: str) -> List[Dict[str, Any]]:
    raw = read_json(path)
    items = raw.get(field) if isinstance(raw, dict) else []
    if not isinstance(items, list):
        return []
    return [dict(item) for item in items if isinstance(item, dict)]


def _persist_doc_list(path: Path, *, schema: int, field: str, items: List[Dict[str, Any]]) -> None:
    atomic_write_json(
        path,
        {
            "schema": schema,
            "updated_at": utc_now_iso(),
            field: items,
        },
        indent=2,
    )


def _patch_candidate_id(*, skill_id: str, patch_kind: str, reason: str, proposed_delta: Dict[str, Any]) -> str:
    raw = "|".join(
        [
            str(skill_id or "").strip(),
            str(patch_kind or "").strip(),
            str(reason or "").strip(),
            str(proposed_delta or ""),
            utc_now_iso(),
        ]
    )
    digest = hashlib.sha1(raw.encode("utf-8")).hexdigest()[:10]
    return f"skillpatch_{str(skill_id or '').strip()}_{digest}"


def _stable_delta_signature(value: Dict[str, Any]) -> str:
    normalized = json_dumps_canonical(value)
    return hashlib.sha1(normalized.encode("utf-8")).hexdigest()[:16]


def json_dumps_canonical(value: Any) -> str:
    import json
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def _iso_after_seconds(base_iso: str, seconds: int) -> str:
    from datetime import datetime, timedelta, timezone

    base = str(base_iso or "").strip()
    parsed = None
    if base:
        try:
            parsed = datetime.fromisoformat(base.replace("Z", "+00:00"))
        except Exception:
            parsed = None
    if parsed is None:
        parsed = datetime.now(timezone.utc)
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=timezone.utc)
    return (parsed + timedelta(seconds=max(0, int(seconds)))).isoformat().replace("+00:00", "Z")


def build_procedural_skill_asset(candidate: Dict[str, Any], *, memory_entry: Optional[Dict[str, Any]] = None) -> Optional[Dict[str, Any]]:
    current = dict(candidate or {})
    candidate_id = str(current.get("id") or "").strip()
    status = str(current.get("status") or "").strip().lower()
    if not candidate_id or status not in {"promoted", "promoted_to_memory"}:
        return None
    title = str(current.get("title") or "").strip()
    summary = str(current.get("summary") or "").strip()
    if not title or not summary:
        return None
    detail = current.get("detail") if isinstance(current.get("detail"), dict) else {}
    promotion = current.get("promotion") if isinstance(current.get("promotion"), dict) else {}
    memory_meta = memory_entry if isinstance(memory_entry, dict) else {}
    steps = _unique_lines(
        str(current.get("recommended_action") or ""),
        str(detail.get("outcome") or ""),
        summary,
    )
    if not steps:
        return None
    updated_at = utc_now_iso()
    skill_id = f"procskill_{candidate_id}"
    return {
        "schema": _SKILL_SCHEMA,
        "skill_id": skill_id,
        "source_experience_candidate_id": candidate_id,
        "status": "active",
        "stability": "stable",
        "governance_policy": {"patch_review_mode": "auto_merge_eligible"},
        "title": title,
        "goal": _trim_text(summary, max_chars=220),
        "steps": steps[:3],
        "constraints": [],
        "failure_signals": [],
        "trigger_refs": _string_list(current.get("source_refs")),
        "memory_entry_id": str(memory_meta.get("entry_id") or "").strip(),
        "memory_file_path": str(memory_meta.get("file_path") or "").strip(),
        "promoted_at": str(promotion.get("at") or current.get("updated_at") or "").strip(),
        "created_at": str(current.get("created_at") or updated_at).strip() or updated_at,
        "updated_at": updated_at,
        "history": [],
    }


def write_procedural_skill_asset_mirror(
    group: Group,
    *,
    candidate: Dict[str, Any],
    memory_entry: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    asset = build_procedural_skill_asset(candidate, memory_entry=memory_entry)
    if asset is None:
        return {"status": "skipped", "file_path": "", "asset": {}}
    skill_id = str(asset.get("skill_id") or "").strip()
    if not skill_id:
        return {"status": "skipped", "file_path": "", "asset": {}}
    path = _skill_path(group, skill_id)
    atomic_write_json(path, asset, indent=2)
    return {
        "status": "written",
        "file_path": str(path),
        "asset": asset,
    }


def delete_procedural_skill_asset_mirror(group: Group, *, candidate_id: str) -> Dict[str, Any]:
    skill_id = f"procskill_{str(candidate_id or '').strip()}"
    if not skill_id.strip():
        return {"status": "skipped", "file_path": "", "skill_id": ""}
    path = _skill_path(group, skill_id)
    if not path.exists():
        return {"status": "skipped", "file_path": str(path), "skill_id": skill_id}
    path.unlink()
    return {"status": "deleted", "file_path": str(path), "skill_id": skill_id}


def delete_procedural_skill_asset(group: Group, *, skill_id: str) -> Dict[str, Any]:
    normalized_skill_id = _normalize_skill_id(skill_id)
    if not normalized_skill_id:
        return {"status": "skipped", "file_path": "", "skill_id": ""}
    path = _skill_path(group, normalized_skill_id)
    if not path.exists():
        return {"status": "skipped", "file_path": str(path), "skill_id": normalized_skill_id}
    path.unlink()
    return {"status": "deleted", "file_path": str(path), "skill_id": normalized_skill_id}


def list_procedural_skill_assets(group: Group, *, query: str = "", limit: int = 3) -> List[Dict[str, Any]]:
    skills_dir = _procedural_skills_dir(group)
    if not skills_dir.exists():
        return []
    raw_skills: List[Dict[str, Any]] = []
    for path in sorted(skills_dir.glob("*.json")):
        raw = read_json(path)
        if not isinstance(raw, dict):
            continue
        item = dict(raw)
        item["file_path"] = str(path)
        raw_skills.append(item)
    return _select_skills(raw_skills, query=query, limit=limit)


def select_procedural_skills_for_consumption(
    group: Group,
    *,
    query: str = "",
    limit: int = 3,
    provided_skills: Optional[List[Dict[str, Any]]] = None,
) -> List[Dict[str, Any]]:
    if isinstance(provided_skills, list):
        selected = [dict(item) for item in provided_skills if isinstance(item, dict)]
    else:
        selected = list_procedural_skill_assets(group, query=query, limit=max(limit * 4, limit))
    active_only = [
        item
        for item in selected
        if str(item.get("status") or "active").strip().lower() in {"", "active"}
    ]
    return _select_skills(active_only, query=query, limit=limit)


def load_procedural_skill_asset(group: Group, *, skill_id: str) -> Optional[Dict[str, Any]]:
    path = _skill_path(group, skill_id)
    if not path.exists():
        return None
    raw = read_json(path)
    if not isinstance(raw, dict):
        return None
    if str(raw.get("skill_id") or "").strip() != str(skill_id or "").strip():
        return None
    item = dict(raw)
    item["file_path"] = str(path)
    return item


def create_manual_procedural_skill_asset(
    group: Group,
    *,
    title: str,
    goal: str,
    steps: List[str],
    skill_id: str = "",
    constraints: Optional[List[str]] = None,
    failure_signals: Optional[List[str]] = None,
    stability: str = "stable",
    review_mode: str = "auto_merge_eligible",
    status: str = "active",
    source_experience_candidate_id: str = "",
) -> Dict[str, Any]:
    normalized_skill_id = _normalize_skill_id(skill_id)
    if not normalized_skill_id:
        normalized_skill_id = _normalize_skill_id(f"manual_{title}")
    if not normalized_skill_id:
        raise ValueError("missing skill_id")
    existing = load_procedural_skill_asset(group, skill_id=normalized_skill_id)
    if existing is not None:
        raise ValueError(f"procedural skill already exists: {normalized_skill_id}")
    now = utc_now_iso()
    asset = {
        "schema": _SKILL_SCHEMA,
        "skill_id": normalized_skill_id,
        "source_experience_candidate_id": str(source_experience_candidate_id or "").strip(),
        "status": str(status or "active").strip() or "active",
        "stability": str(stability or "stable").strip() or "stable",
        "governance_policy": {"patch_review_mode": str(review_mode or "auto_merge_eligible").strip() or "auto_merge_eligible"},
        "title": _trim_text(title, max_chars=160),
        "goal": _trim_text(goal, max_chars=220),
        "steps": _string_list(steps),
        "constraints": _string_list(constraints or []),
        "failure_signals": _string_list(failure_signals or []),
        "trigger_refs": [],
        "memory_entry_id": "",
        "memory_file_path": "",
        "promoted_at": now,
        "created_at": now,
        "updated_at": now,
        "history": [
            {
                "action": "manual_created",
                "at": now,
            }
        ],
    }
    if not str(asset.get("title") or "").strip():
        raise ValueError("missing title")
    if not str(asset.get("goal") or "").strip():
        raise ValueError("missing goal")
    if not _string_list(asset.get("steps")):
        raise ValueError("missing steps")
    path = _skill_path(group, normalized_skill_id)
    atomic_write_json(path, asset, indent=2)
    return load_procedural_skill_asset(group, skill_id=normalized_skill_id) or asset


def update_procedural_skill_asset(
    group: Group,
    *,
    skill_id: str,
    title: Optional[str] = None,
    goal: Optional[str] = None,
    steps: Optional[List[str]] = None,
    constraints: Optional[List[str]] = None,
    failure_signals: Optional[List[str]] = None,
    stability: Optional[str] = None,
    review_mode: Optional[str] = None,
    status: Optional[str] = None,
) -> Dict[str, Any]:
    existing = load_procedural_skill_asset(group, skill_id=skill_id)
    if existing is None:
        raise ValueError(f"procedural skill not found: {skill_id}")
    updated = dict(existing)
    if title is not None:
        updated["title"] = _trim_text(title, max_chars=160)
    if goal is not None:
        updated["goal"] = _trim_text(goal, max_chars=220)
    if steps is not None:
        updated["steps"] = _string_list(steps)
    if constraints is not None:
        updated["constraints"] = _string_list(constraints)
    if failure_signals is not None:
        updated["failure_signals"] = _string_list(failure_signals)
    if stability is not None:
        updated["stability"] = str(stability or "").strip() or "stable"
    if status is not None:
        updated["status"] = str(status or "").strip() or "active"
    if review_mode is not None:
        updated["governance_policy"] = {
            "patch_review_mode": str(review_mode or "").strip() or "auto_merge_eligible"
        }
    if not str(updated.get("title") or "").strip():
        raise ValueError("missing title")
    if not str(updated.get("goal") or "").strip():
        raise ValueError("missing goal")
    if not _string_list(updated.get("steps")):
        raise ValueError("missing steps")
    history = updated.get("history") if isinstance(updated.get("history"), list) else []
    history.append(
        {
            "action": "manual_updated",
            "at": utc_now_iso(),
        }
    )
    updated["history"] = history[-50:]
    updated["updated_at"] = utc_now_iso()
    path = _skill_path(group, skill_id)
    atomic_write_json(path, updated, indent=2)
    return load_procedural_skill_asset(group, skill_id=skill_id) or updated


def load_skill_usage_events(group: Group) -> List[Dict[str, Any]]:
    return _load_doc_list(_usage_events_path(group), field="events")


def append_skill_usage_evidence(
    group: Group,
    *,
    skill_id: str,
    actor_id: str,
    turn_id: str,
    evidence_type: str,
    evidence_payload: Optional[Dict[str, Any]] = None,
    outcome: str = "",
) -> Dict[str, Any]:
    skill = load_procedural_skill_asset(group, skill_id=skill_id)
    if skill is None:
        raise ValueError(f"procedural skill not found: {skill_id}")
    events = load_skill_usage_events(group)
    event_id = f"skillu_{hashlib.sha1('|'.join([skill_id, actor_id, turn_id, evidence_type, utc_now_iso()]).encode('utf-8')).hexdigest()[:12]}"
    score = score_skill_usage_evidence(
        skill=skill,
        events=events,
        evidence_type=evidence_type,
        evidence_payload=evidence_payload,
        outcome=outcome,
    )
    event = {
        "event_id": event_id,
        "skill_id": skill_id,
        "source_experience_candidate_id": str(skill.get("source_experience_candidate_id") or "").strip(),
        "actor_id": str(actor_id or "").strip(),
        "turn_id": str(turn_id or "").strip(),
        "evidence_type": str(evidence_type or "").strip(),
        "evidence_payload": dict(evidence_payload or {}),
        "outcome": str(outcome or "").strip(),
        "score": score,
        "captured_at": utc_now_iso(),
    }
    events.append(event)
    _persist_doc_list(_usage_events_path(group), schema=_USAGE_SCHEMA, field="events", items=events[-500:])
    return event


def load_skill_patch_candidates(group: Group) -> List[Dict[str, Any]]:
    return _load_doc_list(_patch_candidates_path(group), field="candidates")


def persist_skill_patch_candidates(group: Group, candidates: List[Dict[str, Any]]) -> None:
    _persist_doc_list(_patch_candidates_path(group), schema=_PATCH_CANDIDATE_SCHEMA, field="candidates", items=candidates[-500:])


def create_skill_patch_candidate(
    group: Group,
    *,
    skill_id: str,
    actor_id: str,
    evidence: Dict[str, Any],
    patch_kind: str,
    reason: str,
    proposed_delta: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    skill = load_procedural_skill_asset(group, skill_id=skill_id)
    if skill is None:
        raise ValueError(f"procedural skill not found: {skill_id}")
    evidence_score = float(evidence.get("score") or 0.0)
    if evidence_score < _PATCH_CANDIDATE_SCORE_THRESHOLD:
        raise ValueError(
            f"usage evidence score {evidence_score:.2f} below patch threshold {_PATCH_CANDIDATE_SCORE_THRESHOLD:.2f}"
        )
    candidates = load_skill_patch_candidates(group)
    delta = dict(proposed_delta or {})
    delta_signature = _stable_delta_signature(delta)
    for existing in candidates:
        if not isinstance(existing, dict):
            continue
        if str(existing.get("skill_id") or "").strip() != skill_id:
            continue
        if str(existing.get("patch_kind") or "").strip() != str(patch_kind or "").strip():
            continue
        if str(existing.get("status") or "").strip() not in {"pending", "approved"}:
            continue
        if str(existing.get("delta_signature") or "").strip() != delta_signature:
            continue
        refs = _string_list(existing.get("evidence_refs"))
        refs = _append_unique_text(refs, str(evidence.get("event_id") or "").strip())
        existing["evidence_refs"] = refs
        existing["updated_at"] = utc_now_iso()
        candidates = [
            dict(existing) if str(item.get("candidate_id") or "").strip() == str(existing.get("candidate_id") or "").strip() else item
            for item in candidates
        ]
        persist_skill_patch_candidates(group, candidates)
        return dict(existing)
    candidate = {
        "candidate_id": _patch_candidate_id(
            skill_id=skill_id,
            patch_kind=patch_kind,
            reason=reason,
            proposed_delta=delta,
        ),
        "skill_id": skill_id,
        "source_experience_candidate_id": str(skill.get("source_experience_candidate_id") or "").strip(),
        "status": "pending",
        "patch_kind": str(patch_kind or "").strip(),
        "reason": str(reason or "").strip(),
        "proposed_delta": delta,
        "delta_signature": delta_signature,
        "score": evidence_score,
        "evidence_refs": [str(evidence.get("event_id") or "").strip()],
        "created_by": str(actor_id or "").strip(),
        "created_at": utc_now_iso(),
        "updated_at": utc_now_iso(),
        "review_mode": str(
            (
                skill.get("governance_policy")
                if isinstance(skill.get("governance_policy"), dict)
                else {}
            ).get("patch_review_mode")
            or "auto_merge_eligible"
        ).strip()
        or "auto_merge_eligible",
        "lineage": {},
        "governance": {},
    }
    candidates.append(candidate)
    _persist_doc_list(_patch_candidates_path(group), schema=_PATCH_CANDIDATE_SCHEMA, field="candidates", items=candidates[-500:])
    return candidate


def _append_unique_text(values: List[str], text: str) -> List[str]:
    candidate = str(text or "").strip()
    if not candidate:
        return list(values)
    out = [str(item or "").strip() for item in values if str(item or "").strip()]
    if candidate not in out:
        out.append(candidate)
    return out


def apply_skill_patch_candidate(
    group: Group,
    *,
    candidate: Dict[str, Any],
    actor_id: str,
) -> Dict[str, Any]:
    skill_id = str(candidate.get("skill_id") or "").strip()
    skill = load_procedural_skill_asset(group, skill_id=skill_id)
    if skill is None:
        raise ValueError(f"procedural skill not found: {skill_id}")
    updated = dict(skill)
    patch_kind = str(candidate.get("patch_kind") or "").strip()
    delta = candidate.get("proposed_delta") if isinstance(candidate.get("proposed_delta"), dict) else {}
    if patch_kind == "add_step":
        updated["steps"] = _append_unique_text(_string_list(updated.get("steps")), str(delta.get("step") or ""))
    elif patch_kind == "remove_step":
        target = str(delta.get("step") or "").strip()
        updated["steps"] = [item for item in _string_list(updated.get("steps")) if item != target]
    elif patch_kind == "adjust_constraint":
        updated["constraints"] = _append_unique_text(_string_list(updated.get("constraints")), str(delta.get("constraint") or ""))
    elif patch_kind == "clarify_failure_signal":
        updated["failure_signals"] = _append_unique_text(
            _string_list(updated.get("failure_signals")),
            str(delta.get("failure_signal") or ""),
        )
    else:
        raise ValueError(f"unsupported patch_kind: {patch_kind}")

    history = updated.get("history") if isinstance(updated.get("history"), list) else []
    merged_at = utc_now_iso()
    history.append(
        {
            "action": "patch_merged",
            "candidate_id": str(candidate.get("candidate_id") or "").strip(),
            "by": str(actor_id or "").strip(),
            "at": merged_at,
            "patch_kind": patch_kind,
        }
    )
    updated["history"] = history[-50:]
    updated["updated_at"] = merged_at
    updated["stability"] = "probation"
    updated["governance_policy"] = {"patch_review_mode": "auto_merge_eligible"}
    updated["post_merge_evaluation"] = {
        "status": "observing",
        "candidate_id": str(candidate.get("candidate_id") or "").strip(),
        "opened_at": merged_at,
        "observe_until": _iso_after_seconds(merged_at, _POST_MERGE_OBSERVATION_WINDOW_SECONDS),
    }
    path = _skill_path(group, skill_id)
    atomic_write_json(path, updated, indent=2)
    return {"status": "merged", "file_path": str(path), "asset": updated}


def evaluate_post_merge_skill_observation(
    group: Group,
    *,
    skill_id: str,
    evidence: Dict[str, Any],
    patch_candidate: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    skill = load_procedural_skill_asset(group, skill_id=skill_id)
    if skill is None:
        raise ValueError(f"procedural skill not found: {skill_id}")
    evaluation = skill.get("post_merge_evaluation") if isinstance(skill.get("post_merge_evaluation"), dict) else {}
    if str(evaluation.get("status") or "").strip() != "observing":
        return {"status": "skipped", "reason": "not_observing", "asset": skill}

    outcome = str(evidence.get("outcome") or "").strip().lower()
    score = float(evidence.get("score") or 0.0)
    followup_candidate_id = str((patch_candidate or {}).get("candidate_id") or "").strip()
    if outcome in {"ok", "passed", "success", "resolved"} and score < _PATCH_CANDIDATE_SCORE_THRESHOLD:
        next_status = "validated"
    elif outcome in {"failed", "error", "regressed"} and score >= _PATCH_CANDIDATE_SCORE_THRESHOLD:
        next_status = "regressed"
    else:
        next_status = "needs_followup"

    observed_at = utc_now_iso()
    updated = dict(skill)
    if next_status == "validated":
        updated["stability"] = "stable"
        updated["governance_policy"] = {"patch_review_mode": "auto_merge_eligible"}
    elif next_status == "regressed":
        updated["stability"] = "unstable"
        updated["governance_policy"] = {"patch_review_mode": "manual_review_required"}
    else:
        updated["stability"] = str(updated.get("stability") or "probation").strip() or "probation"
    updated["post_merge_evaluation"] = {
        "status": next_status,
        "candidate_id": str(evaluation.get("candidate_id") or "").strip(),
        "opened_at": str(evaluation.get("opened_at") or "").strip(),
        "observe_until": str(evaluation.get("observe_until") or "").strip(),
        "observed_at": observed_at,
        "evidence_event_id": str(evidence.get("event_id") or "").strip(),
        "followup_candidate_id": followup_candidate_id,
    }
    history = updated.get("history") if isinstance(updated.get("history"), list) else []
    history.append(
        {
            "action": "post_merge_evaluation_updated",
            "status": next_status,
            "evidence_event_id": str(evidence.get("event_id") or "").strip(),
            "followup_candidate_id": followup_candidate_id,
            "at": observed_at,
        }
    )
    updated["history"] = history[-50:]
    updated["updated_at"] = observed_at
    path = _skill_path(group, skill_id)
    atomic_write_json(path, updated, indent=2)
    return {"status": next_status, "file_path": str(path), "asset": updated}


def mark_patch_candidate_as_regressed_followup(
    group: Group,
    *,
    candidate_id: str,
    parent_candidate_id: str,
) -> Dict[str, Any]:
    candidates = load_skill_patch_candidates(group)
    target: Optional[Dict[str, Any]] = None
    for idx, item in enumerate(candidates):
        if str(item.get("candidate_id") or "").strip() != str(candidate_id or "").strip():
            continue
        updated = dict(item)
        updated["review_mode"] = "manual_review_required"
        lineage = updated.get("lineage") if isinstance(updated.get("lineage"), dict) else {}
        lineage["regressed_from_candidate_id"] = str(parent_candidate_id or "").strip()
        updated["lineage"] = lineage
        updated["updated_at"] = utc_now_iso()
        candidates[idx] = updated
        target = updated
        break
    if target is None:
        raise ValueError(f"skill patch candidate not found: {candidate_id}")
    persist_skill_patch_candidates(group, candidates)
    return target


def score_skill_usage_evidence(
    *,
    skill: Dict[str, Any],
    events: List[Dict[str, Any]],
    evidence_type: str,
    evidence_payload: Optional[Dict[str, Any]] = None,
    outcome: str = "",
) -> float:
    base_by_type = {
        "missing_constraint": 0.72,
        "tool_mismatch": 0.68,
        "missing_step": 0.74,
        "order_error": 0.7,
        "failure_signal_triggered": 0.76,
    }
    score = float(base_by_type.get(str(evidence_type or "").strip(), 0.45))
    if str(outcome or "").strip().lower() in {"failed", "error", "regressed"}:
        score += 0.1
    payload = evidence_payload if isinstance(evidence_payload, dict) else {}
    if payload:
        score += 0.06
    skill_id = str(skill.get("skill_id") or "").strip()
    same_type_count = sum(
        1
        for item in events
        if isinstance(item, dict)
        and str(item.get("skill_id") or "").strip() == skill_id
        and str(item.get("evidence_type") or "").strip() == str(evidence_type or "").strip()
    )
    if same_type_count > 0:
        score += min(0.12, same_type_count * 0.04)
    return round(min(score, 0.99), 4)
