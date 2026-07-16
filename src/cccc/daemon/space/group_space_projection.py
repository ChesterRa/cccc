from __future__ import annotations

from typing import Any, Dict

from ...util.fs import atomic_write_json, read_json
from ...util.time import utc_now_iso
from .group_space_memory_sync import summarize_memory_notebooklm_sync
from .group_space_paths import (
    resolve_space_root,
    space_state_path,
    space_status_path,
)
from .group_space_store import (
    get_space_bindings,
    get_space_provider_state,
    list_space_jobs,
    space_queue_summary,
    space_queue_summaries,
)


def _neutral_work_sync_state(
    *,
    sync_state: Dict[str, Any],
    group_id: str,
    provider: str,
    remote_space_id: str,
    reason: str,
) -> Dict[str, Any]:
    state = dict(sync_state) if isinstance(sync_state, dict) else {}
    return {
        "state": "ok",
        "run_id": str(state.get("run_id") or ""),
        "last_run_at": "",
        "converged": False,
        "unsynced_count": 0,
        "failed_count": 0,
        "failed_items": [],
        "last_error": "",
        "reason": str(reason or "").strip(),
        "group_id": str(group_id or ""),
        "provider": str(provider or "notebooklm"),
        "remote_space_id": str(remote_space_id or ""),
        "available": bool(state.get("available")),
    }


def _live_work_queue_summary(*, group_id: str, provider: str, binding: Dict[str, Any]) -> Dict[str, Any]:
    binding_status = str(binding.get("status") or "").strip().lower()
    bound_remote_id = str(binding.get("remote_space_id") or "").strip()
    if binding_status != "bound" or not bound_remote_id:
        return {"pending": 0, "running": 0, "failed": 0}
    return space_queue_summary(
        group_id=group_id,
        provider=provider,
        lane="work",
        remote_space_id=bound_remote_id,
    )


def sync_group_space_projection(group_id: str, *, provider: str = "notebooklm") -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    if not gid:
        return {"written": False, "reason": "missing_group_id"}
    space_root = resolve_space_root(gid, create=True)
    if space_root is None:
        return {"written": False, "reason": "no_local_scope"}

    provider_id = str(provider or "notebooklm").strip() or "notebooklm"
    bindings = get_space_bindings(gid, provider=provider_id)
    provider_state = get_space_provider_state(provider_id)
    queue = space_queue_summaries(group_id=gid, provider=provider_id)
    work_binding = bindings.get("work") if isinstance(bindings.get("work"), dict) else {}
    work_queue = _live_work_queue_summary(group_id=gid, provider=provider_id, binding=work_binding)
    queue["work"] = work_queue

    binding_status = str(work_binding.get("status") or "").strip().lower()
    bound_remote_id = str(work_binding.get("remote_space_id") or "").strip()
    jobs = list_space_jobs(
        group_id=gid,
        provider=provider_id,
        lane="work",
        state="",
        remote_space_id=(bound_remote_id if binding_status == "bound" and bound_remote_id else ""),
        limit=20,
    )
    latest_context_sync: Dict[str, Any] = {}
    for item in jobs:
        if not isinstance(item, dict):
            continue
        if str(item.get("kind") or "").strip() != "context_sync":
            continue
        latest_context_sync = {
            "job_id": str(item.get("job_id") or ""),
            "state": str(item.get("state") or ""),
            "updated_at": str(item.get("updated_at") or ""),
            "last_error": item.get("last_error") if isinstance(item.get("last_error"), dict) else {},
        }
        break

    sync_state_raw = read_json(space_state_path(space_root))
    sync_state: Dict[str, Any] = sync_state_raw if isinstance(sync_state_raw, dict) else {}
    if binding_status != "bound" or not bound_remote_id:
        work_sync = _neutral_work_sync_state(
            sync_state=sync_state,
            group_id=gid,
            provider=provider_id,
            remote_space_id="",
            reason="work_lane_unbound",
        )
        latest_context_sync = {}
    else:
        sync_remote_id = str(sync_state.get("remote_space_id") or "").strip()
        if sync_remote_id != bound_remote_id:
            work_sync = _neutral_work_sync_state(
                sync_state=sync_state,
                group_id=gid,
                provider=provider_id,
                remote_space_id=bound_remote_id,
                reason=("sync_state_not_ready" if not sync_remote_id else "binding_remote_mismatch"),
            )
        else:
            failed_items_raw = sync_state.get("failed_items")
            failed_items: list[Dict[str, Any]] = []
            if isinstance(failed_items_raw, list):
                for item in failed_items_raw:
                    if not isinstance(item, dict):
                        continue
                    failed_items.append(
                        {
                            "rel_path": str(item.get("rel_path") or "").strip(),
                            "code": str(item.get("code") or "").strip(),
                            "message": str(item.get("message") or "").strip(),
                        }
                    )
                    if len(failed_items) >= 20:
                        break
            work_sync = {
                "state": str(
                    sync_state.get("state") or ("error" if int(sync_state.get("unsynced_count") or 0) > 0 else "ok")
                ),
                "run_id": str(sync_state.get("run_id") or ""),
                "last_run_at": str(sync_state.get("last_run_at") or ""),
                "converged": bool(sync_state.get("converged")),
                "unsynced_count": int(sync_state.get("unsynced_count") or 0),
                "failed_count": int(sync_state.get("failed_count") or len(failed_items)),
                "failed_items": failed_items,
                "last_error": str(sync_state.get("last_error") or ""),
            }
    memory_binding = bindings.get("memory") if isinstance(bindings.get("memory"), dict) else {}
    memory_sync = summarize_memory_notebooklm_sync(
        gid,
        remote_space_id=str(memory_binding.get("remote_space_id") or ""),
    )

    doc = {
        "v": 3,
        "generated_at": utc_now_iso(),
        "group_id": gid,
        "provider": provider_id,
        "space_root": str(space_root),
        "provider_state": provider_state,
        "bindings": bindings,
        "queue_summary": queue,
        "latest_context_sync": latest_context_sync,
        "sync": work_sync,
        "memory_sync": memory_sync,
    }
    out_path = space_status_path(space_root)
    atomic_write_json(out_path, doc, indent=2)
    return {"written": True, "path": str(out_path)}
