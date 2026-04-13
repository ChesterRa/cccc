from __future__ import annotations

from typing import Any, Dict

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.actors import find_foreman
from ...kernel.group import load_group
from ...util.conv import coerce_bool
from ...util.time import utc_now_iso
from . import experience_assets, experience_memory_lane as memory_lane
from .experience_common import _RETIRED_STATUSES


def _error(code: str, message: str, *, details: Dict[str, Any] | None = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def handle_experience_promote_to_memory(args: Dict[str, Any]) -> DaemonResponse:
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

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    foreman = find_foreman(group)
    foreman_id = str((foreman or {}).get("id") or "").strip() if isinstance(foreman, dict) else ""
    if by != "user" and by != foreman_id:
        return _error(
            "permission_denied",
            "permission denied: only user or foreman can promote experience to memory",
            details={"by": by, "foreman_id": foreman_id},
        )

    candidates = memory_lane.load_experience_candidates(group)
    candidate_index = -1
    for idx, item in enumerate(candidates):
        if str(item.get("id") or "").strip() == candidate_id:
            candidate_index = idx
            break
    if candidate_index < 0:
        return _error("candidate_not_found", f"candidate not found: {candidate_id}")

    current = dict(candidates[candidate_index])
    current_status = str(current.get("status") or "").strip()
    if current_status in _RETIRED_STATUSES:
        return _error(
            "validation_error",
            f"candidate {candidate_id} is retired via {current_status} and cannot be promoted",
        )
    updated = dict(current)
    updated["status"] = "promoted_to_memory"
    updated["updated_at"] = utc_now_iso()
    promotion_meta = updated.get("promotion") if isinstance(updated.get("promotion"), dict) else {}
    promotion_meta["by"] = by
    promotion_meta["at"] = updated["updated_at"]
    promotion_meta["target"] = "memory"
    entry_id = memory_lane.experience_memory_entry_id(candidate_id)
    promotion_meta["memory_entry"] = {
        "entry_id": entry_id,
        "file_path": str(memory_lane.resolve_memory_layout(group_id, ensure_files=False).memory_file),
    }
    updated["promotion"] = promotion_meta

    memory_entry = memory_lane.build_experience_memory_entry(
        group_id=group_id,
        candidate=updated,
        actor_id=by,
        entry_id=entry_id,
        created_at=updated["updated_at"],
    )
    memory_write_result: Dict[str, Any] = {}
    asset_write_result: Dict[str, Any] = {"status": "skipped", "file_path": "", "asset": {}}
    skill_write_result: Dict[str, Any] = {"status": "skipped", "file_path": "", "asset": {}}
    index_sync_result: Dict[str, Any] = {"status": "skipped", "commit_state": "candidate_only"}

    if not dry_run:
        candidates_path = group.path / "state" / "experience_candidates.json"
        memory_plan = memory_lane.compute_memory_mutation_plan(
            group_id=group_id,
            mutations=[
                {
                    "action": "upsert",
                    "entry_id": entry_id,
                    "block": memory_lane.render_structured_memory_entry(
                        memory_entry,
                        idempotency_key=f"experience_promote:{candidate_id}",
                    ),
                }
            ],
        )
        try:
            write_result = memory_lane.write_memory_content(
                group_id=group_id,
                content=str(memory_plan.get("updated_text") or ""),
            )
            candidates[candidate_index] = updated
            memory_lane.persist_candidates(candidates_path, candidates)
        except Exception as exc:
            try:
                memory_lane.write_memory_content(group_id=group_id, content=str(memory_plan.get("current_text") or ""))
            except Exception:
                pass
            return _error("memory_sync_error", str(exc))
        index_sync_result = memory_lane.run_index_sync_after_commit(group_id=group_id)
        promotion_meta["memory_entry"]["file_path"] = str(write_result.get("file_path") or "")
        updated["promotion"] = promotion_meta
        candidates[candidate_index] = updated
        memory_lane.persist_candidates(candidates_path, candidates)
        try:
            asset_write_result = experience_assets.write_experience_asset_mirror(
                group,
                candidate=updated,
                memory_entry=promotion_meta.get("memory_entry") if isinstance(promotion_meta.get("memory_entry"), dict) else {},
            )
        except Exception as exc:
            asset_write_result = {"status": "error", "file_path": "", "asset": {}, "error": str(exc)}
        try:
            skill_write_result = experience_assets.sync_procedural_skill_mirror(
                group=group,
                candidate=updated,
                memory_entry=promotion_meta.get("memory_entry") if isinstance(promotion_meta.get("memory_entry"), dict) else {},
            )
        except Exception as exc:
            skill_write_result = {"status": "error", "file_path": "", "asset": {}, "error": str(exc)}
        memory_write_result = {
            "file_path": str(write_result.get("file_path") or ""),
            "status": str(write_result.get("status") or ""),
            "content_hash": str(write_result.get("content_hash") or ""),
            "entry_id": entry_id,
        }
    else:
        layout = memory_lane.resolve_memory_layout(group_id, ensure_files=False)
        memory_write_result = {
            "file_path": str(layout.memory_file),
            "status": "dry_run",
            "content_hash": "",
            "entry_id": entry_id,
        }
        asset_write_result = {
            "status": "dry_run",
            "file_path": str(group.path / "state" / "experience_assets" / f"{candidate_id}.json"),
            "asset": {},
        }
        skill_write_result = {
            "status": "dry_run",
            "file_path": str(group.path / "state" / "procedural_skills" / f"procskill_{candidate_id}.json"),
            "asset": {},
        }
        index_sync_result = {"status": "skipped", "commit_state": "dry_run"}

    return DaemonResponse(
        ok=True,
        result={
            "group_id": group_id,
            "candidate_id": candidate_id,
            "dry_run": dry_run,
            "candidate": updated,
            "memory_write": memory_write_result,
            "asset_write": asset_write_result,
            "skill_write": skill_write_result,
            "index_sync": index_sync_result,
            "commit_state": str(index_sync_result.get("commit_state") or "candidate_only"),
            "memory_entry_preview": memory_lane.render_structured_memory_entry(
                memory_entry,
                idempotency_key=f"experience_promote:{candidate_id}",
            ),
        },
    )


def handle_experience_runtime_prompt_delivery(args: Dict[str, Any]) -> DaemonResponse:
    group_id = str(args.get("group_id") or "").strip()
    candidate_id = str(args.get("candidate_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")
    if not candidate_id:
        return _error("validation_error", "candidate_id is required")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    selected_assets = experience_assets.select_experience_assets_for_consumption(group, limit=3)
    runtime_prompt_delivery = experience_assets.refresh_runtime_prompt_consumption(
        group=group,
        candidate_id=candidate_id,
        selected_assets=selected_assets,
    )
    return DaemonResponse(
        ok=True,
        result={
            "group_id": group_id,
            "candidate_id": candidate_id,
            "runtime_prompt_delivery": runtime_prompt_delivery,
        },
    )
