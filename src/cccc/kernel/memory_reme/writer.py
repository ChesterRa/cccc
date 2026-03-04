from __future__ import annotations

import hashlib
import json
import uuid
import re
from pathlib import Path
from typing import Any, Dict, List, Optional

from ...util.fs import atomic_write_text
from ...util.time import utc_now_iso
from .layout import MemoryLayout, resolve_memory_layout


def _sha256(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _line_count(text: str) -> int:
    if not text:
        return 0
    return len(text.splitlines())


def build_memory_entry(
    *,
    group_label: str,
    kind: str,
    summary: str,
    actor_id: str = "",
    source_refs: Optional[List[str]] = None,
    tags: Optional[List[str]] = None,
    supersedes: Optional[List[str]] = None,
    entry_id: Optional[str] = None,
    created_at: Optional[str] = None,
    date: Optional[str] = None,
) -> Dict[str, Any]:
    ts = str(created_at or utc_now_iso())
    return {
        "entry_id": str(entry_id or f"mem_{uuid.uuid4().hex[:12]}"),
        "date": str(date or ts[:10]),
        "group_label": str(group_label or ""),
        "kind": str(kind or "note"),
        "summary": str(summary or "").strip(),
        "source_refs": list(source_refs or []),
        "tags": list(tags or []),
        "supersedes": list(supersedes or []),
        "actor_id": str(actor_id or ""),
        "created_at": ts,
    }


def _render_entry(entry: Dict[str, Any], *, idempotency_key: str = "") -> str:
    summary = str(entry.get("summary") or "").strip()
    content_hash = _sha256(summary)
    meta = {
        "entry_id": str(entry.get("entry_id") or ""),
        "kind": str(entry.get("kind") or ""),
        "date": str(entry.get("date") or ""),
        "group_label": str(entry.get("group_label") or ""),
        "actor_id": str(entry.get("actor_id") or ""),
        "created_at": str(entry.get("created_at") or ""),
        "source_refs": list(entry.get("source_refs") or []),
        "tags": list(entry.get("tags") or []),
        "supersedes": list(entry.get("supersedes") or []),
        "content_hash": content_hash,
    }
    if idempotency_key:
        meta["idempotency_key"] = idempotency_key

    meta_json = json.dumps(meta, ensure_ascii=False)
    return (
        f"## {meta.get('entry_id')} [{meta.get('kind')}] {meta.get('created_at')}\n"
        f"<!-- cccc.memory.meta {meta_json} -->\n\n"
        f"{summary}\n\n"
    )


def _append_with_idempotency(path: Path, block: str, *, idempotency_key: str = "") -> Dict[str, Any]:
    existing = path.read_text(encoding="utf-8", errors="replace") if path.exists() else ""
    if idempotency_key and idempotency_key in existing:
        return {
            "written": False,
            "status": "silent",
            "reason": "persistence_idempotency_key",
            "file_path": str(path),
            "bytes_written": 0,
            "line_count": _line_count(existing),
            "content_hash": _sha256(existing),
        }
    m = re.search(r'"content_hash"\s*:\s*"([0-9a-f]{64})"', block)
    if m:
        digest = str(m.group(1) or "")
        if re.search(rf'"content_hash"\s*:\s*"{digest}"', existing):
            return {
                "written": False,
                "status": "silent",
                "reason": "persistence_content_hash",
                "file_path": str(path),
                "bytes_written": 0,
                "line_count": _line_count(existing),
                "content_hash": _sha256(existing),
            }

    prefix = "" if existing.endswith("\n") or not existing else "\n"
    merged = f"{existing}{prefix}{block}"
    atomic_write_text(path, merged, encoding="utf-8")
    return {
        "written": True,
        "status": "written",
        "file_path": str(path),
        "bytes_written": len(block.encode("utf-8")),
        "line_count": _line_count(merged),
        "content_hash": _sha256(merged),
    }


def append_daily_entry(
    group_id: str,
    *,
    entry: Dict[str, Any],
    date: Optional[str] = None,
    idempotency_key: str = "",
) -> Dict[str, Any]:
    layout = resolve_memory_layout(group_id, date=date, ensure_files=True)
    block = _render_entry(entry, idempotency_key=idempotency_key)
    return _append_with_idempotency(layout.today_daily_file, block, idempotency_key=idempotency_key)


def append_memory_entry(
    group_id: str,
    *,
    entry: Dict[str, Any],
    idempotency_key: str = "",
) -> Dict[str, Any]:
    layout = resolve_memory_layout(group_id, ensure_files=True)
    block = _render_entry(entry, idempotency_key=idempotency_key)
    return _append_with_idempotency(layout.memory_file, block, idempotency_key=idempotency_key)


def write_raw_content(
    group_id: str,
    *,
    target: str,
    content: str,
    mode: str = "append",
    date: Optional[str] = None,
    idempotency_key: str = "",
) -> Dict[str, Any]:
    layout: MemoryLayout = resolve_memory_layout(group_id, date=date, ensure_files=True)
    normalized_target = str(target or "").strip().lower()
    normalized_mode = str(mode or "append").strip().lower()
    if normalized_target not in {"memory", "daily"}:
        raise ValueError("target must be 'memory' or 'daily'")
    if normalized_mode not in {"append", "replace"}:
        raise ValueError("mode must be 'append' or 'replace'")

    file_path = layout.memory_file if normalized_target == "memory" else layout.today_daily_file
    payload = str(content or "")
    if normalized_mode == "replace":
        atomic_write_text(file_path, payload, encoding="utf-8")
        return {
            "written": True,
            "status": "written",
            "file_path": str(file_path),
            "bytes_written": len(payload.encode("utf-8")),
            "line_count": _line_count(payload),
            "content_hash": _sha256(payload),
        }

    existing = file_path.read_text(encoding="utf-8", errors="replace") if file_path.exists() else ""
    if idempotency_key and idempotency_key in existing:
        return {
            "written": False,
            "status": "silent",
            "reason": "persistence_idempotency_key",
            "file_path": str(file_path),
            "bytes_written": 0,
            "line_count": _line_count(existing),
            "content_hash": _sha256(existing),
        }
    prefix = "" if existing.endswith("\n") or not existing else "\n"
    merged = f"{existing}{prefix}{payload}"
    atomic_write_text(file_path, merged, encoding="utf-8")
    return {
        "written": True,
        "status": "written",
        "file_path": str(file_path),
        "bytes_written": len(payload.encode("utf-8")),
        "line_count": _line_count(merged),
        "content_hash": _sha256(merged),
    }
