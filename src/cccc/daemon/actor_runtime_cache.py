from __future__ import annotations

import threading
from typing import Any, Dict

_LOCK = threading.Lock()
_BY_GROUP: Dict[str, Dict[str, Dict[str, Any]]] = {}


def replace_group_runtime(group_id: str, actors: Dict[str, Dict[str, Any]]) -> None:
    gid = str(group_id or "").strip()
    if not gid:
        return
    normalized: Dict[str, Dict[str, Any]] = {}
    for actor_id, payload in actors.items():
        aid = str(actor_id or "").strip()
        if not aid or not isinstance(payload, dict):
            continue
        normalized[aid] = {
            "running": bool(payload.get("running")),
            "runner_effective": str(payload.get("runner_effective") or "").strip() or "pty",
            "idle_seconds": payload.get("idle_seconds"),
            "effective_working_state": str(payload.get("effective_working_state") or "").strip() or "stopped",
            "effective_working_reason": str(payload.get("effective_working_reason") or "").strip(),
            "effective_working_updated_at": payload.get("effective_working_updated_at"),
            "effective_active_task_id": payload.get("effective_active_task_id"),
        }
    with _LOCK:
        _BY_GROUP[gid] = normalized


def get_group_runtime(group_id: str) -> Dict[str, Dict[str, Any]]:
    gid = str(group_id or "").strip()
    if not gid:
        return {}
    with _LOCK:
        current = _BY_GROUP.get(gid) or {}
        return {actor_id: dict(payload) for actor_id, payload in current.items()}
