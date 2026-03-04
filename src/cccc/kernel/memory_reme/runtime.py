from __future__ import annotations

import asyncio
import re
import threading
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

from ...util.time import utc_now_iso
from ...vendor.reme.core.enumeration import MemorySource
from ...vendor.reme.core.file_store import LocalFileStore
from ...vendor.reme.core.schema import FileMetadata, MemorySearchResult
from ...vendor.reme.core.utils.chunking_utils import chunk_markdown
from ...vendor.reme.core.utils.common_utils import hash_text
from .layout import MemoryLayout, resolve_memory_layout


@dataclass
class RemeRuntime:
    group_id: str
    layout: MemoryLayout
    store: LocalFileStore
    lock: threading.RLock = field(default_factory=threading.RLock)
    started: bool = False
    last_sync_at: str = ""
    indexed_files: int = 0
    indexed_chunks: int = 0
    watched_paths: List[str] = field(default_factory=list)


_RUNTIME_CACHE: Dict[str, RemeRuntime] = {}
_RUNTIME_CACHE_LOCK = threading.RLock()


def _run_async(coro: Any) -> Any:
    try:
        asyncio.get_running_loop()
    except RuntimeError:
        return asyncio.run(coro)

    result: Dict[str, Any] = {}
    err: Dict[str, BaseException] = {}

    def _worker() -> None:
        loop = asyncio.new_event_loop()
        try:
            asyncio.set_event_loop(loop)
            result["value"] = loop.run_until_complete(coro)
        except BaseException as e:  # pragma: no cover - rare nested-loop fallback
            err["error"] = e
        finally:
            try:
                loop.run_until_complete(loop.shutdown_asyncgens())
            except Exception:
                pass
            loop.close()

    t = threading.Thread(target=_worker, daemon=True)
    t.start()
    t.join()
    if "error" in err:
        raise err["error"]
    return result.get("value")


def _store_name(group_id: str) -> str:
    token = re.sub(r"[^A-Za-z0-9_]+", "_", str(group_id or "").strip())
    token = token.strip("_") or "group"
    return f"reme_{token}"


def _build_runtime(group_id: str) -> RemeRuntime:
    layout = resolve_memory_layout(group_id, ensure_files=True)
    index_dir = layout.memory_root / ".index"
    store = LocalFileStore(
        store_name=_store_name(group_id),
        db_path=index_dir,
        vector_enabled=False,
        fts_enabled=True,
    )
    rt = RemeRuntime(
        group_id=group_id,
        layout=layout,
        store=store,
    )
    _run_async(store.start())
    rt.started = True
    return rt


def get_runtime(group_id: str) -> Optional[RemeRuntime]:
    gid = str(group_id or "").strip()
    if not gid:
        return None
    with _RUNTIME_CACHE_LOCK:
        rt = _RUNTIME_CACHE.get(gid)
        if rt is not None:
            return rt
    try:
        built = _build_runtime(gid)
    except Exception:
        return None
    with _RUNTIME_CACHE_LOCK:
        existing = _RUNTIME_CACHE.get(gid)
        if existing is not None:
            return existing
        _RUNTIME_CACHE[gid] = built
        return built


def close_all_runtimes() -> None:
    with _RUNTIME_CACHE_LOCK:
        runtimes = list(_RUNTIME_CACHE.values())
        _RUNTIME_CACHE.clear()
    for rt in runtimes:
        try:
            _run_async(rt.store.close())
        except Exception:
            pass


def _collect_target_files(layout: MemoryLayout) -> List[Path]:
    files: List[Path] = []
    if layout.memory_file.exists():
        files.append(layout.memory_file)
    if layout.daily_dir.exists():
        files.extend(sorted(p for p in layout.daily_dir.glob("*.md") if p.is_file()))
    return files


def index_sync(group_id: str, *, mode: str = "scan") -> Dict[str, Any]:
    rt = get_runtime(group_id)
    if rt is None:
        raise ValueError(f"group not found: {group_id}")

    normalized_mode = str(mode or "scan").strip().lower()
    if normalized_mode not in {"scan", "rebuild"}:
        raise ValueError("mode must be 'scan' or 'rebuild'")

    with rt.lock:
        rt.layout = resolve_memory_layout(group_id, ensure_files=True)
        if normalized_mode == "rebuild":
            _run_async(rt.store.clear_all())

        target_files = _collect_target_files(rt.layout)
        wanted_paths = {str(p.resolve()) for p in target_files}

        existing_paths = set(_run_async(rt.store.list_files(MemorySource.MEMORY)))
        stale = [p for p in existing_paths if p not in wanted_paths]
        for path in stale:
            _run_async(rt.store.delete_file(path, MemorySource.MEMORY))

        indexed_files = 0
        indexed_chunks = 0
        watched_paths: List[str] = []

        for fp in target_files:
            abs_path = str(fp.resolve())
            watched_paths.append(abs_path)
            text = fp.read_text(encoding="utf-8", errors="replace")
            st = fp.stat()
            digest = hash_text(text)

            prev = _run_async(rt.store.get_file_metadata(abs_path, MemorySource.MEMORY))
            if (
                normalized_mode == "scan"
                and prev is not None
                and str(prev.hash or "") == digest
                and int(prev.size or 0) == int(st.st_size)
            ):
                indexed_files += 1
                indexed_chunks += int(prev.chunk_count or 0)
                continue

            chunks = (
                chunk_markdown(
                    text=text,
                    path=abs_path,
                    source=MemorySource.MEMORY,
                    chunk_tokens=400,
                    overlap=80,
                )
                or []
            )
            if chunks:
                chunks = _run_async(rt.store.get_chunk_embeddings(chunks))

            meta = FileMetadata(
                hash=digest,
                mtime_ms=st.st_mtime * 1000,
                size=st.st_size,
                path=abs_path,
                content=text,
                chunk_count=len(chunks),
            )
            _run_async(rt.store.upsert_file(meta, MemorySource.MEMORY, chunks))
            indexed_files += 1
            indexed_chunks += len(chunks)

        rt.last_sync_at = utc_now_iso()
        rt.indexed_files = indexed_files
        rt.indexed_chunks = indexed_chunks
        rt.watched_paths = watched_paths

        return {
            "indexed_files": indexed_files,
            "indexed_chunks": indexed_chunks,
            "watched_paths": watched_paths,
            "last_sync_at": rt.last_sync_at,
        }


def search(
    group_id: str,
    *,
    query: str,
    max_results: int = 5,
    min_score: float = 0.1,
    sources: Optional[List[str]] = None,
    vector_weight: Optional[float] = None,
    candidate_multiplier: Optional[float] = None,
) -> Dict[str, Any]:
    rt = get_runtime(group_id)
    if rt is None:
        raise ValueError(f"group not found: {group_id}")
    if not str(query or "").strip():
        raise ValueError("query is required")

    with rt.lock:
        if rt.indexed_files <= 0:
            index_sync(group_id, mode="scan")

        src_values = sources or ["memory"]
        source_enums: List[MemorySource] = []
        for token in src_values:
            t = str(token or "").strip().lower()
            if t == MemorySource.MEMORY.value:
                source_enums.append(MemorySource.MEMORY)
            elif t == MemorySource.SESSIONS.value:
                source_enums.append(MemorySource.SESSIONS)
        if not source_enums:
            source_enums = [MemorySource.MEMORY]

        v_weight = float(vector_weight) if vector_weight is not None else 0.7
        c_mult = float(candidate_multiplier) if candidate_multiplier is not None else 3.0

        results: List[MemorySearchResult] = _run_async(
            rt.store.hybrid_search(
                query=str(query),
                limit=max(1, min(int(max_results), 50)),
                sources=source_enums,
                vector_weight=v_weight,
                candidate_multiplier=c_mult,
            )
        )

        hits: List[Dict[str, Any]] = []
        for item in results:
            if float(item.score) < float(min_score):
                continue
            hits.append(
                {
                    "path": str(item.path),
                    "start_line": int(item.start_line),
                    "end_line": int(item.end_line),
                    "score": float(item.score),
                    "snippet": str(item.snippet),
                    "source": str(item.source.value if hasattr(item.source, "value") else item.source),
                    "raw_metric": float(item.raw_metric) if item.raw_metric is not None else None,
                    "metadata": dict(item.metadata or {}),
                }
            )
        return {"hits": hits}


def get_file_slice(
    group_id: str,
    *,
    path: str,
    offset: int = 1,
    limit: int = 200,
) -> Dict[str, Any]:
    rt = get_runtime(group_id)
    if rt is None:
        raise ValueError(f"group not found: {group_id}")

    with rt.lock:
        rt.layout = resolve_memory_layout(group_id, ensure_files=True)
        target = Path(str(path or "")).expanduser().resolve()
        root = rt.layout.memory_root.resolve()
        if target != root and root not in target.parents:
            raise ValueError("path must be inside state/memory")
        if not target.exists() or not target.is_file():
            raise ValueError(f"file not found: {target}")

        lines = target.read_text(encoding="utf-8", errors="replace").splitlines()
        total = len(lines)
        start = max(1, int(offset))
        lim = max(1, min(int(limit), 5000))
        begin_idx = min(start - 1, total)
        end_idx = min(begin_idx + lim, total)
        chunk = "\n".join(lines[begin_idx:end_idx])

        return {
            "path": str(target),
            "offset": start,
            "limit": lim,
            "total_lines": total,
            "content": chunk,
        }

