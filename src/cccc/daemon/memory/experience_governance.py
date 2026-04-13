from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, List, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_foreman
from ...kernel.group import load_group
from ...kernel import procedural_skills
from ...util.conv import coerce_bool
from ...util.time import utc_now_iso
from . import experience_assets, experience_memory_lane as memory_lane
from .experience_common import (
    _PROMOTED_STATUSES,
    _RETIRED_STATUSES,
    _add_history_event,
    _append_unique,
    _candidate_governance,
    _candidate_is_promoted,
    _candidate_ref,
    _candidate_review,
    _governance_conflict,
    _governance_lineage_ids,
    _lineage_source_refs,
    _string_list,
)


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _resolve_governance_actor(*, group_id: str, by: str) -> tuple[Optional[DaemonResponse], str]:
    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}"), ""

    foreman = find_foreman(group)
    foreman_id = str((foreman or {}).get("id") or "").strip() if isinstance(foreman, dict) else ""
    if by != "user" and by != foreman_id:
        return (
            _error(
                "permission_denied",
                "permission denied: only user or foreman can govern experience assets",
                details={"by": by, "foreman_id": foreman_id},
            ),
            "",
        )
    return None, foreman_id


def handle_procedural_skill_usage_report(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    skill_id = str(args.get("skill_id") or "").strip()
    by = str(args.get("by") or "").strip()
    actor_id = str(args.get("actor_id") or by).strip()
    turn_id = str(args.get("turn_id") or "").strip()
    outcome = str(args.get("outcome") or "").strip()
    evidence_type = str(args.get("evidence_type") or "").strip()
    reason = str(args.get("reason") or "").strip()
    patch_kind = str(args.get("patch_kind") or "").strip()
    generate_patch = coerce_bool(args.get("generate_patch"), default=False)
    evidence_payload = args.get("evidence_payload") if isinstance(args.get("evidence_payload"), dict) else {}
    proposed_delta = args.get("proposed_delta") if isinstance(args.get("proposed_delta"), dict) else {}

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not skill_id:
        return _error("validation_error", "skill_id is required")
    if not by:
        return _error("validation_error", "by is required")
    if not turn_id:
        return _error("validation_error", "turn_id is required")
    if not evidence_type:
        return _error("validation_error", "evidence_type is required")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    try:
        evidence = procedural_skills.append_skill_usage_evidence(
            group,
            skill_id=skill_id,
            actor_id=actor_id,
            turn_id=turn_id,
            evidence_type=evidence_type,
            evidence_payload=evidence_payload,
            outcome=outcome,
        )
        patch_candidate: Dict[str, Any] | None = None
        post_merge_evaluation: Dict[str, Any] = {}
        patch_gate: Dict[str, Any] = {
            "status": "not_requested",
            "score": float(evidence.get("score") or 0.0),
            "threshold": procedural_skills.patch_candidate_score_threshold(),
        }
        if generate_patch:
            if not patch_kind:
                raise ValueError("patch_kind is required when generate_patch=true")
            if not isinstance(proposed_delta, dict) or not proposed_delta:
                raise ValueError("proposed_delta is required when generate_patch=true")
            if patch_gate["score"] < patch_gate["threshold"]:
                patch_gate["status"] = "below_threshold"
            else:
                patch_candidate = procedural_skills.create_skill_patch_candidate(
                    group,
                    skill_id=skill_id,
                    actor_id=actor_id or by,
                    evidence=evidence,
                    patch_kind=patch_kind,
                    reason=reason or evidence_type,
                    proposed_delta=proposed_delta,
                )
                patch_gate["status"] = "candidate_ready"
        post_merge_evaluation = procedural_skills.evaluate_post_merge_skill_observation(
            group,
            skill_id=skill_id,
            evidence=evidence,
            patch_candidate=patch_candidate,
        )
        if (
            isinstance(patch_candidate, dict)
            and patch_candidate
            and str(post_merge_evaluation.get("status") or "").strip() == "regressed"
        ):
            parent_candidate_id = ""
            asset = post_merge_evaluation.get("asset") if isinstance(post_merge_evaluation.get("asset"), dict) else {}
            asset_eval = asset.get("post_merge_evaluation") if isinstance(asset.get("post_merge_evaluation"), dict) else {}
            parent_candidate_id = str(asset_eval.get("candidate_id") or "").strip()
            if parent_candidate_id:
                patch_candidate = procedural_skills.mark_patch_candidate_as_regressed_followup(
                    group,
                    candidate_id=str(patch_candidate.get("candidate_id") or "").strip(),
                    parent_candidate_id=parent_candidate_id,
                )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group_id,
                "skill_id": skill_id,
                "usage_evidence": evidence,
                "patch_candidate": patch_candidate or {},
                "patch_gate": patch_gate,
                "post_merge_evaluation": post_merge_evaluation,
            },
        )
    except Exception as exc:
        return _error("validation_error", str(exc))


def handle_procedural_skill_patch_governance(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "").strip()
    candidate_id = str(args.get("candidate_id") or "").strip()
    lifecycle_action = str(args.get("lifecycle_action") or "").strip().lower()
    reason = str(args.get("reason") or "").strip()

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not by:
        return _error("validation_error", "by is required")
    if not candidate_id:
        return _error("validation_error", "candidate_id is required")
    if lifecycle_action not in {"merge", "reject"}:
        return _error("validation_error", "lifecycle_action must be one of: merge, reject")

    permission_error, _ = _resolve_governance_actor(group_id=group_id, by=by)
    if permission_error is not None:
        return permission_error

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    candidates = procedural_skills.load_skill_patch_candidates(group)
    target_index = next(
        (idx for idx, item in enumerate(candidates) if str(item.get("candidate_id") or "").strip() == candidate_id),
        -1,
    )
    if target_index < 0:
        return _error("candidate_not_found", f"skill patch candidate not found: {candidate_id}")

    current = dict(candidates[target_index])
    status = str(current.get("status") or "").strip().lower()
    if status in {"merged", "rejected"}:
        return _error("validation_error", f"skill patch candidate already finalized via {status}")

    current["status"] = "merged" if lifecycle_action == "merge" else "rejected"
    current["updated_at"] = utc_now_iso()
    governance = current.get("governance") if isinstance(current.get("governance"), dict) else {}
    governance[lifecycle_action] = {"by": by, "at": current["updated_at"], "reason": reason}
    current["governance"] = governance

    merge_result: Dict[str, Any] = {"status": "skipped", "file_path": "", "asset": {}}
    if lifecycle_action == "merge":
        try:
            merge_result = procedural_skills.apply_skill_patch_candidate(group, candidate=current, actor_id=by)
        except Exception as exc:
            return _error("validation_error", str(exc))

    candidates[target_index] = current
    procedural_skills.persist_skill_patch_candidates(group, candidates)
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group_id,
            "candidate_id": candidate_id,
            "lifecycle_action": lifecycle_action,
            "candidate": current,
            "skill_write": merge_result,
        },
    )


def handle_experience_governance(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    by = str(args.get("by") or "").strip()
    lifecycle_action = str(args.get("lifecycle_action") or "").strip().lower()
    dry_run = coerce_bool(args.get("dry_run"), default=False)
    reason = str(args.get("reason") or "").strip()

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not by:
        return _error("validation_error", "by is required")
    if lifecycle_action not in {"reject", "merge", "supersede"}:
        return _error("validation_error", "lifecycle_action must be one of: reject, merge, supersede")

    permission_error, _ = _resolve_governance_actor(group_id=group_id, by=by)
    if permission_error is not None:
        return permission_error

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    candidates = memory_lane.load_experience_candidates(group)
    by_id: Dict[str, Dict[str, Any]] = {}
    for item in candidates:
        candidate_id = str(item.get("id") or "").strip()
        if candidate_id:
            by_id[candidate_id] = dict(item)

    changed_ids: List[str] = []
    now = utc_now_iso()
    memory_mutations: List[Dict[str, Any]] = []
    memory_effects: List[Dict[str, Any]] = []

    def _remember_changed(candidate_id: str) -> None:
        if candidate_id not in changed_ids:
            changed_ids.append(candidate_id)

    def _require_candidate(candidate_id: str) -> Dict[str, Any]:
        item = by_id.get(candidate_id)
        if item is None:
            raise ValueError(f"candidate not found: {candidate_id}")
        return item

    def _queue_retire_memory(candidate: Dict[str, Any], *, candidate_id: str) -> None:
        locator = memory_lane.require_memory_locator(candidate, candidate_id=candidate_id)
        entry_id = str(locator.get("entry_id") or "").strip()
        retired = dict(candidate)
        retired["updated_at"] = now
        memory_mutations.append(
            {
                "action": "upsert",
                "entry_id": entry_id,
                "block": memory_lane.render_structured_memory_entry(
                    memory_lane.build_experience_memory_entry(
                        group_id=group_id,
                        candidate=retired,
                        actor_id=by,
                        entry_id=entry_id,
                        created_at=now,
                    )
                ),
            }
        )
        memory_effects.append({"candidate_id": candidate_id, "action": "tombstone", "entry_id": entry_id})

    def _queue_upsert_memory(candidate: Dict[str, Any], *, candidate_id: str, entry_id: Optional[str] = None) -> Dict[str, Any]:
        resolved_entry_id = str(entry_id or memory_lane.experience_memory_entry_id(candidate_id)).strip()
        entry = memory_lane.build_experience_memory_entry(
            group_id=group_id,
            candidate=candidate,
            actor_id=by,
            entry_id=resolved_entry_id,
            created_at=now,
        )
        memory_mutations.append(
            {
                "action": "upsert",
                "entry_id": resolved_entry_id,
                "block": memory_lane.render_structured_memory_entry(entry),
            }
        )
        memory_effects.append({"candidate_id": candidate_id, "action": "replace", "entry_id": resolved_entry_id})
        return {
            "entry_id": resolved_entry_id,
            "file_path": str(memory_lane.resolve_memory_layout(group_id, ensure_files=False).memory_file),
        }

    try:
        if lifecycle_action == "reject":
            candidate_id = str(args.get("candidate_id") or "").strip()
            if not candidate_id:
                raise ValueError("candidate_id is required for reject")
            candidate = _require_candidate(candidate_id)
            was_promoted = _candidate_is_promoted(candidate)
            status = str(candidate.get("status") or "").strip()
            if status in {"merged", "superseded"}:
                raise ValueError(f"candidate is already retired via {status} and cannot be rejected")
            updated = dict(candidate)
            updated["status"] = "rejected"
            updated["updated_at"] = now
            governance = _candidate_governance(updated)
            governance["rejected"] = {"by": by, "at": now, "reason": reason}
            _add_history_event(governance, {"action": "reject", "by": by, "at": now, "reason": reason})
            governance["lineage_source_refs"] = _lineage_source_refs(updated)
            updated["governance"] = governance
            review = _candidate_review(updated)
            if reason:
                review["rejected_reason"] = reason
                updated["review"] = review
            by_id[candidate_id] = updated
            _remember_changed(candidate_id)
            if was_promoted:
                _queue_retire_memory(updated, candidate_id=candidate_id)
        else:
            target_candidate_id = str(args.get("target_candidate_id") or "").strip()
            source_candidate_ids = _string_list(args.get("source_candidate_ids"))
            if not target_candidate_id:
                raise ValueError("target_candidate_id is required")
            if not source_candidate_ids:
                raise ValueError("source_candidate_ids is required")
            if target_candidate_id in source_candidate_ids:
                raise ValueError("target_candidate_id cannot appear in source_candidate_ids")

            target = dict(_require_candidate(target_candidate_id))
            target_was_promoted = _candidate_is_promoted(target)
            target_status = str(target.get("status") or "").strip()
            if target_status in _RETIRED_STATUSES:
                raise ValueError(f"target candidate is already retired via {target_status}")

            target_governance = _candidate_governance(target)
            target_refs = _lineage_source_refs(target)
            lineage_candidate_ids = _governance_lineage_ids(target)
            event_sources: List[str] = []
            promoted_source_ids: List[str] = []

            for source_candidate_id in source_candidate_ids:
                source = dict(_require_candidate(source_candidate_id))
                source_was_promoted = _candidate_is_promoted(source)
                source_status = str(source.get("status") or "").strip()
                target_field = "merged_into" if lifecycle_action == "merge" else "superseded_by"
                conflicting_target = _governance_conflict(source, target_field=target_field, target_id=target_candidate_id)
                if conflicting_target is not None:
                    raise ValueError(
                        f"candidate {source_candidate_id} already points to {target_field}={conflicting_target}"
                    )
                if source_status == "rejected":
                    raise ValueError(f"candidate {source_candidate_id} is rejected and cannot be {lifecycle_action}d")
                if source_status in _RETIRED_STATUSES and not (
                    source_status == ("merged" if lifecycle_action == "merge" else "superseded")
                ):
                    raise ValueError(
                        f"candidate {source_candidate_id} is already retired via {source_status} and cannot be {lifecycle_action}d"
                    )

                source_governance = _candidate_governance(source)
                source_governance[target_field] = target_candidate_id
                source_governance["lineage_source_refs"] = _lineage_source_refs(source)
                _add_history_event(
                    source_governance,
                    {
                        "action": lifecycle_action,
                        "by": by,
                        "at": now,
                        "reason": reason,
                        "target_candidate_id": target_candidate_id,
                    },
                )
                source["governance"] = source_governance
                source["status"] = "merged" if lifecycle_action == "merge" else "superseded"
                source["updated_at"] = now
                by_id[source_candidate_id] = source
                _remember_changed(source_candidate_id)
                if source_was_promoted:
                    promoted_source_ids.append(source_candidate_id)

                target_refs = _append_unique(target_refs, *_lineage_source_refs(source), _candidate_ref(source_candidate_id))
                lineage_candidate_ids = _append_unique(
                    lineage_candidate_ids,
                    source_candidate_id,
                    *_governance_lineage_ids(source),
                )
                event_sources.append(source_candidate_id)

            target_governance["lineage_source_refs"] = target_refs
            target_governance["lineage_candidate_ids"] = lineage_candidate_ids
            relation_field = "merged_from" if lifecycle_action == "merge" else "supersedes"
            target_governance[relation_field] = _append_unique(
                _string_list(target_governance.get(relation_field)),
                *event_sources,
            )
            _add_history_event(
                target_governance,
                {
                    "action": lifecycle_action,
                    "by": by,
                    "at": now,
                    "reason": reason,
                    "source_candidate_ids": event_sources,
                },
            )
            target["governance"] = target_governance
            target["source_refs"] = target_refs
            target["updated_at"] = now
            if promoted_source_ids and not target_was_promoted:
                raise ValueError("promoted source requires target to already be promoted_to_memory")
            target_should_promote = target_was_promoted
            if target_should_promote:
                target["status"] = "promoted_to_memory"
                promotion_meta = target.get("promotion") if isinstance(target.get("promotion"), dict) else {}
                existing_memory_entry = promotion_meta.get("memory_entry") if isinstance(promotion_meta.get("memory_entry"), dict) else {}
                resolved_entry_id = str(existing_memory_entry.get("entry_id") or "").strip() or memory_lane.experience_memory_entry_id(target_candidate_id)
                promotion_meta["by"] = by
                promotion_meta["at"] = now
                promotion_meta["target"] = "memory"
                promotion_meta["memory_entry"] = _queue_upsert_memory(
                    target,
                    candidate_id=target_candidate_id,
                    entry_id=resolved_entry_id,
                )
                target["promotion"] = promotion_meta
            by_id[target_candidate_id] = target
            _remember_changed(target_candidate_id)
            for promoted_source_id in promoted_source_ids:
                retired_source = _require_candidate(promoted_source_id)
                _queue_retire_memory(retired_source, candidate_id=promoted_source_id)
    except ValueError as exc:
        return _error("validation_error", str(exc))

    updated_candidates: List[Dict[str, Any]] = []
    for item in candidates:
        candidate_id = str(item.get("id") or "").strip()
        updated_candidates.append(by_id.get(candidate_id, item))

    if not dry_run:
        candidates_path = group.path / "state" / "experience_candidates.json"
        memory_plan = memory_lane.compute_memory_mutation_plan(group_id=group_id, mutations=memory_mutations) if memory_mutations else None
        try:
            if memory_plan is not None:
                memory_lane.write_memory_content(group_id=group_id, content=str(memory_plan.get("updated_text") or ""))
            memory_lane.persist_candidates(candidates_path, updated_candidates)
        except Exception as exc:
            if memory_plan is not None:
                try:
                    memory_lane.write_memory_content(group_id=group_id, content=str(memory_plan.get("current_text") or ""))
                except Exception:
                    pass
            return _error("memory_sync_error", str(exc))
        experience_asset_effects: List[Dict[str, Any]] = []
        procedural_skill_effects: List[Dict[str, Any]] = []
        for candidate_id in changed_ids:
            candidate = by_id.get(candidate_id)
            if not isinstance(candidate, dict):
                continue
            locator = memory_lane.memory_locator_from_candidate(candidate)
            memory_entry = locator if locator else None
            asset_effect = experience_assets.sync_experience_asset_mirror(
                group=group,
                candidate=candidate,
                memory_entry=memory_entry,
            )
            asset_effect["candidate_id"] = candidate_id
            experience_asset_effects.append(asset_effect)
            effect = experience_assets.sync_procedural_skill_mirror(
                group=group,
                candidate=candidate,
                memory_entry=memory_entry,
            )
            effect["candidate_id"] = candidate_id
            procedural_skill_effects.append(effect)
        index_sync_result = memory_lane.run_index_sync_after_commit(group_id=group_id) if memory_plan is not None else {
            "status": "skipped",
            "commit_state": "candidate_only",
        }
    else:
        experience_asset_effects = []
        procedural_skill_effects = []
        index_sync_result = {"status": "skipped", "commit_state": "dry_run"}

    preview: List[Dict[str, Any]] = []
    for candidate_id in changed_ids:
        item = by_id.get(candidate_id)
        if isinstance(item, dict):
            preview.append(item)

    return DaemonResponse(
        ok=True,
        result={
            "group_id": group_id,
            "lifecycle_action": lifecycle_action,
            "dry_run": dry_run,
            "changed_candidate_ids": changed_ids,
            "candidates": preview,
            "memory_effects": memory_effects,
            "experience_asset_effects": experience_asset_effects,
            "procedural_skill_effects": procedural_skill_effects,
            "index_sync": index_sync_result,
            "commit_state": str(index_sync_result.get("commit_state") or "candidate_only"),
        },
    )


def handle_experience_repair_memory(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    candidate_id = str(args.get("candidate_id") or "").strip()
    by = str(args.get("by") or "").strip()
    dry_run = coerce_bool(args.get("dry_run"), default=False)

    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not candidate_id:
        return _error("validation_error", "candidate_id is required")
    if not by:
        return _error("validation_error", "by is required")

    permission_error, _ = _resolve_governance_actor(group_id=group_id, by=by)
    if permission_error is not None:
        return permission_error

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    candidates = memory_lane.load_experience_candidates(group)
    candidate_index = -1
    for idx, item in enumerate(candidates):
        if str(item.get("id") or "").strip() == candidate_id:
            candidate_index = idx
            break
    if candidate_index < 0:
        return _error("candidate_not_found", f"candidate not found: {candidate_id}")

    current = dict(candidates[candidate_index])
    if str(current.get("status") or "").strip() != "promoted_to_memory":
        return _error("validation_error", "repair only accepts promoted_to_memory candidates")

    memory_file_path = memory_lane.memory_file_path(group_id)
    promotion = current.get("promotion") if isinstance(current.get("promotion"), dict) else {}
    existing_locator = promotion.get("memory_entry") if isinstance(promotion.get("memory_entry"), dict) else {}
    existing_file_path = str(existing_locator.get("file_path") or "").strip()
    if existing_file_path and existing_file_path != memory_file_path:
        return _error("validation_error", "repair only supports MEMORY.md targets")

    memory_file = Path(memory_file_path)
    memory_text = memory_file.read_text(encoding="utf-8", errors="replace") if memory_file.exists() else ""
    entry_id = memory_lane.experience_memory_entry_id(candidate_id)
    structured_block = memory_lane.find_memory_entry_block(memory_text, entry_id=entry_id)
    if memory_lane.is_structured_experience_block(structured_block, candidate_id=candidate_id, entry_id=entry_id):
        updated = dict(current)
        promotion_meta = updated.get("promotion") if isinstance(updated.get("promotion"), dict) else {}
        canonical_memory_entry = {"entry_id": entry_id, "file_path": memory_file_path}
        locator_missing = (
            str(existing_locator.get("entry_id") or "").strip() != entry_id
            or str(existing_locator.get("file_path") or "").strip() != memory_file_path
        )
        promotion_meta["memory_entry"] = canonical_memory_entry
        updated["promotion"] = promotion_meta
        commit_state = "dry_run" if dry_run else "no_change"
        asset_write = {"status": "skipped", "file_path": "", "asset_id": f"expasset_{candidate_id}"}
        skill_write = {"status": "skipped", "file_path": "", "skill_id": f"procskill_{candidate_id}"}
        if locator_missing and not dry_run:
            candidates[candidate_index] = updated
            candidates_path = group.path / "state" / "experience_candidates.json"
            memory_lane.persist_candidates(candidates_path, candidates)
            commit_state = "candidate_committed"
        if not dry_run:
            asset_write = experience_assets.sync_experience_asset_mirror(
                group=group,
                candidate=updated,
                memory_entry=canonical_memory_entry,
            )
            skill_write = experience_assets.sync_procedural_skill_mirror(
                group=group,
                candidate=updated,
                memory_entry=canonical_memory_entry,
            )
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group_id,
                "candidate_id": candidate_id,
                "dry_run": dry_run,
                "already_structured": True,
                "candidate": updated,
                "commit_state": commit_state,
                "asset_write": asset_write,
                "skill_write": skill_write,
                "index_sync": {"status": "skipped", "commit_state": commit_state},
            },
        )

    legacy = memory_lane.legacy_memory_block_range(text=memory_text, candidate=current)
    if len(legacy) != 1:
        return _error(
            "validation_error",
            "legacy promoted memory block must match exactly once in MEMORY.md",
            details={"match_count": len(legacy)},
        )

    match = legacy[0]
    updated = dict(current)
    updated["updated_at"] = utc_now_iso()
    promotion_meta = updated.get("promotion") if isinstance(updated.get("promotion"), dict) else {}
    promotion_meta["by"] = by
    promotion_meta["at"] = str(updated.get("updated_at") or "")
    promotion_meta["target"] = "memory"
    promotion_meta["memory_entry"] = {"entry_id": entry_id, "file_path": memory_file_path}
    updated["promotion"] = promotion_meta

    replacement_block = memory_lane.render_structured_memory_entry(
        memory_lane.build_experience_memory_entry(
            group_id=group_id,
            candidate=updated,
            actor_id=by,
            entry_id=entry_id,
            created_at=str(updated.get("updated_at") or ""),
        )
    )
    updated_text = memory_lane.normalize_memory_text(
        memory_text[: int(match["start"])] + replacement_block + memory_text[int(match["end"]) :]
    )

    if dry_run:
        return DaemonResponse(
            ok=True,
            result={
                "group_id": group_id,
                "candidate_id": candidate_id,
                "dry_run": True,
                "already_structured": False,
                "legacy_match": {
                    "start_line": int(match.get("start_line") or 0),
                    "end_line": int(match.get("end_line") or 0),
                },
                "candidate": updated,
                "memory_entry_preview": replacement_block,
                "commit_state": "dry_run",
                "index_sync": {"status": "skipped", "commit_state": "dry_run"},
            },
        )

    candidates_path = group.path / "state" / "experience_candidates.json"
    try:
        write_result = memory_lane.write_memory_content(group_id=group_id, content=updated_text)
        candidates[candidate_index] = updated
        memory_lane.persist_candidates(candidates_path, candidates)
    except Exception as exc:
        try:
            memory_lane.write_memory_content(group_id=group_id, content=memory_text)
        except Exception:
            pass
        return _error("memory_sync_error", str(exc))

    asset_write = experience_assets.sync_experience_asset_mirror(
        group=group,
        candidate=updated,
        memory_entry={"entry_id": entry_id, "file_path": memory_file_path},
    )
    skill_write = experience_assets.sync_procedural_skill_mirror(
        group=group,
        candidate=updated,
        memory_entry={"entry_id": entry_id, "file_path": memory_file_path},
    )
    index_sync_result = memory_lane.run_index_sync_after_commit(group_id=group_id)
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group_id,
            "candidate_id": candidate_id,
            "dry_run": False,
            "already_structured": False,
            "legacy_match": {
                "start_line": int(match.get("start_line") or 0),
                "end_line": int(match.get("end_line") or 0),
            },
            "candidate": updated,
            "memory_write": {
                "file_path": str(write_result.get("file_path") or ""),
                "status": str(write_result.get("status") or ""),
                "content_hash": str(write_result.get("content_hash") or ""),
                "entry_id": entry_id,
            },
            "asset_write": asset_write,
            "skill_write": skill_write,
            "index_sync": index_sync_result,
            "commit_state": str(index_sync_result.get("commit_state") or "disk_committed"),
        },
    )
