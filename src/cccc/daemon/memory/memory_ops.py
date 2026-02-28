"""
Memory operations for daemon.

All memory operations go through the daemon to preserve single-writer on the SQLite DB.
Includes a simple connection pool cache: {group_id: MemoryStore}.
"""

from __future__ import annotations

import json
import os
import re
import threading
from collections import defaultdict
from typing import Any, Dict, List, Optional

from ...contracts.v1 import DaemonResponse, DaemonError
from ...kernel.group import load_group
from ...kernel.ledger import read_last_lines
from ...kernel.memory import MEMORY_KINDS, MemoryStore
from ...kernel.memory_export import export_markdown, export_manifest
from ...util.conv import coerce_bool


def _error(code: str, message: str, *, details: Optional[Dict[str, Any]] = None) -> DaemonResponse:
    return DaemonResponse(ok=False, error=DaemonError(code=code, message=message, details=(details or {})))


# =============================================================================
# Connection pool cache
# =============================================================================

_MAX_CACHED_STORES = 8
_store_cache: Dict[str, MemoryStore] = {}
_store_cache_lock = threading.Lock()


def _get_memory_store(group_id: str) -> Optional[MemoryStore]:
    """Get or create a MemoryStore for a group, with simple LRU cache."""
    with _store_cache_lock:
        if group_id in _store_cache:
            # Move to end for proper LRU ordering (most recently used = last)
            _store_cache[group_id] = _store_cache.pop(group_id)
            return _store_cache[group_id]

    group = load_group(group_id)
    if group is None:
        return None

    db_path = str(group.path / "memory.db")
    store = MemoryStore(db_path, group_id=group_id)

    evicted: Optional[MemoryStore] = None
    with _store_cache_lock:
        # Another thread may have created it while we were opening DB.
        existing = _store_cache.get(group_id)
        if existing is not None:
            _store_cache[group_id] = _store_cache.pop(group_id)
            store.close()
            return _store_cache[group_id]

        # Simple LRU: evict oldest if at capacity.
        if len(_store_cache) >= _MAX_CACHED_STORES:
            oldest_key = next(iter(_store_cache))
            evicted = _store_cache.pop(oldest_key)

        _store_cache[group_id] = store

    if evicted is not None:
        evicted.close()
    return store


def close_all_stores() -> None:
    """Close all cached stores (for clean shutdown)."""
    with _store_cache_lock:
        stores = list(_store_cache.values())
        _store_cache.clear()
    for store in stores:
        store.close()


# =============================================================================
# memory_store op
# =============================================================================


def handle_memory_store(args: Dict[str, Any]) -> DaemonResponse:
    """Store or update a memory.

    When 'id' is provided, updates the existing memory.
    When 'solidify' is true, solidifies after store/update.
    """
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    store = _get_memory_store(group_id)
    if store is None:
        return _error("group_not_found", f"group not found: {group_id}")

    memory_id = str(args.get("id") or "").strip()
    content = str(args.get("content") or "").strip()
    solidify = bool(args.get("solidify"))

    if memory_id:
        # Update mode
        update_kwargs: Dict[str, Any] = {}
        if content:
            update_kwargs["content"] = content
        for field in ("kind", "status", "confidence", "source_type", "source_ref",
                       "actor_id", "task_id", "event_ts"):
            if field in args:
                update_kwargs[field] = str(args[field] or "")
        if "tags" in args:
            raw_tags = args.get("tags")
            if isinstance(raw_tags, list):
                update_kwargs["tags"] = [str(t) for t in raw_tags]

        try:
            mem = store.update(memory_id, **update_kwargs)
        except ValueError as e:
            return _error("validation_error", str(e))
        if mem is None:
            return _error("memory_not_found", f"memory not found: {memory_id}")

        if solidify:
            mem = store.solidify(memory_id)

        return DaemonResponse(ok=True, result={"memory": _memory_to_dict(mem), "updated": True})

    # Create mode
    if not content:
        return _error("missing_content", "content is required for new memories")

    store_kwargs: Dict[str, Any] = {}
    for field in ("kind", "source_type", "source_ref", "status", "confidence",
                   "scope_key", "actor_id", "task_id", "event_ts", "strategy"):
        val = args.get(field)
        if val is not None:
            store_kwargs[field] = str(val)
    if "tags" in args:
        raw_tags = args.get("tags")
        if isinstance(raw_tags, list):
            store_kwargs["tags"] = [str(t) for t in raw_tags]

    try:
        result = store.store(content, **store_kwargs)
    except ValueError as e:
        return _error("validation_error", str(e))

    if solidify and not result.get("deduplicated"):
        store.solidify(result["id"])
        result["status"] = "solid"

    return DaemonResponse(ok=True, result=result)


def _memory_to_dict(mem: Dict[str, Any]) -> Dict[str, Any]:
    """Normalize memory dict for response."""
    return {
        "id": mem.get("id", ""),
        "content": mem.get("content", ""),
        "kind": mem.get("kind", ""),
        "source_type": mem.get("source_type", ""),
        "source_ref": mem.get("source_ref", ""),
        "status": mem.get("status", ""),
        "confidence": mem.get("confidence", ""),
        "group_id": mem.get("group_id", ""),
        "scope_key": mem.get("scope_key", ""),
        "actor_id": mem.get("actor_id", ""),
        "task_id": mem.get("task_id", ""),
        "event_ts": mem.get("event_ts", ""),
        "created_at": mem.get("created_at", ""),
        "updated_at": mem.get("updated_at", ""),
        "last_recalled_at": mem.get("last_recalled_at", ""),
        "content_hash": mem.get("content_hash", ""),
        "hit_count": mem.get("hit_count", 0),
        "tags": mem.get("tags", []),
    }


# =============================================================================
# memory_search op
# =============================================================================


def handle_memory_search(args: Dict[str, Any]) -> DaemonResponse:
    """Search memories via recall()."""
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    store = _get_memory_store(group_id)
    if store is None:
        return _error("group_not_found", f"group not found: {group_id}")

    recall_kwargs: Dict[str, Any] = {}
    query = str(args.get("query") or "").strip()
    if query:
        recall_kwargs["query"] = query

    for field in ("status", "kind", "actor_id", "task_id", "confidence", "since", "until"):
        val = args.get(field)
        if val is not None:
            recall_kwargs[field] = str(val)

    if "tags" in args:
        raw_tags = args.get("tags")
        if isinstance(raw_tags, list):
            recall_kwargs["tags"] = [str(t) for t in raw_tags]

    if "track_hit" in args:
        recall_kwargs["track_hit"] = coerce_bool(args.get("track_hit"), default=False)

    limit = args.get("limit")
    if limit is not None:
        try:
            recall_kwargs["limit"] = min(max(int(limit), 1), 100)
        except (TypeError, ValueError):
            return _error("validation_error", "limit must be an integer")

    try:
        results = store.recall(**recall_kwargs)
    except ValueError as e:
        return _error("validation_error", str(e))
    return DaemonResponse(ok=True, result={
        "memories": [_memory_to_dict(m) for m in results],
        "count": len(results),
    })


# =============================================================================
# memory_stats op
# =============================================================================


def handle_memory_stats(args: Dict[str, Any]) -> DaemonResponse:
    """Get memory statistics."""
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    store = _get_memory_store(group_id)
    if store is None:
        return _error("group_not_found", f"group not found: {group_id}")

    stats = store.stats()
    return DaemonResponse(ok=True, result=stats)


# =============================================================================
# memory_ingest op (T098)
# =============================================================================

_WATERMARK_META_KEY = "ingest_watermark"

_DEFAULT_INGEST_LIMIT = 50
_MAX_INGEST_LIMIT = 200
_MAX_INGEST_EXPANSION_LIMIT = 200_000


def _parse_chat_events(lines: List[str]) -> List[Dict[str, Any]]:
    """Parse ledger lines into chat.message events."""
    events: List[Dict[str, Any]] = []
    for raw in lines:
        raw = raw.strip()
        if not raw:
            continue
        try:
            ev = json.loads(raw)
        except (json.JSONDecodeError, ValueError):
            continue
        if str(ev.get("kind") or "") != "chat.message":
            continue
        events.append(ev)
    return events


def _filter_after_watermark(events: List[Dict[str, Any]], watermark: str) -> tuple:
    """Filter events that come after the watermark event_id.

    Returns (filtered_events, watermark_found: bool).
    When watermark is not found in the window, returns all events with found=False.
    """
    if not watermark:
        return events, True  # No watermark = treat as found (fresh start)
    # Find watermark position, return everything after it
    for i, ev in enumerate(events):
        if str(ev.get("id") or "") == watermark:
            return events[i + 1:], True
    # Watermark not found in window — return all (stale watermark)
    return events, False


def _extract_key_phrases(text: str, max_phrases: int = 5) -> List[str]:
    """Extract simple key phrases from text (word frequency based)."""
    # Remove common short words and extract significant tokens
    words = re.findall(r'[\w\u4e00-\u9fff\u3040-\u309f\u30a0-\u30ff]{2,}', text.lower())
    stop_words = {
        "the", "and", "for", "are", "but", "not", "you", "all", "can", "had",
        "her", "was", "one", "our", "out", "has", "have", "this", "that", "with",
        "from", "they", "been", "will", "would", "could", "should", "there",
        "their", "what", "when", "make", "like", "just", "into", "than", "then",
        "also", "about", "more", "some", "very", "much", "each", "other",
        "的", "了", "在", "是", "我", "有", "和", "就", "不", "人", "都", "一",
        "个", "上", "也", "很", "到", "说", "要", "去", "你", "会", "着", "没有",
        "看", "好", "自己", "这", "他", "她", "它", "们", "那", "把", "被",
    }
    freq: Dict[str, int] = defaultdict(int)
    for w in words:
        if w not in stop_words:
            freq[w] += 1
    sorted_words = sorted(freq.items(), key=lambda x: x[1], reverse=True)
    return [w for w, _ in sorted_words[:max_phrases]]


def _suggest_kind(text: str) -> str:
    """Suggest a memory kind based on text content.

    Always returns a valid kind from MEMORY_KINDS.
    """
    lower = text.lower()
    decision_words = {"decided", "decision", "chose", "choice", "agreed", "决定", "选择", "确定", "采用"}
    instruction_words = {"plan", "todo", "next", "will", "计划", "下一步", "接下来", "准备"}
    fact_words = {"found", "discovered", "confirmed", "verified", "发现", "确认", "验证"}

    for w in decision_words:
        if w in lower:
            return "decision"
    for w in instruction_words:
        if w in lower:
            return "instruction"
    for w in fact_words:
        if w in lower:
            return "fact"
    return "observation"


def _ingest_signal(events: List[Dict[str, Any]]) -> Dict[str, Any]:
    """Signal mode: produce structured summary grouped by actor + time segments."""
    if not events:
        return {"signals": [], "events_processed": 0}

    # Group messages by actor
    by_actor: Dict[str, List[Dict[str, Any]]] = defaultdict(list)
    for ev in events:
        actor = str(ev.get("by") or "unknown")
        data = ev.get("data") if isinstance(ev.get("data"), dict) else {}
        text = str(data.get("text") or "")
        ts = str(ev.get("ts") or "")
        by_actor[actor].append({"text": text, "ts": ts, "event_id": str(ev.get("id") or "")})

    signals: List[Dict[str, Any]] = []
    for actor, msgs in by_actor.items():
        combined_text = " ".join(m["text"] for m in msgs if m["text"])
        if not combined_text.strip():
            continue
        signals.append({
            "actor_id": actor,
            "messages_count": len(msgs),
            "suggested_kind": _suggest_kind(combined_text),
            "key_phrases": _extract_key_phrases(combined_text),
            "time_range": {
                "first": msgs[0]["ts"] if msgs else "",
                "last": msgs[-1]["ts"] if msgs else "",
            },
            "topic": combined_text[:200].strip(),
        })

    return {"signals": signals, "events_processed": len(events)}


def _ingest_raw(
    events: List[Dict[str, Any]],
    store: MemoryStore,
    group_id: str,
    actor_filter: str = "",
) -> Dict[str, Any]:
    """Raw mode: bulk import chat messages as memories."""
    imported = 0
    skipped = 0

    for ev in events:
        by = str(ev.get("by") or "")
        if actor_filter and by != actor_filter:
            skipped += 1
            continue

        data = ev.get("data") if isinstance(ev.get("data"), dict) else {}
        text = str(data.get("text") or "").strip()
        if not text:
            skipped += 1
            continue

        event_id = str(ev.get("id") or "")
        event_ts = str(ev.get("ts") or "")

        result = store.store(
            text,
            kind="observation",
            source_type="chat_ingest",
            source_ref=event_id,
            actor_id=by,
            event_ts=event_ts,
        )
        if result.get("deduplicated"):
            skipped += 1
        else:
            imported += 1

    return {"imported": imported, "skipped": skipped}


def handle_memory_ingest(args: Dict[str, Any]) -> DaemonResponse:
    """Ingest chat messages into memory.

    Modes:
      - signal: returns structured summary for agent to decide what to store
      - raw: bulk imports chat messages as source_type=chat_ingest memories

    Uses watermark (last_ingest_event_id) to skip already-processed events.
    """
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    store = _get_memory_store(group_id)
    if store is None:
        return _error("group_not_found", f"group not found: {group_id}")

    mode = str(args.get("mode") or "signal").strip()
    if mode not in ("signal", "raw"):
        return _error("invalid_mode", f"mode must be 'signal' or 'raw', got: {mode}")

    limit = min(max(int(args.get("limit") or _DEFAULT_INGEST_LIMIT), 1), _MAX_INGEST_LIMIT)
    actor_filter = str(args.get("actor_id") or "").strip()
    reset_watermark = bool(args.get("reset_watermark"))

    # Apply watermark (persisted in memory_meta)
    if reset_watermark:
        store.delete_meta(_WATERMARK_META_KEY)
    watermark = store.get_meta(_WATERMARK_META_KEY, "")

    # Read ledger lines; if watermark is stale in current window, expand progressively
    # until watermark is found or we hit file head.
    read_limit = limit
    lines = read_last_lines(group.ledger_path, read_limit)
    parsed_events = _parse_chat_events(lines)
    events, watermark_found = _filter_after_watermark(parsed_events, watermark)

    while watermark and not watermark_found:
        if len(lines) < read_limit:
            # We already reached file head.
            break
        next_limit = min(read_limit * 2, _MAX_INGEST_EXPANSION_LIMIT)
        if next_limit <= read_limit:
            break
        read_limit = next_limit
        lines = read_last_lines(group.ledger_path, read_limit)
        parsed_events = _parse_chat_events(lines)
        events, watermark_found = _filter_after_watermark(parsed_events, watermark)

    # Update watermark to last event processed.
    # Only advance watermark when the previous watermark was found in the window,
    # preventing skipped messages when limit < total new events.
    new_watermark = ""
    if events and watermark_found:
        last_id = str(events[-1].get("id") or "")
        if last_id:
            new_watermark = last_id

    if mode == "signal":
        result = _ingest_signal(events)
    else:
        result = _ingest_raw(events, store, group_id, actor_filter)

    # Persist watermark to memory_meta (survives daemon restart)
    if new_watermark:
        store.set_meta(_WATERMARK_META_KEY, new_watermark)

    result["watermark"] = new_watermark or watermark
    result["watermark_stale"] = not watermark_found and bool(watermark)
    result["mode"] = mode
    return DaemonResponse(ok=True, result=result)


# =============================================================================
# memory_delete op
# =============================================================================


def handle_memory_delete(args: Dict[str, Any]) -> DaemonResponse:
    """Delete memory by ID(s).

    Supports:
    - id: single memory ID
    - ids: list of memory IDs (batch delete)
    """
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    store = _get_memory_store(group_id)
    if store is None:
        return _error("group_not_found", f"group not found: {group_id}")

    memory_id = str(args.get("id") or "").strip()
    raw_ids = args.get("ids")

    ids: List[str] = []
    if raw_ids is not None:
        if not isinstance(raw_ids, list):
            return _error("validation_error", "ids must be a list")
        ids = [str(x or "").strip() for x in raw_ids if str(x or "").strip()]

    # Single delete mode (backward-compatible)
    if memory_id:
        deleted = store.delete(memory_id)
        return DaemonResponse(
            ok=True,
            result={
                "deleted": deleted,
                "deleted_count": 1 if deleted else 0,
                "ids": [memory_id] if deleted else [],
            },
        )

    # Batch delete mode
    if ids:
        result = store.delete_many(ids)
        count = int(result.get("deleted") or 0)
        deleted_ids = result.get("ids") if isinstance(result.get("ids"), list) else []
        return DaemonResponse(
            ok=True,
            result={
                "deleted": count > 0,
                "deleted_count": count,
                "ids": deleted_ids,
            },
        )

    return _error("missing_id", "missing memory id or ids")


# =============================================================================
# memory_decay op
# =============================================================================


def handle_memory_decay(args: Dict[str, Any]) -> DaemonResponse:
    """Find stale memory candidates for cleanup (safe: no deletion)."""
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    store = _get_memory_store(group_id)
    if store is None:
        return _error("group_not_found", f"group not found: {group_id}")

    def _as_int(name: str, default: int) -> int:
        val = args.get(name)
        if val is None:
            return default
        try:
            return int(val)
        except (TypeError, ValueError):
            raise ValueError(f"{name} must be an integer")

    try:
        result = store.find_stale(
            draft_days=_as_int("draft_days", 30),
            zero_hit_days=_as_int("zero_hit_days", 14),
            solid_review_days=_as_int("solid_review_days", 120),
            solid_max_hit=_as_int("solid_max_hit", 1),
            limit=_as_int("limit", 100),
        )
    except ValueError as e:
        return _error("validation_error", str(e))

    return DaemonResponse(ok=True, result=result)


# =============================================================================
# memory_solidify_batch op (Step 2)
# =============================================================================


def handle_memory_solidify_batch(args: Dict[str, Any]) -> DaemonResponse:
    """Batch solidify draft memories, optionally filtered by task/kind."""
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    store = _get_memory_store(group_id)
    if store is None:
        return _error("group_not_found", f"group not found: {group_id}")

    kwargs: Dict[str, Any] = {}
    task_id = str(args.get("task_id") or "").strip()
    if task_id:
        kwargs["task_id"] = task_id
    kind = str(args.get("kind") or "").strip()
    if kind:
        kwargs["kind"] = kind

    try:
        result = store.solidify_batch(**kwargs)
    except ValueError as e:
        return _error("validation_error", str(e))

    return DaemonResponse(ok=True, result=result)


# =============================================================================
# memory_export op (Step 3)
# =============================================================================


def handle_memory_export(args: Dict[str, Any]) -> DaemonResponse:
    """Export memories as Markdown + manifest.

    Writes memory.md and manifest.json to the group's state directory.
    Returns the manifest and file paths.
    """
    group_id = str(args.get("group_id") or "").strip()
    if not group_id:
        return _error("missing_group_id", "missing group_id")

    group = load_group(group_id)
    if group is None:
        return _error("group_not_found", f"group not found: {group_id}")

    store = _get_memory_store(group_id)
    if store is None:
        return _error("group_not_found", f"group not found: {group_id}")

    include_draft = bool(args.get("include_draft"))
    output_dir = str(args.get("output_dir") or "").strip()
    if not output_dir:
        output_dir = str(group.path / "state")

    os.makedirs(output_dir, exist_ok=True)

    md = export_markdown(store, include_draft=include_draft)

    # Count exported memories
    count = 0
    for line in md.split("\n"):
        if line.startswith("### ["):
            count += 1

    manifest = export_manifest(md, group_id=group_id, memory_count=count)

    md_path = os.path.join(output_dir, "memory.md")
    manifest_path = os.path.join(output_dir, "manifest.json")

    with open(md_path, "w", encoding="utf-8") as f:
        f.write(md)
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)

    # Record export in meta
    store.set_meta("last_export", json.dumps({
        "sha256": manifest["sha256"],
        "memory_count": count,
        "at": manifest["exported_at"],
        "md_path": md_path,
    }))

    return DaemonResponse(ok=True, result={
        "manifest": manifest,
        "md_path": md_path,
        "manifest_path": manifest_path,
    })


# =============================================================================
# Dispatcher
# =============================================================================


def try_handle_memory_op(op: str, args: Dict[str, Any]) -> Optional[DaemonResponse]:
    if op == "memory_store":
        return handle_memory_store(args)
    if op == "memory_search":
        return handle_memory_search(args)
    if op == "memory_stats":
        return handle_memory_stats(args)
    if op == "memory_ingest":
        return handle_memory_ingest(args)
    if op == "memory_solidify_batch":
        return handle_memory_solidify_batch(args)
    if op == "memory_export":
        return handle_memory_export(args)
    if op == "memory_delete":
        return handle_memory_delete(args)
    if op == "memory_decay":
        return handle_memory_decay(args)
    return None
