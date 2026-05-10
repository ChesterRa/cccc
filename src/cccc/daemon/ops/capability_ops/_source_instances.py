"""Capability source instance grouping and matching."""

from __future__ import annotations

from typing import Any, Dict, Iterable, List
from urllib.parse import quote, urlparse


def _github_ref_from_record(source_uri: str, source_record_id: str) -> str:
    parsed = urlparse(source_uri)
    parts = [part for part in parsed.path.strip("/").split("/") if part]
    if len(parts) < 4 or parts[2] not in {"tree", "blob"}:
        return "main"

    tail = parts[3:]
    record_path = source_record_id.split(":", 1)[1].strip() if ":" in source_record_id else ""
    record_dir_parts = [part for part in record_path.split("/")[:-1] if part]
    if record_dir_parts and len(tail) > len(record_dir_parts) and tail[-len(record_dir_parts):] == record_dir_parts:
        return "/".join(tail[:-len(record_dir_parts)]) or "main"
    return "/".join(tail) or "main"


def _github_instance_from_record(rec: Dict[str, Any]) -> Dict[str, str]:
    source_record_id = str(rec.get("source_record_id") or "").strip()
    owner_repo = source_record_id.split(":", 1)[0].strip() if ":" in source_record_id else ""
    source_uri = str(rec.get("source_uri") or "").strip()
    parsed = urlparse(source_uri)
    parts = [part for part in parsed.path.strip("/").split("/") if part]
    if len(parts) >= 2 and not owner_repo:
        owner_repo = f"{parts[0]}/{parts[1].removesuffix('.git')}"
    ref = _github_ref_from_record(source_uri, source_record_id)
    if not owner_repo:
        owner_repo = source_uri or source_record_id or "github_import"
    key = f"github_import:{owner_repo}@{ref}"
    return {
        "key": key,
        "label": f"{owner_repo} @ {ref}" if owner_repo else key,
        "source_uri": f"https://github.com/{quote(owner_repo, safe='/')}/tree/{quote(ref)}" if "/" in owner_repo else source_uri,
    }


def _capability_source_instance(rec: Dict[str, Any]) -> Dict[str, str]:
    source_id = str(rec.get("source_id") or "").strip()
    if source_id == "github_import":
        return _github_instance_from_record(rec)
    source_uri = str(rec.get("source_uri") or "").strip()
    source_record_id = str(rec.get("source_record_id") or "").strip()
    value = source_uri or source_record_id or source_id
    return {
        "key": f"{source_id}:{value}",
        "label": value or source_id,
        "source_uri": source_uri,
    }


def capability_source_instances(records: Iterable[Dict[str, Any]]) -> List[Dict[str, Any]]:
    grouped: Dict[str, Dict[str, Any]] = {}
    for rec in records:
        if not isinstance(rec, dict):
            continue
        source_id = str(rec.get("source_id") or "").strip()
        if source_id not in {"github_import", "url_import", "local_import", "manual_import", "agent_self_proposed"}:
            continue
        capability_id = str(rec.get("capability_id") or "").strip()
        if not capability_id:
            continue
        instance = _capability_source_instance(rec)
        key = str(instance.get("key") or "").strip()
        if not key:
            continue
        row = grouped.setdefault(
            key,
            {
                "source_instance_key": key,
                "source_id": source_id,
                "label": str(instance.get("label") or key),
                "source_uri": str(instance.get("source_uri") or ""),
                "record_count": 0,
                "capability_ids": [],
                "last_synced_at": "",
                "sync_state": "",
            },
        )
        row["record_count"] = int(row.get("record_count") or 0) + 1
        row["capability_ids"] = [*list(row.get("capability_ids") or []), capability_id]
        last_synced_at = str(rec.get("last_synced_at") or "").strip()
        if last_synced_at and last_synced_at > str(row.get("last_synced_at") or ""):
            row["last_synced_at"] = last_synced_at
        sync_state = str(rec.get("sync_state") or "").strip()
        if sync_state and not str(row.get("sync_state") or ""):
            row["sync_state"] = sync_state
    return sorted(grouped.values(), key=lambda row: (str(row.get("source_id") or ""), str(row.get("label") or "")))


def capability_record_matches_source_instance(rec: Dict[str, Any], source_instance_key: str) -> bool:
    if not isinstance(rec, dict):
        return False
    key = str(source_instance_key or "").strip()
    if not key:
        return False
    return str(_capability_source_instance(rec).get("key") or "").strip() == key
