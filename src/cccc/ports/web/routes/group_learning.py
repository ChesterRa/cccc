from __future__ import annotations

from datetime import datetime, timezone
from typing import Any, Dict, List, Optional

from fastapi import APIRouter, Depends, FastAPI
from pydantic import BaseModel, Field
from starlette.concurrency import run_in_threadpool

from ....kernel import procedural_skills
from ....kernel.group import load_group
from ..schemas import RouteContext, require_group


class ProceduralSkillUpsertRequest(BaseModel):
    skill_id: str = Field(default="")
    title: str = Field(default="")
    goal: str = Field(default="")
    steps: List[str] = Field(default_factory=list)
    constraints: List[str] = Field(default_factory=list)
    failure_signals: List[str] = Field(default_factory=list)
    stability: str = Field(default="stable")
    review_mode: str = Field(default="auto_merge_eligible")
    status: str = Field(default="active")
    source_experience_candidate_id: str = Field(default="")


def _parse_iso_utc(value: Any) -> Optional[datetime]:
    text = str(value or "").strip()
    if not text:
        return None
    try:
        if text.endswith("Z"):
            return datetime.fromisoformat(text[:-1] + "+00:00").astimezone(timezone.utc)
        parsed = datetime.fromisoformat(text)
        if parsed.tzinfo is None:
            return parsed.replace(tzinfo=timezone.utc)
        return parsed.astimezone(timezone.utc)
    except Exception:
        return None


def read_learning_snapshot(group_id: str) -> Dict[str, Any]:
    group = load_group(group_id)
    if group is None:
        return {"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}}

    now = datetime.now(timezone.utc)
    day_ago = now.timestamp() - 24 * 60 * 60
    week_ago = now.timestamp() - 7 * 24 * 60 * 60
    usage_events = procedural_skills.load_skill_usage_events(group)
    patch_candidates = procedural_skills.load_skill_patch_candidates(group)
    skills = procedural_skills.list_procedural_skill_assets(group, limit=500)

    usage_by_event_id: Dict[str, Dict[str, Any]] = {}
    usage_by_skill: Dict[str, list[Dict[str, Any]]] = {}
    usage_events_24h = 0
    usage_events_7d = 0
    below_threshold_count = 0
    threshold = float(procedural_skills.patch_candidate_score_threshold() or 0.0)

    for event in usage_events:
        if not isinstance(event, dict):
            continue
        event_id = str(event.get("event_id") or "").strip()
        skill_id = str(event.get("skill_id") or "").strip()
        captured_at = _parse_iso_utc(event.get("captured_at"))
        score = float(event.get("score") or 0.0)
        if captured_at is not None:
            if captured_at.timestamp() >= day_ago:
                usage_events_24h += 1
            if captured_at.timestamp() >= week_ago:
                usage_events_7d += 1
        if score < threshold:
            below_threshold_count += 1
        if event_id:
            usage_by_event_id[event_id] = dict(event)
        if skill_id:
            usage_by_skill.setdefault(skill_id, []).append(dict(event))

    pending_patches: list[Dict[str, Any]] = []
    merged_patch_count_7d = 0
    rejected_patch_count_7d = 0
    manual_review_pending_count = 0
    merged_candidates_by_id: Dict[str, Dict[str, Any]] = {}
    patch_candidates_by_id: Dict[str, Dict[str, Any]] = {}
    merged_candidate_total = 0
    for item in patch_candidates:
        if not isinstance(item, dict):
            continue
        candidate_id = str(item.get("candidate_id") or "").strip()
        if candidate_id:
            patch_candidates_by_id[candidate_id] = dict(item)
        status = str(item.get("status") or "").strip().lower()
        updated_at = _parse_iso_utc(item.get("updated_at"))
        if status == "pending":
            evidence_refs = [str(ref).strip() for ref in (item.get("evidence_refs") or []) if str(ref).strip()]
            evidence_items = [usage_by_event_id.get(ref, {}) for ref in evidence_refs]
            last_evidence_at = ""
            latest_dt: Optional[datetime] = None
            sample_event = next((event for event in evidence_items if isinstance(event, dict) and event), {})
            for event in evidence_items:
                captured = _parse_iso_utc((event or {}).get("captured_at")) if isinstance(event, dict) else None
                if captured is not None and (latest_dt is None or captured > latest_dt):
                    latest_dt = captured
                    last_evidence_at = captured.isoformat().replace("+00:00", "Z")
            pending_patches.append(
                {
                    "candidate_id": candidate_id,
                    "skill_id": str(item.get("skill_id") or "").strip(),
                    "source_experience_candidate_id": str(item.get("source_experience_candidate_id") or "").strip(),
                    "patch_kind": str(item.get("patch_kind") or "").strip(),
                    "reason": str(item.get("reason") or "").strip(),
                    "score": float(item.get("score") or 0.0),
                    "created_at": str(item.get("created_at") or "").strip(),
                    "updated_at": str(item.get("updated_at") or "").strip(),
                    "evidence_count": len(evidence_refs),
                    "last_evidence_at": last_evidence_at,
                    "sample_evidence_type": str(sample_event.get("evidence_type") or "").strip(),
                    "sample_outcome": str(sample_event.get("outcome") or "").strip(),
                    "review_mode": str(item.get("review_mode") or "").strip() or "auto_merge_eligible",
                    "regressed_from_candidate_id": str(
                        (
                            item.get("lineage")
                            if isinstance(item.get("lineage"), dict)
                            else {}
                        ).get("regressed_from_candidate_id")
                        or ""
                    ).strip(),
                }
            )
            if str(item.get("review_mode") or "").strip() == "manual_review_required":
                manual_review_pending_count += 1
        elif status == "merged":
            merged_candidate_total += 1
            if candidate_id:
                merged_candidates_by_id[candidate_id] = dict(item)
            if updated_at is not None and updated_at.timestamp() >= week_ago:
                merged_patch_count_7d += 1
        elif status == "rejected":
            if updated_at is not None and updated_at.timestamp() >= week_ago:
                rejected_patch_count_7d += 1

    pending_patches.sort(
        key=lambda item: (
            float(item.get("score") or 0.0),
            str(item.get("updated_at") or ""),
            str(item.get("created_at") or ""),
        ),
        reverse=True,
    )

    recent_learning: list[Dict[str, Any]] = []
    observing_skills: list[Dict[str, Any]] = []
    skill_items: list[Dict[str, Any]] = []
    active_skill_count = 0
    observing_skill_count = 0
    for skill in skills:
        if not isinstance(skill, dict):
            continue
        skill_id = str(skill.get("skill_id") or "").strip()
        if not skill_id:
            continue
        status = str(skill.get("status") or "active").strip() or "active"
        if status.lower() == "active":
            active_skill_count += 1
        history = skill.get("history") if isinstance(skill.get("history"), list) else []
        evaluation = skill.get("post_merge_evaluation") if isinstance(skill.get("post_merge_evaluation"), dict) else {}
        eval_status = str(evaluation.get("status") or "").strip()
        governance_policy = skill.get("governance_policy") if isinstance(skill.get("governance_policy"), dict) else {}
        stability = str(skill.get("stability") or "").strip() or "stable"
        patch_review_mode = str(governance_policy.get("patch_review_mode") or "").strip() or "auto_merge_eligible"
        skill_items.append(
            {
                "skill_id": skill_id,
                "source_experience_candidate_id": str(skill.get("source_experience_candidate_id") or "").strip(),
                "title": str(skill.get("title") or "").strip(),
                "goal": str(skill.get("goal") or "").strip(),
                "steps": [str(step).strip() for step in (skill.get("steps") or []) if str(step).strip()],
                "constraints": [str(step).strip() for step in (skill.get("constraints") or []) if str(step).strip()],
                "failure_signals": [str(step).strip() for step in (skill.get("failure_signals") or []) if str(step).strip()],
                "status": status,
                "stability": stability,
                "review_mode": patch_review_mode,
                "updated_at": str(skill.get("updated_at") or "").strip(),
            }
        )
        followup_candidate_id = str(evaluation.get("followup_candidate_id") or "").strip()
        followup_candidate = patch_candidates_by_id.get(followup_candidate_id, {})
        followup_lineage = (
            followup_candidate.get("lineage")
            if isinstance(followup_candidate.get("lineage"), dict)
            else {}
        )
        if eval_status == "observing":
            observing_skill_count += 1
        if eval_status in {"observing", "needs_followup", "regressed"}:
            observing_skills.append(
                {
                    "skill_id": skill_id,
                    "source_experience_candidate_id": str(skill.get("source_experience_candidate_id") or "").strip(),
                    "title": str(skill.get("title") or "").strip(),
                    "goal": str(skill.get("goal") or "").strip(),
                    "status": eval_status,
                    "candidate_id": str(evaluation.get("candidate_id") or "").strip(),
                    "stability": stability,
                    "patch_review_mode": patch_review_mode,
                    "opened_at": str(evaluation.get("opened_at") or "").strip(),
                    "observe_until": str(evaluation.get("observe_until") or "").strip(),
                    "observed_at": str(evaluation.get("observed_at") or "").strip(),
                    "followup_candidate_id": followup_candidate_id,
                    "followup_review_mode": str(followup_candidate.get("review_mode") or "").strip(),
                    "regressed_from_candidate_id": str(
                        followup_lineage.get("regressed_from_candidate_id") or ""
                    ).strip(),
                }
            )
        latest_merged_event: Optional[Dict[str, Any]] = None
        latest_merged_at: Optional[datetime] = None
        for raw_event in history:
            if not isinstance(raw_event, dict):
                continue
            if str(raw_event.get("action") or "").strip() != "patch_merged":
                continue
            merged_at = _parse_iso_utc(raw_event.get("at"))
            if latest_merged_event is not None and merged_at is not None and latest_merged_at is not None and merged_at <= latest_merged_at:
                continue
            latest_merged_event = raw_event
            latest_merged_at = merged_at
        if latest_merged_event is not None:
            raw_event = latest_merged_event
            merged_at = latest_merged_at
            candidate_id = str(raw_event.get("candidate_id") or "").strip()
            patch = merged_candidates_by_id.get(candidate_id, {})
            evidence_refs = [str(ref).strip() for ref in (patch.get("evidence_refs") or []) if str(ref).strip()]
            consumed_after_merge = 0
            if merged_at is not None:
                for event in usage_by_skill.get(skill_id, []):
                    captured_at = _parse_iso_utc(event.get("captured_at"))
                    if captured_at is not None and captured_at >= merged_at:
                        consumed_after_merge += 1
            recent_learning.append(
                {
                    "candidate_id": candidate_id,
                    "skill_id": skill_id,
                    "source_experience_candidate_id": str(skill.get("source_experience_candidate_id") or "").strip(),
                    "title": str(skill.get("title") or "").strip(),
                    "patch_kind": str(raw_event.get("patch_kind") or patch.get("patch_kind") or "").strip(),
                    "reason": str(patch.get("reason") or "").strip(),
                    "score": float(patch.get("score") or 0.0),
                    "merged_at": str(raw_event.get("at") or "").strip(),
                    "merged_by": str(raw_event.get("by") or "").strip(),
                    "evidence_count": len(evidence_refs),
                    "runtime_consumed_count": consumed_after_merge,
                    "post_merge_status": eval_status,
                    "stability": stability,
                    "patch_review_mode": patch_review_mode,
                    "observed_at": str(evaluation.get("observed_at") or "").strip(),
                    "followup_candidate_id": followup_candidate_id,
                    "followup_review_mode": str(followup_candidate.get("review_mode") or "").strip(),
                    "regressed_from_candidate_id": str(
                        followup_lineage.get("regressed_from_candidate_id") or ""
                    ).strip(),
                }
            )

    recent_learning.sort(key=lambda item: str(item.get("merged_at") or ""), reverse=True)
    observing_skills.sort(
        key=lambda item: (
            str(item.get("observed_at") or ""),
            str(item.get("opened_at") or ""),
        ),
        reverse=True,
    )
    skill_items.sort(
        key=lambda item: (
            1 if str(item.get("status") or "").strip().lower() == "active" else 0,
            str(item.get("updated_at") or ""),
            str(item.get("title") or ""),
        ),
        reverse=True,
    )

    runtime_consumed_count = len(
        [item for item in recent_learning if int(item.get("runtime_consumed_count") or 0) > 0]
    )
    return {
        "ok": True,
        "result": {
            "overview": {
                "usage_events_24h": usage_events_24h,
                "usage_events_7d": usage_events_7d,
                "pending_patch_count": len(pending_patches),
                "merged_patch_count_7d": merged_patch_count_7d,
                "rejected_patch_count_7d": rejected_patch_count_7d,
                "active_skill_count": active_skill_count,
                "observing_skill_count": observing_skill_count,
                "runtime_consumed_recent_count": runtime_consumed_count,
            },
            "funnel": {
                "evidence_count": len(usage_events),
                "below_threshold_count": below_threshold_count,
                "candidate_created_count": len(patch_candidates),
                "candidate_ready_count": len(pending_patches),
                "pending_review_count": manual_review_pending_count,
                "merged_count": merged_candidate_total,
                "runtime_consumed_count": runtime_consumed_count,
                "threshold": threshold,
            },
            "pending_patches": pending_patches[:8],
            "recent_learning": recent_learning[:8],
            "observing_skills": observing_skills[:8],
            "skills": skill_items,
        },
    }


def create_procedural_skill(group_id: str, req: ProceduralSkillUpsertRequest) -> Dict[str, Any]:
    group = load_group(group_id)
    if group is None:
        return {"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}}
    try:
        skill = procedural_skills.create_manual_procedural_skill_asset(
            group,
            skill_id=req.skill_id,
            title=req.title,
            goal=req.goal,
            steps=req.steps,
            constraints=req.constraints,
            failure_signals=req.failure_signals,
            stability=req.stability,
            review_mode=req.review_mode,
            status=req.status,
            source_experience_candidate_id=req.source_experience_candidate_id,
        )
    except ValueError as exc:
        return {"ok": False, "error": {"code": "invalid_procedural_skill", "message": str(exc)}}
    return {"ok": True, "result": {"skill": skill}}


def update_procedural_skill(group_id: str, skill_id: str, req: ProceduralSkillUpsertRequest) -> Dict[str, Any]:
    group = load_group(group_id)
    if group is None:
        return {"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}}
    try:
        skill = procedural_skills.update_procedural_skill_asset(
            group,
            skill_id=skill_id,
            title=req.title,
            goal=req.goal,
            steps=req.steps,
            constraints=req.constraints,
            failure_signals=req.failure_signals,
            stability=req.stability,
            review_mode=req.review_mode,
            status=req.status,
        )
    except ValueError as exc:
        return {"ok": False, "error": {"code": "invalid_procedural_skill", "message": str(exc)}}
    return {"ok": True, "result": {"skill": skill}}


def delete_procedural_skill(group_id: str, skill_id: str) -> Dict[str, Any]:
    group = load_group(group_id)
    if group is None:
        return {"ok": False, "error": {"code": "group_not_found", "message": f"group not found: {group_id}"}}
    existing = procedural_skills.load_procedural_skill_asset(group, skill_id=skill_id)
    if existing is None:
        return {"ok": False, "error": {"code": "procedural_skill_not_found", "message": f"procedural skill not found: {skill_id}"}}
    result = procedural_skills.delete_procedural_skill_asset(group, skill_id=skill_id)
    if str(result.get("status") or "").strip() == "skipped":
        return {"ok": False, "error": {"code": "procedural_skill_delete_failed", "message": f"failed to delete procedural skill: {skill_id}"}}
    return {"ok": True, "result": {"skill_id": skill_id}}


def create_routers(_ctx: RouteContext) -> list[APIRouter]:
    group_router = APIRouter(prefix="/api/v1/groups/{group_id}", dependencies=[Depends(require_group)])

    @group_router.get("/learning")
    async def group_learning(group_id: str) -> Dict[str, Any]:
        return await run_in_threadpool(read_learning_snapshot, group_id)

    @group_router.post("/learning/skills")
    async def group_learning_create_skill(group_id: str, req: ProceduralSkillUpsertRequest) -> Dict[str, Any]:
        return await run_in_threadpool(create_procedural_skill, group_id, req)

    @group_router.put("/learning/skills/{skill_id}")
    async def group_learning_update_skill(group_id: str, skill_id: str, req: ProceduralSkillUpsertRequest) -> Dict[str, Any]:
        return await run_in_threadpool(update_procedural_skill, group_id, skill_id, req)

    @group_router.delete("/learning/skills/{skill_id}")
    async def group_learning_delete_skill(group_id: str, skill_id: str) -> Dict[str, Any]:
        return await run_in_threadpool(delete_procedural_skill, group_id, skill_id)

    return [group_router]


def register_group_learning_routes(app: FastAPI, *, ctx: RouteContext) -> None:
    for router in create_routers(ctx):
        app.include_router(router)
