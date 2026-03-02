"""Runtime artifact/binding management and audit logging for capability_ops."""

from __future__ import annotations

import json
import os
import uuid
from typing import Any, Dict, List, Optional, Tuple

from ....util.time import utc_now_iso

from ._common import _RUNTIME_LOCK, _AUDIT_LOCK, _runtime_path, _audit_path, _quota_limit
from ._documents import _load_runtime_doc, _save_runtime_doc

def _runtime_artifacts(runtime_doc: Dict[str, Any]) -> Dict[str, Dict[str, Any]]:
    raw = runtime_doc.get("artifacts")
    return raw if isinstance(raw, dict) else {}

def _runtime_capability_artifacts(runtime_doc: Dict[str, Any]) -> Dict[str, str]:
    raw = runtime_doc.get("capability_artifacts")
    return raw if isinstance(raw, dict) else {}

def _runtime_actor_bindings(runtime_doc: Dict[str, Any]) -> Dict[str, Dict[str, Dict[str, Dict[str, Any]]]]:
    raw = runtime_doc.get("actor_instances")
    return raw if isinstance(raw, dict) else {}

def _runtime_recent_success(runtime_doc: Dict[str, Any]) -> Dict[str, Dict[str, Any]]:
    raw = runtime_doc.get("recent_success")
    return raw if isinstance(raw, dict) else {}

def _record_runtime_recent_success(
    runtime_doc: Dict[str, Any],
    *,
    capability_id: str,
    group_id: str = "",
    actor_id: str = "",
    action: str = "",
) -> None:
    cap_id = str(capability_id or "").strip()
    if not cap_id:
        return
    now_iso = utc_now_iso()
    mapping = _runtime_recent_success(runtime_doc)
    prior = mapping.get(cap_id) if isinstance(mapping.get(cap_id), dict) else {}
    count = max(0, int(prior.get("success_count") or 0)) + 1
    row: Dict[str, Any] = {
        "capability_id": cap_id,
        "success_count": count,
        "last_success_at": now_iso,
        "last_group_id": str(group_id or prior.get("last_group_id") or "").strip(),
        "last_actor_id": str(actor_id or prior.get("last_actor_id") or "").strip(),
        "last_action": str(action or prior.get("last_action") or "").strip(),
    }
    mapping[cap_id] = row
    keep_limit = _quota_limit("CCCC_CAPABILITY_RECENT_SUCCESS_LIMIT", 512, minimum=50, maximum=5000)
    if len(mapping) > keep_limit:
        ranked = sorted(
            (
                (
                    str(v.get("last_success_at") or ""),
                    str(k),
                )
                for k, v in mapping.items()
                if isinstance(v, dict)
            )
        )
        overflow = max(0, len(mapping) - keep_limit)
        for _, key in ranked[:overflow]:
            mapping.pop(str(key), None)
    runtime_doc["recent_success"] = mapping

def _set_runtime_capability_artifact(
    runtime_doc: Dict[str, Any],
    *,
    capability_id: str,
    artifact_id: str,
) -> None:
    cid = str(capability_id or "").strip()
    aid = str(artifact_id or "").strip()
    if not cid:
        return
    mapping = _runtime_capability_artifacts(runtime_doc)
    if aid:
        mapping[cid] = aid
    else:
        mapping.pop(cid, None)
    runtime_doc["capability_artifacts"] = mapping

def _runtime_install_for_capability(
    runtime_doc: Dict[str, Any],
    *,
    capability_id: str,
) -> Tuple[str, Optional[Dict[str, Any]]]:
    cid = str(capability_id or "").strip()
    if not cid:
        return "", None
    mapping = _runtime_capability_artifacts(runtime_doc)
    artifacts = _runtime_artifacts(runtime_doc)
    aid = str(mapping.get(cid) or "").strip()
    if not aid:
        return "", None
    art = artifacts.get(aid) if isinstance(artifacts.get(aid), dict) else None
    return aid, (dict(art) if isinstance(art, dict) else None)

def _remove_runtime_capability_artifact(
    runtime_doc: Dict[str, Any],
    *,
    capability_id: str,
) -> str:
    cid = str(capability_id or "").strip()
    if not cid:
        return ""
    mapping = _runtime_capability_artifacts(runtime_doc)
    aid = str(mapping.pop(cid, "") or "").strip()
    runtime_doc["capability_artifacts"] = mapping
    if aid:
        artifacts = _runtime_artifacts(runtime_doc)
        row = artifacts.get(aid) if isinstance(artifacts.get(aid), dict) else None
        if isinstance(row, dict):
            caps_raw = row.get("capability_ids")
            caps = [str(x).strip() for x in caps_raw if str(x).strip()] if isinstance(caps_raw, list) else []
            caps = [x for x in caps if x != cid]
            row["capability_ids"] = caps
            artifacts[aid] = row
            runtime_doc["artifacts"] = artifacts
    return aid

def _remove_runtime_artifact_if_unreferenced(
    runtime_doc: Dict[str, Any],
    *,
    artifact_id: str,
) -> bool:
    aid = str(artifact_id or "").strip()
    if not aid:
        return False
    mapping = _runtime_capability_artifacts(runtime_doc)
    for mapped in mapping.values():
        if str(mapped or "").strip() == aid:
            return False
    artifacts = _runtime_artifacts(runtime_doc)
    if aid in artifacts:
        artifacts.pop(aid, None)
        runtime_doc["artifacts"] = artifacts
        return True
    return False

def _set_runtime_actor_binding(
    runtime_doc: Dict[str, Any],
    *,
    group_id: str,
    actor_id: str,
    capability_id: str,
    artifact_id: str = "",
    state: str,
    last_error: str = "",
) -> None:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    cid = str(capability_id or "").strip()
    if not gid or not aid or not cid:
        return
    bindings = _runtime_actor_bindings(runtime_doc)
    per_group = bindings.setdefault(gid, {})
    per_actor = per_group.setdefault(aid, {})
    per_actor[cid] = {
        "artifact_id": str(artifact_id or "").strip(),
        "state": str(state or "").strip() or "unknown",
        "last_error": str(last_error or "").strip(),
        "updated_at": utc_now_iso(),
    }
    runtime_doc["actor_instances"] = bindings

def _remove_runtime_actor_binding(
    runtime_doc: Dict[str, Any],
    *,
    group_id: str,
    actor_id: str,
    capability_id: str,
) -> int:
    gid = str(group_id or "").strip()
    aid = str(actor_id or "").strip()
    cid = str(capability_id or "").strip()
    if not gid or not aid or not cid:
        return 0
    bindings = _runtime_actor_bindings(runtime_doc)
    per_group = bindings.get(gid) if isinstance(bindings.get(gid), dict) else {}
    per_actor = per_group.get(aid) if isinstance(per_group.get(aid), dict) else {}
    if cid not in per_actor:
        return 0
    per_actor.pop(cid, None)
    if per_actor:
        per_group[aid] = per_actor
    else:
        per_group.pop(aid, None)
    if per_group:
        bindings[gid] = per_group
    else:
        bindings.pop(gid, None)
    runtime_doc["actor_instances"] = bindings
    return 1

def _remove_runtime_group_capability_bindings(
    runtime_doc: Dict[str, Any],
    *,
    group_id: str,
    capability_id: str,
) -> int:
    gid = str(group_id or "").strip()
    cid = str(capability_id or "").strip()
    if not gid or not cid:
        return 0
    bindings = _runtime_actor_bindings(runtime_doc)
    per_group = bindings.get(gid) if isinstance(bindings.get(gid), dict) else {}
    removed = 0
    actor_ids = list(per_group.keys()) if isinstance(per_group, dict) else []
    for aid in actor_ids:
        per_actor = per_group.get(aid) if isinstance(per_group.get(aid), dict) else {}
        if cid in per_actor:
            per_actor.pop(cid, None)
            removed += 1
        if per_actor:
            per_group[aid] = per_actor
        else:
            per_group.pop(aid, None)
    if per_group:
        bindings[gid] = per_group
    else:
        bindings.pop(gid, None)
    runtime_doc["actor_instances"] = bindings
    return removed

def _remove_runtime_capability_bindings_all_groups(
    runtime_doc: Dict[str, Any],
    *,
    capability_id: str,
) -> int:
    cid = str(capability_id or "").strip()
    if not cid:
        return 0
    bindings = _runtime_actor_bindings(runtime_doc)
    removed = 0
    group_ids = list(bindings.keys()) if isinstance(bindings, dict) else []
    for gid in group_ids:
        per_group = bindings.get(gid) if isinstance(bindings.get(gid), dict) else {}
        actor_ids = list(per_group.keys()) if isinstance(per_group, dict) else []
        for aid in actor_ids:
            per_actor = per_group.get(aid) if isinstance(per_group.get(aid), dict) else {}
            if cid in per_actor:
                per_actor.pop(cid, None)
                removed += 1
            if per_actor:
                per_group[aid] = per_actor
            else:
                per_group.pop(aid, None)
        if per_group:
            bindings[gid] = per_group
        else:
            bindings.pop(gid, None)
    runtime_doc["actor_instances"] = bindings
    return removed

def _append_audit_event(
    *,
    action_id: str,
    op: str,
    group_id: str,
    actor_id: str,
    by: str,
    capability_id: str,
    scope: str,
    enabled: bool,
    outcome: str,
    details: Optional[Dict[str, Any]] = None,
) -> None:
    event = {
        "v": 1,
        "event_id": f"cae_{uuid.uuid4().hex}",
        "at": utc_now_iso(),
        "action_id": str(action_id or "").strip(),
        "op": str(op or "").strip(),
        "group_id": str(group_id or "").strip(),
        "actor_id": str(actor_id or "").strip(),
        "by": str(by or "").strip(),
        "capability_id": str(capability_id or "").strip(),
        "scope": str(scope or "").strip(),
        "enabled": bool(enabled),
        "outcome": str(outcome or "").strip(),
        "details": details if isinstance(details, dict) else {},
    }
    line = json.dumps(event, ensure_ascii=False, default=str)
    with _AUDIT_LOCK:
        path = _audit_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("a", encoding="utf-8") as f:
            f.write(line + "\n")

