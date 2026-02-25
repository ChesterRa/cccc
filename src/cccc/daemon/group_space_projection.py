from __future__ import annotations

from typing import Any, Dict

from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso
from .group_space_paths import (
    resolve_space_root,
    space_state_path,
    space_status_path,
)
from .group_space_store import (
    get_space_binding,
    get_space_provider_state,
    list_space_jobs,
    space_queue_summary,
)


def sync_group_space_projection(group_id: str, *, provider: str = "notebooklm") -> Dict[str, Any]:
    gid = str(group_id or "").strip()
    if not gid:
        return {"written": False, "reason": "missing_group_id"}
    space_root = resolve_space_root(gid, create=True)
    if space_root is None:
        return {"written": False, "reason": "no_local_scope"}

    provider_id = str(provider or "notebooklm").strip() or "notebooklm"
    binding = get_space_binding(gid, provider=provider_id) or {}
    provider_state = get_space_provider_state(provider_id)
    queue = space_queue_summary(group_id=gid, provider=provider_id)
    jobs = list_space_jobs(group_id=gid, provider=provider_id, state="", limit=20)
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
    sync_view = {
        "state": str(sync_state.get("state") or ("error" if int(sync_state.get("unsynced_count") or 0) > 0 else "ok")),
        "run_id": str(sync_state.get("run_id") or ""),
        "last_run_at": str(sync_state.get("last_run_at") or ""),
        "converged": bool(sync_state.get("converged")),
        "unsynced_count": int(sync_state.get("unsynced_count") or 0),
        "failed_count": int(sync_state.get("failed_count") or len(failed_items)),
        "failed_items": failed_items,
        "last_error": str(sync_state.get("last_error") or ""),
    }

    doc = {
        "v": 1,
        "generated_at": utc_now_iso(),
        "group_id": gid,
        "provider": provider_id,
        "space_root": str(space_root),
        "provider_state": provider_state,
        "binding": binding,
        "queue_summary": queue,
        "latest_context_sync": latest_context_sync,
        "sync": sync_view,
        "sync_state": sync_view,
    }
    out_path = space_status_path(space_root)
    atomic_write_json(out_path, doc, indent=2)
    return {"written": True, "path": str(out_path)}
