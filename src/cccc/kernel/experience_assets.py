from __future__ import annotations

import re
from pathlib import Path
from typing import Any, Dict, List, Optional

from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso
from .group import Group

_ASSET_SCHEMA = 1
_TOKEN_RE = re.compile(r"[\w\u4e00-\u9fff]+", re.UNICODE)
_ACTIVE_ASSET_STATUSES = {"promoted", "promoted_to_memory"}


def _experience_assets_dir(group: Group) -> Path:
    return group.path / "state" / "experience_assets"


def _asset_path(group: Group, candidate_id: str) -> Path:
    safe_candidate_id = str(candidate_id or "").strip()
    return _experience_assets_dir(group) / f"{safe_candidate_id}.json"


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


def _tokenize(text: str) -> List[str]:
    return [token.lower() for token in _TOKEN_RE.findall(str(text or ""))]


def _score_asset(asset: Dict[str, Any], *, query_tokens: List[str]) -> float:
    if not query_tokens:
        return 0.0
    haystack = " ".join(
        [
            str(asset.get("title") or ""),
            str(asset.get("summary") or ""),
            str(asset.get("recommended_action") or ""),
            " ".join(_string_list(asset.get("source_refs"))),
        ]
    ).lower()
    if not haystack.strip():
        return 0.0
    matches = sum(1 for token in query_tokens if token and token in haystack)
    if matches <= 0:
        return 0.0
    return round(matches / max(len(query_tokens), 1), 4)


def _normalize_asset_record(raw: Dict[str, Any], *, file_path: str = "", query_tokens: Optional[List[str]] = None) -> Optional[Dict[str, Any]]:
    if not isinstance(raw, dict):
        return None
    candidate_id = str(raw.get("candidate_id") or "").strip()
    if not candidate_id:
        return None
    status = str(raw.get("status") or "").strip().lower()
    if status and status not in _ACTIVE_ASSET_STATUSES:
        return None
    asset = dict(raw)
    if file_path:
        asset["file_path"] = str(file_path)
    query_tokens = query_tokens if isinstance(query_tokens, list) else []
    asset["score"] = _score_asset(asset, query_tokens=query_tokens)
    return asset


def _select_assets(raw_assets: List[Dict[str, Any]], *, query: str, limit: int) -> List[Dict[str, Any]]:
    capped_limit = max(int(limit or 0), 0)
    if capped_limit <= 0:
        return []
    query_tokens = _tokenize(query)
    scored: List[Dict[str, Any]] = []
    for item in raw_assets:
        asset = _normalize_asset_record(
            item,
            file_path=str(item.get("file_path") or ""),
            query_tokens=query_tokens,
        )
        if asset is None:
            continue
        scored.append(asset)
    scored.sort(
        key=lambda item: (
            float(item.get("score") or 0.0),
            str(item.get("promoted_at") or ""),
            str(item.get("updated_at") or ""),
        ),
        reverse=True,
    )
    return scored[:capped_limit]


def build_experience_asset(candidate: Dict[str, Any], *, memory_entry: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    current = dict(candidate or {})
    promotion = current.get("promotion") if isinstance(current.get("promotion"), dict) else {}
    memory_meta = memory_entry if isinstance(memory_entry, dict) else {}
    candidate_id = str(current.get("id") or "").strip()
    updated_at = utc_now_iso()
    title = str(current.get("title") or "").strip() or str(current.get("summary") or "").strip() or candidate_id
    summary = _trim_text(current.get("summary"), max_chars=220)
    recommended_action = _trim_text(current.get("recommended_action"), max_chars=220)
    failure_signals = _string_list(current.get("failure_signals"))[:3]
    return {
        "schema": _ASSET_SCHEMA,
        "asset_id": f"expasset_{candidate_id}" if candidate_id else "",
        "candidate_id": candidate_id,
        "status": str(current.get("status") or "").strip(),
        "title": title,
        "summary": summary,
        "recommended_action": recommended_action,
        "failure_signals": failure_signals,
        "source_kind": str(current.get("source_kind") or "").strip(),
        "source_refs": _string_list(current.get("source_refs")),
        "memory_entry_id": str(memory_meta.get("entry_id") or "").strip(),
        "memory_file_path": str(memory_meta.get("file_path") or "").strip(),
        "promoted_at": str(promotion.get("at") or current.get("updated_at") or "").strip(),
        "created_at": str(current.get("created_at") or updated_at).strip() or updated_at,
        "updated_at": updated_at,
    }


def write_experience_asset_mirror(
    group: Group,
    *,
    candidate: Dict[str, Any],
    memory_entry: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    asset = build_experience_asset(candidate, memory_entry=memory_entry)
    candidate_id = str(asset.get("candidate_id") or "").strip()
    if not candidate_id:
        raise ValueError("candidate_id is required for experience asset mirror")
    path = _asset_path(group, candidate_id)
    atomic_write_json(path, asset, indent=2)
    return {
        "status": "written",
        "file_path": str(path),
        "asset": asset,
    }


def delete_experience_asset_mirror(group: Group, *, candidate_id: str) -> Dict[str, Any]:
    normalized_candidate_id = str(candidate_id or "").strip()
    if not normalized_candidate_id:
        return {"status": "skipped", "file_path": "", "asset_id": ""}
    path = _asset_path(group, normalized_candidate_id)
    asset_id = f"expasset_{normalized_candidate_id}"
    if not path.exists():
        return {"status": "skipped", "file_path": str(path), "asset_id": asset_id}
    path.unlink()
    return {"status": "deleted", "file_path": str(path), "asset_id": asset_id}


def list_experience_assets(group: Group, *, query: str = "", limit: int = 3) -> List[Dict[str, Any]]:
    assets_dir = _experience_assets_dir(group)
    if not assets_dir.exists():
        return []
    raw_assets: List[Dict[str, Any]] = []
    for path in sorted(assets_dir.glob("*.json")):
        raw = read_json(path)
        if not isinstance(raw, dict):
            continue
        item = dict(raw)
        item["file_path"] = str(path)
        raw_assets.append(item)
    return _select_assets(raw_assets, query=query, limit=limit)


def select_experience_assets_for_consumption(
    group: Group,
    *,
    query: str = "",
    limit: int = 3,
    provided_assets: Optional[List[Dict[str, Any]]] = None,
) -> List[Dict[str, Any]]:
    if isinstance(provided_assets, list):
        return _select_assets([dict(item) for item in provided_assets if isinstance(item, dict)], query=query, limit=limit)
    return list_experience_assets(group, query=query, limit=limit)
