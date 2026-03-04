"""Document templates, normalization, and I/O for capability state/catalog/runtime."""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from ....util.fs import atomic_write_json, read_json
from ....util.time import parse_utc_iso, utc_now_iso

from ._common import (
    _SOURCE_IDS,
    _STATE_LOCK,
    _CATALOG_LOCK,
    _RUNTIME_LOCK,
    _state_path,
    _catalog_path,
    _runtime_path,
)

def _source_state_template(sync_state: str = "never") -> Dict[str, Any]:
    return {
        "sync_state": sync_state,
        "last_synced_at": "",
        "staleness_seconds": None,
        "error": "",
        "record_count": 0,
        "next_cursor": "",
        "updated_since": "",
    }


def _new_state_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 1,
        "created_at": now,
        "updated_at": now,
        "group_enabled": {},
        "actor_enabled": {},
        "session_enabled": {},
        "global_blocked": {},
        "group_blocked": {},
    }


def _new_catalog_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 1,
        "created_at": now,
        "updated_at": now,
        "sources": {
            "manual_import": _source_state_template("never"),
            "mcp_registry_official": _source_state_template("never"),
            "anthropic_skills": _source_state_template("never"),
            "github_skills_curated": _source_state_template("never"),
            "skillsmp_remote": _source_state_template("never"),
            "clawhub_remote": _source_state_template("never"),
            "openclaw_skills_remote": _source_state_template("never"),
            "clawskills_remote": _source_state_template("never"),
        },
        "records": {},
    }


def _new_runtime_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 2,
        "created_at": now,
        "updated_at": now,
        "artifacts": {},
        "capability_artifacts": {},
        "actor_instances": {},
        "recent_success": {},
    }


def _normalize_state_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return _new_state_doc()
    doc = dict(raw)
    now = utc_now_iso()
    doc["v"] = 1
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = now
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = now

    group_enabled_raw = doc.get("group_enabled")
    group_enabled: Dict[str, List[str]] = {}
    if isinstance(group_enabled_raw, dict):
        for group_id, items in group_enabled_raw.items():
            gid = str(group_id or "").strip()
            if not gid or not isinstance(items, list):
                continue
            clean = sorted({str(x or "").strip() for x in items if str(x or "").strip()})
            if clean:
                group_enabled[gid] = clean
    doc["group_enabled"] = group_enabled

    actor_enabled_raw = doc.get("actor_enabled")
    actor_enabled: Dict[str, Dict[str, List[str]]] = {}
    if isinstance(actor_enabled_raw, dict):
        for group_id, per_group in actor_enabled_raw.items():
            gid = str(group_id or "").strip()
            if not gid or not isinstance(per_group, dict):
                continue
            clean_group: Dict[str, List[str]] = {}
            for actor_id, items in per_group.items():
                aid = str(actor_id or "").strip()
                if not aid or not isinstance(items, list):
                    continue
                clean = sorted({str(x or "").strip() for x in items if str(x or "").strip()})
                if clean:
                    clean_group[aid] = clean
            if clean_group:
                actor_enabled[gid] = clean_group
    doc["actor_enabled"] = actor_enabled

    session_enabled_raw = doc.get("session_enabled")
    session_enabled: Dict[str, Dict[str, List[Dict[str, str]]]] = {}
    if isinstance(session_enabled_raw, dict):
        for group_id, per_group in session_enabled_raw.items():
            gid = str(group_id or "").strip()
            if not gid or not isinstance(per_group, dict):
                continue
            clean_group: Dict[str, List[Dict[str, str]]] = {}
            for actor_id, entries in per_group.items():
                aid = str(actor_id or "").strip()
                if not aid or not isinstance(entries, list):
                    continue
                clean_entries: List[Dict[str, str]] = []
                for item in entries:
                    if not isinstance(item, dict):
                        continue
                    cap_id = str(item.get("capability_id") or "").strip()
                    expires_at = str(item.get("expires_at") or "").strip()
                    if not cap_id or not expires_at:
                        continue
                    if parse_utc_iso(expires_at) is None:
                        continue
                    clean_entries.append({"capability_id": cap_id, "expires_at": expires_at})
                if clean_entries:
                    clean_group[aid] = clean_entries
            if clean_group:
                session_enabled[gid] = clean_group
    doc["session_enabled"] = session_enabled

    def _normalize_block_entry(item: Any) -> Optional[Dict[str, str]]:
        if not isinstance(item, dict):
            return None
        blocked_at = str(item.get("blocked_at") or "").strip()
        if parse_utc_iso(blocked_at) is None:
            blocked_at = now
        expires_at = str(item.get("expires_at") or "").strip()
        if expires_at and parse_utc_iso(expires_at) is None:
            expires_at = ""
        return {
            "reason": str(item.get("reason") or "").strip(),
            "by": str(item.get("by") or "").strip(),
            "blocked_at": blocked_at,
            "expires_at": expires_at,
        }

    global_blocked_raw = doc.get("global_blocked")
    global_blocked: Dict[str, Dict[str, str]] = {}
    if isinstance(global_blocked_raw, dict):
        for capability_id, entry in global_blocked_raw.items():
            cap_id = str(capability_id or "").strip()
            if not cap_id:
                continue
            normalized = _normalize_block_entry(entry)
            if normalized is None:
                continue
            global_blocked[cap_id] = normalized
    doc["global_blocked"] = global_blocked

    group_blocked_raw = doc.get("group_blocked")
    group_blocked: Dict[str, Dict[str, Dict[str, str]]] = {}
    if isinstance(group_blocked_raw, dict):
        for group_id, per_group in group_blocked_raw.items():
            gid = str(group_id or "").strip()
            if not gid or not isinstance(per_group, dict):
                continue
            clean_group: Dict[str, Dict[str, str]] = {}
            for capability_id, entry in per_group.items():
                cap_id = str(capability_id or "").strip()
                if not cap_id:
                    continue
                normalized = _normalize_block_entry(entry)
                if normalized is None:
                    continue
                clean_group[cap_id] = normalized
            if clean_group:
                group_blocked[gid] = clean_group
    doc["group_blocked"] = group_blocked
    return doc


def _normalize_catalog_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return _new_catalog_doc()
    doc = dict(raw)
    now = utc_now_iso()
    doc["v"] = 1
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = now
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = now

    sources_raw = doc.get("sources")
    sources = sources_raw if isinstance(sources_raw, dict) else {}
    normalized_sources: Dict[str, Dict[str, Any]] = {}
    for source_id in _SOURCE_IDS:
        item = sources.get(source_id)
        normalized = _source_state_template("never")
        if isinstance(item, dict):
            normalized.update(
                {
                    "sync_state": str(item.get("sync_state") or "never"),
                    "last_synced_at": str(item.get("last_synced_at") or ""),
                    "staleness_seconds": item.get("staleness_seconds"),
                    "error": str(item.get("error") or ""),
                    "record_count": int(item.get("record_count") or 0),
                    "next_cursor": str(item.get("next_cursor") or ""),
                    "updated_since": str(item.get("updated_since") or ""),
                }
            )
        normalized_sources[source_id] = normalized
    doc["sources"] = normalized_sources

    records_raw = doc.get("records")
    records: Dict[str, Dict[str, Any]] = {}
    if isinstance(records_raw, dict):
        for capability_id, item in records_raw.items():
            cap_id = str(capability_id or "").strip()
            if not cap_id or not isinstance(item, dict):
                continue
            candidate = dict(item)
            candidate["capability_id"] = cap_id
            records[cap_id] = candidate
    doc["records"] = records
    return doc


def _normalize_runtime_doc(raw: Any) -> Dict[str, Any]:
    if not isinstance(raw, dict):
        return _new_runtime_doc()
    doc = dict(raw)
    now = utc_now_iso()
    doc["v"] = 2
    if not str(doc.get("created_at") or "").strip():
        doc["created_at"] = now
    if not str(doc.get("updated_at") or "").strip():
        doc["updated_at"] = now
    artifacts_raw = doc.get("artifacts")
    artifacts: Dict[str, Dict[str, Any]] = {}
    if isinstance(artifacts_raw, dict):
        for artifact_id, item in artifacts_raw.items():
            aid = str(artifact_id or "").strip()
            if not aid or not isinstance(item, dict):
                continue
            tools_raw = item.get("tools")
            tools: List[Dict[str, Any]] = []
            if isinstance(tools_raw, list):
                for tool in tools_raw:
                    if not isinstance(tool, dict):
                        continue
                    name = str(tool.get("name") or "").strip()
                    real_name = str(tool.get("real_tool_name") or "").strip()
                    if not name or not real_name:
                        continue
                    schema = tool.get("inputSchema")
                    if not isinstance(schema, dict):
                        schema = {"type": "object", "properties": {}, "required": []}
                    tools.append(
                        {
                            "name": name,
                            "real_tool_name": real_name,
                            "description": str(tool.get("description") or "").strip(),
                            "inputSchema": schema,
                        }
                    )
            cap_ids_raw = item.get("capability_ids")
            cap_ids: List[str] = []
            if isinstance(cap_ids_raw, list):
                seen: set[str] = set()
                for x in cap_ids_raw:
                    cid = str(x or "").strip()
                    if not cid or cid in seen:
                        continue
                    seen.add(cid)
                    cap_ids.append(cid)
            artifacts[aid] = {
                "artifact_id": aid,
                "install_key": str(item.get("install_key") or "").strip() or aid,
                "state": str(item.get("state") or "").strip() or "unknown",
                "installer": str(item.get("installer") or "").strip(),
                "install_mode": str(item.get("install_mode") or "").strip(),
                "invoker": dict(item.get("invoker")) if isinstance(item.get("invoker"), dict) else {},
                "tools": tools,
                "last_error": str(item.get("last_error") or "").strip(),
                "last_error_code": str(item.get("last_error_code") or "").strip(),
                "updated_at": str(item.get("updated_at") or "").strip() or now,
                "capability_ids": cap_ids,
            }

    capability_artifacts_raw = doc.get("capability_artifacts")
    capability_artifacts: Dict[str, str] = {}
    if isinstance(capability_artifacts_raw, dict):
        for capability_id, artifact_id in capability_artifacts_raw.items():
            cid = str(capability_id or "").strip()
            aid = str(artifact_id or "").strip()
            if not cid or not aid:
                continue
            if aid in artifacts:
                capability_artifacts[cid] = aid

    # Legacy migration path: installations -> artifacts/capability_artifacts.
    installs_raw = doc.get("installations")
    if isinstance(installs_raw, dict):
        for capability_id, item in installs_raw.items():
            cid = str(capability_id or "").strip()
            if not cid or not isinstance(item, dict):
                continue
            aid = str(capability_artifacts.get(cid) or "").strip()
            if not aid:
                aid = f"art_{hashlib.sha1(f'legacy:{cid}'.encode('utf-8')).hexdigest()[:16]}"
            existing = artifacts.get(aid) if isinstance(artifacts.get(aid), dict) else None
            tools_raw = item.get("tools")
            tools: List[Dict[str, Any]] = []
            if isinstance(tools_raw, list):
                for tool in tools_raw:
                    if not isinstance(tool, dict):
                        continue
                    name = str(tool.get("name") or "").strip()
                    real_name = str(tool.get("real_tool_name") or "").strip()
                    if not name or not real_name:
                        continue
                    schema = tool.get("inputSchema")
                    if not isinstance(schema, dict):
                        schema = {"type": "object", "properties": {}, "required": []}
                    tools.append(
                        {
                            "name": name,
                            "real_tool_name": real_name,
                            "description": str(tool.get("description") or "").strip(),
                            "inputSchema": schema,
                        }
                    )
            merged_caps = (
                list(existing.get("capability_ids") or []) if isinstance(existing, dict) else []
            )
            if cid not in merged_caps:
                merged_caps.append(cid)
            artifacts[aid] = {
                "artifact_id": aid,
                "install_key": str((existing or {}).get("install_key") or aid).strip() or aid,
                "state": str(item.get("state") or (existing or {}).get("state") or "").strip() or "unknown",
                "installer": str(item.get("installer") or (existing or {}).get("installer") or "").strip(),
                "install_mode": str(item.get("install_mode") or (existing or {}).get("install_mode") or "").strip(),
                "invoker": dict(item.get("invoker")) if isinstance(item.get("invoker"), dict) else (
                    dict((existing or {}).get("invoker")) if isinstance((existing or {}).get("invoker"), dict) else {}
                ),
                "tools": tools or (list((existing or {}).get("tools") or [])),
                "last_error": str(item.get("last_error") or (existing or {}).get("last_error") or "").strip(),
                "last_error_code": str(
                    item.get("last_error_code") or (existing or {}).get("last_error_code") or ""
                ).strip(),
                "updated_at": str(item.get("updated_at") or (existing or {}).get("updated_at") or "").strip() or now,
                "capability_ids": merged_caps,
            }
            capability_artifacts[cid] = aid

    # Ensure reverse references are consistent.
    for cid, aid in list(capability_artifacts.items()):
        art = artifacts.get(aid)
        if not isinstance(art, dict):
            capability_artifacts.pop(cid, None)
            continue
        cap_ids = art.get("capability_ids") if isinstance(art.get("capability_ids"), list) else []
        if cid not in cap_ids:
            cap_ids.append(cid)
            art["capability_ids"] = cap_ids
            artifacts[aid] = art

    actor_instances_raw = doc.get("actor_instances")
    if not isinstance(actor_instances_raw, dict):
        actor_instances_raw = doc.get("actor_bindings")
    actor_instances: Dict[str, Dict[str, Dict[str, Dict[str, Any]]]] = {}
    if isinstance(actor_instances_raw, dict):
        for group_id, per_group in actor_instances_raw.items():
            gid = str(group_id or "").strip()
            if not gid or not isinstance(per_group, dict):
                continue
            clean_group: Dict[str, Dict[str, Dict[str, Any]]] = {}
            for actor_id, per_actor in per_group.items():
                aid = str(actor_id or "").strip()
                if not aid or not isinstance(per_actor, dict):
                    continue
                clean_actor: Dict[str, Dict[str, Any]] = {}
                for capability_id, item in per_actor.items():
                    cid = str(capability_id or "").strip()
                    if not cid or not isinstance(item, dict):
                        continue
                    artifact_id = str(item.get("artifact_id") or capability_artifacts.get(cid) or "").strip()
                    if artifact_id and artifact_id not in artifacts:
                        artifact_id = ""
                    clean_actor[cid] = {
                        "artifact_id": artifact_id,
                        "state": str(item.get("state") or "").strip() or "unknown",
                        "last_error": str(item.get("last_error") or "").strip(),
                        "updated_at": str(item.get("updated_at") or "").strip() or now,
                    }
                if clean_actor:
                    clean_group[aid] = clean_actor
            if clean_group:
                actor_instances[gid] = clean_group

    recent_success_raw = doc.get("recent_success")
    recent_success: Dict[str, Dict[str, Any]] = {}
    if isinstance(recent_success_raw, dict):
        for capability_id, row in recent_success_raw.items():
            cid = str(capability_id or "").strip()
            if not cid or not isinstance(row, dict):
                continue
            count = int(row.get("success_count") or 0)
            if count <= 0:
                continue
            recent_success[cid] = {
                "capability_id": cid,
                "success_count": count,
                "last_success_at": str(row.get("last_success_at") or "").strip() or now,
                "last_group_id": str(row.get("last_group_id") or "").strip(),
                "last_actor_id": str(row.get("last_actor_id") or "").strip(),
                "last_action": str(row.get("last_action") or "").strip(),
            }

    doc["artifacts"] = artifacts
    doc["capability_artifacts"] = capability_artifacts
    doc["actor_instances"] = actor_instances
    doc["recent_success"] = recent_success
    return doc


def _load_state_doc() -> Tuple[Path, Dict[str, Any]]:
    path = _state_path()
    return path, _normalize_state_doc(read_json(path))


def _save_state_doc(path: Path, doc: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(path, doc, indent=2)


def _load_catalog_doc() -> Tuple[Path, Dict[str, Any]]:
    path = _catalog_path()
    return path, _normalize_catalog_doc(read_json(path))


def _save_catalog_doc(path: Path, doc: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(path, doc, indent=2)


def _load_runtime_doc() -> Tuple[Path, Dict[str, Any]]:
    path = _runtime_path()
    return path, _normalize_runtime_doc(read_json(path))


def _save_runtime_doc(path: Path, doc: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    doc["updated_at"] = utc_now_iso()
    atomic_write_json(path, doc, indent=2)

