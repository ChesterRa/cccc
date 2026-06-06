from __future__ import annotations

import json
import re
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

from ...contracts.v1 import DaemonError, DaemonResponse
from ...kernel.memory_reme import get_runtime, index_sync, resolve_memory_layout
from ...util.time import utc_now_iso


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


def _memory_sdk_meta(*, source: str, started_at: float) -> Dict[str, Any]:
    return {
        "provider": "cccc-memory",
        "source": source,
        "latencyMs": int((time.perf_counter() - started_at) * 1000),
    }


def _memory_sdk_error(resp: DaemonResponse) -> DaemonResponse:
    if resp.ok or resp.error is None:
        return resp
    code = str(resp.error.code or "")
    mapped = {
        "group_not_found": "memory_group_missing",
        "missing_group_id": "memory_group_missing",
    }.get(code, code)
    return _error(mapped, str(resp.error.message or code), details=dict(resp.error.details or {}))


def _memory_sdk_sources_for_target(target: Any) -> List[str]:
    token = str(target or "").strip().lower()
    # ReMe currently indexes the local memory file and daily memory files under
    # the memory source. Keep this mapping explicit so SDK target stays stable.
    if token in {"", "memory", "daily"}:
        return ["memory"]
    return ["memory"]


_MEMORY_META_RE = re.compile(r"<!--\s*cccc\.memory\.meta\s+(\{.*?\})\s*-->", flags=re.DOTALL)


def _parse_memory_meta(text: str) -> Dict[str, Any]:
    match = _MEMORY_META_RE.search(str(text or ""))
    if not match:
        return {}
    try:
        parsed = json.loads(match.group(1))
    except Exception:
        return {}
    return parsed if isinstance(parsed, dict) else {}


def _memory_sdk_entry_meta_for_hit(path: str, start_line: int) -> Dict[str, Any]:
    try:
        lines = Path(path).read_text(encoding="utf-8", errors="replace").splitlines()
    except Exception:
        return {}
    line_index = max(0, min(int(start_line or 1) - 1, len(lines) - 1))
    for idx in range(line_index, -1, -1):
        metadata = _parse_memory_meta(lines[idx])
        if metadata:
            return metadata
    return {}


def _memory_sdk_hit(raw: Dict[str, Any]) -> Dict[str, Any]:
    metadata = raw.get("metadata") if isinstance(raw.get("metadata"), dict) else {}
    snippet = str(raw.get("snippet") or "")
    if not metadata:
        metadata = _parse_memory_meta(snippet)
    path = str(raw.get("path") or "")
    start_line = int(raw.get("start_line") or raw.get("startLine") or 1)
    if not metadata:
        metadata = _memory_sdk_entry_meta_for_hit(path, start_line)
    hit: Dict[str, Any] = {
        "path": path,
        "startLine": start_line,
        "score": float(raw.get("score") or 0.0),
        "snippet": snippet,
    }
    tags = metadata.get("tags")
    if isinstance(tags, list):
        hit["tags"] = [str(x) for x in tags]
    source_refs = metadata.get("source_refs") or metadata.get("sourceRefs")
    if isinstance(source_refs, list):
        hit["sourceRefs"] = [str(x) for x in source_refs]
    content = raw.get("content")
    if isinstance(content, str):
        hit["content"] = content
    return hit


def _memory_sdk_hit_matches(hit: Dict[str, Any], *, target: Any, tags: Any) -> bool:
    target_token = str(target or "").strip().lower()
    path = str(hit.get("path") or "")
    path_parts = [part for part in re.split(r"[\\/]+", path) if part]
    if target_token == "daily" and "daily" not in path_parts:
        return False
    if target_token == "memory" and not path.endswith("MEMORY.md"):
        return False

    tag_filter = {str(x) for x in tags if isinstance(x, str)} if isinstance(tags, list) else set()
    if tag_filter:
        hit_tags = {str(x) for x in hit.get("tags") or [] if isinstance(x, str)} if isinstance(hit.get("tags"), list) else set()
        if not tag_filter.issubset(hit_tags):
            return False
    return True


def _memory_sdk_path_from_target(args: Dict[str, Any]) -> str:
    path = str(args.get("path") or "").strip()
    if path:
        return path
    group_id = str(args.get("group_id") or "").strip()
    date = str(args.get("date") or "").strip() or None
    layout = resolve_memory_layout(group_id, date=date, ensure_files=True)
    target = str(args.get("target") or "memory").strip().lower()
    if target == "daily" or date:
        return str(layout.today_daily_file)
    return str(layout.memory_file)


def handle_memory_search(args: Dict[str, Any]) -> DaemonResponse:
    from .memory_ops import handle_memory_reme_search

    started_at = time.perf_counter()
    reme_args: Dict[str, Any] = {
        "group_id": args.get("group_id"),
        "query": args.get("query"),
        "sources": _memory_sdk_sources_for_target(args.get("target")),
        "min_score": 0.01,
    }
    if args.get("limit") is not None:
        reme_args["max_results"] = args.get("limit")
    resp = handle_memory_reme_search(reme_args)
    if not resp.ok:
        return _memory_sdk_error(resp)
    payload = resp.result if isinstance(resp.result, dict) else {}
    raw_hits = payload.get("hits") if isinstance(payload.get("hits"), list) else []
    hits = []
    for raw_hit in raw_hits:
        if not isinstance(raw_hit, dict):
            continue
        hit = _memory_sdk_hit(raw_hit)
        if _memory_sdk_hit_matches(hit, target=args.get("target"), tags=args.get("tags")):
            hits.append(hit)
    return DaemonResponse(
        ok=True,
        result={
            **_memory_sdk_meta(source="local-index", started_at=started_at),
            "hits": hits,
        },
    )


def handle_memory_get(args: Dict[str, Any]) -> DaemonResponse:
    from .memory_ops import handle_memory_reme_get

    started_at = time.perf_counter()
    try:
        path = _memory_sdk_path_from_target(args)
    except ValueError as e:
        return _error("memory_group_missing", str(e))
    reme_args: Dict[str, Any] = {
        "group_id": args.get("group_id"),
        "path": path,
    }
    if args.get("offset") is not None:
        reme_args["offset"] = args.get("offset")
    if args.get("limit") is not None:
        reme_args["limit"] = args.get("limit")
    resp = handle_memory_reme_get(reme_args)
    if not resp.ok:
        return _memory_sdk_error(resp)
    payload = resp.result if isinstance(resp.result, dict) else {}
    return DaemonResponse(
        ok=True,
        result={
            **_memory_sdk_meta(source="local-file", started_at=started_at),
            "path": str(payload.get("path") or path),
            "offset": int(payload.get("offset") or 1),
            "limit": int(payload.get("limit") or 0),
            "content": str(payload.get("content") or ""),
        },
    )


def handle_memory_write(args: Dict[str, Any]) -> DaemonResponse:
    from .memory_ops import handle_memory_reme_write

    started_at = time.perf_counter()
    target = str(args.get("target") or "").strip().lower()
    reme_args = dict(args)
    if target == "daily" and not str(reme_args.get("date") or "").strip():
        reme_args["date"] = utc_now_iso()[:10]
    resp = handle_memory_reme_write(reme_args)
    if not resp.ok:
        err = _memory_sdk_error(resp)
        if err.error and str(err.error.code or "") == "memory_runtime_error":
            return _error("memory_write_failed", str(err.error.message or "memory write failed"), details=dict(err.error.details or {}))
        return err
    payload = resp.result if isinstance(resp.result, dict) else {}
    return DaemonResponse(
        ok=True,
        result={
            **_memory_sdk_meta(source="local-file", started_at=started_at),
            "status": str(payload.get("status") or "written"),
            "path": str(payload.get("file_path") or ""),
            "contentHash": str(payload.get("content_hash") or ""),
            "dedup": payload.get("dedup") if isinstance(payload.get("dedup"), dict) else {},
        },
    )


def handle_memory_profile_get(args: Dict[str, Any]) -> DaemonResponse:
    started_at = time.perf_counter()
    query_parts = ["profile"]
    user_id = str(args.get("user_id") or "").strip()
    actor_id = str(args.get("actor_id") or "").strip()
    tags = [str(x) for x in args.get("tags") or [] if isinstance(x, str)] if isinstance(args.get("tags"), list) else []
    if user_id:
        query_parts.append(user_id)
    if actor_id:
        query_parts.append(actor_id)
    query_parts.extend(tags)
    search_resp = handle_memory_search(
        {
            "group_id": args.get("group_id"),
            "actor_id": actor_id,
            "query": " ".join(query_parts),
            "limit": 8,
            "tags": tags,
        }
    )
    if not search_resp.ok:
        return search_resp
    search_payload = search_resp.result if isinstance(search_resp.result, dict) else {}
    hits = search_payload.get("hits") if isinstance(search_payload.get("hits"), list) else []
    snippets = [str(hit.get("snippet") or "").strip() for hit in hits if isinstance(hit, dict) and str(hit.get("snippet") or "").strip()]
    return DaemonResponse(
        ok=True,
        result={
            **_memory_sdk_meta(source="local-index", started_at=started_at),
            "profile": "\n\n".join(snippets),
            "hits": hits,
        },
    )


def handle_memory_health(args: Dict[str, Any]) -> DaemonResponse:
    started_at = time.perf_counter()
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("memory_group_missing", "missing group_id")
    try:
        layout = resolve_memory_layout(group_id, ensure_files=True)
        runtime = get_runtime(group_id)
        if runtime is None:
            return DaemonResponse(
                ok=True,
                result={
                    **_memory_sdk_meta(source="local-index", started_at=started_at),
                    "status": "degraded",
                    "indexReady": False,
                    "writable": layout.memory_root.exists() and layout.memory_root.is_dir(),
                    "memoryRoot": str(layout.memory_root),
                },
            )
        index_sync(group_id, mode="scan")
        writable = layout.memory_root.exists() and layout.memory_root.is_dir()
        return DaemonResponse(
            ok=True,
            result={
                **_memory_sdk_meta(source="local-index", started_at=started_at),
                "status": "ok" if writable else "degraded",
                "indexReady": True,
                "writable": writable,
                "memoryRoot": str(layout.memory_root),
                "lastIndexedAt": str(runtime.last_sync_at or ""),
            },
        )
    except ValueError as e:
        return _error("memory_group_missing", str(e))
    except Exception as e:
        return DaemonResponse(
            ok=True,
            result={
                **_memory_sdk_meta(source="local-index", started_at=started_at),
                "status": "error",
                "indexReady": False,
                "writable": False,
                "memoryRoot": "",
                "error": str(e),
            },
        )


def try_handle_memory_sdk_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "memory_search":
        return handle_memory_search(args)
    if op == "memory_get":
        return handle_memory_get(args)
    if op == "memory_write":
        return handle_memory_write(args)
    if op == "memory_profile_get":
        return handle_memory_profile_get(args)
    if op == "memory_health":
        return handle_memory_health(args)
    return None
