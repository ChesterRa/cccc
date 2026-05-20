from __future__ import annotations

import gzip
import json
import os
import re
from collections import deque
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Deque, Dict, Iterable, Iterator, List

from ..util.fs import atomic_write_json


_MANIFEST_SCHEMA = 1
ACTIVE_SOURCE_SEQ = 1_000_000_000
_SEGMENT_FILE_RE = re.compile(r"^ledger\.(?P<stamp>\d{8}T\d{6}Z)\.(?P<seq>\d{6})\.jsonl(?P<gz>\.gz)?$")


def _stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def ledger_state_dir(group_path: Path) -> Path:
    return group_path / "state" / "ledger"


def ledger_segments_dir(group_path: Path) -> Path:
    return ledger_state_dir(group_path) / "segments"


def ledger_manifest_path(group_path: Path) -> Path:
    return ledger_state_dir(group_path) / "manifest.json"


def active_ledger_path(group_path: Path) -> Path:
    return group_path / "ledger.jsonl"


def _segment_entry_from_path(group_path: Path, path: Path, *, count_lines: bool = False) -> Dict[str, Any] | None:
    match = _SEGMENT_FILE_RE.match(path.name)
    if match is None:
        return None
    seq = int(match.group("seq") or 0)
    rel_path = str(path.relative_to(group_path))
    compressed = bool(match.group("gz"))
    try:
        size_bytes = max(0, int(path.stat().st_size))
    except Exception:
        size_bytes = 0
    line_count = 0
    if count_lines:
        try:
            with open_ledger_source_text(path) as handle:
                for _ in handle:
                    line_count += 1
        except Exception:
            line_count = 0
    stamp = str(match.group("stamp") or "")
    return {
        "id": f"{seq:06d}",
        "seq": seq,
        "path": rel_path,
        "compressed": compressed,
        "created_at": stamp,
        "sealed_at": stamp,
        "reason": "recovered",
        "size_bytes": size_bytes,
        "line_count": line_count,
    }


def _discover_segment_entries(group_path: Path) -> Dict[int, Dict[str, Any]]:
    out: Dict[int, Dict[str, Any]] = {}
    seg_dir = ledger_segments_dir(group_path)
    if not seg_dir.exists():
        return out
    for path in sorted(seg_dir.glob("ledger.*.jsonl*")):
        if not path.is_file():
            continue
        entry = _segment_entry_from_path(group_path, path)
        if entry is None:
            continue
        seq = int(entry.get("seq") or 0)
        if seq <= 0:
            continue
        prev = out.get(seq)
        if prev is None:
            out[seq] = entry
            continue
        # Prefer compressed artifacts when both variants exist after an interrupted compression.
        if bool(entry.get("compressed")) and not bool(prev.get("compressed")):
            out[seq] = entry
    return out


def _normalize_manifest_segments(group_path: Path, segments: List[Dict[str, Any]]) -> tuple[List[Dict[str, Any]], bool]:
    discovered = _discover_segment_entries(group_path)
    by_seq: Dict[int, Dict[str, Any]] = {}
    changed = False
    for item in segments:
        if not isinstance(item, dict):
            changed = True
            continue
        seq = int(item.get("seq") or 0)
        if seq <= 0:
            changed = True
            continue
        normalized = dict(item)
        discovered_entry = discovered.get(seq)
        if discovered_entry is not None:
            if str(normalized.get("path") or "") != str(discovered_entry.get("path") or ""):
                normalized["path"] = discovered_entry["path"]
                changed = True
            if bool(normalized.get("compressed")) != bool(discovered_entry.get("compressed")):
                normalized["compressed"] = bool(discovered_entry.get("compressed"))
                changed = True
            normalized["size_bytes"] = int(discovered_entry.get("size_bytes") or 0)
            if int(normalized.get("line_count") or 0) <= 0:
                normalized["line_count"] = int(discovered_entry.get("line_count") or 0)
            if not str(normalized.get("created_at") or "").strip():
                normalized["created_at"] = str(discovered_entry.get("created_at") or "")
                changed = True
            if not str(normalized.get("sealed_at") or "").strip():
                normalized["sealed_at"] = str(discovered_entry.get("sealed_at") or "")
                changed = True
        else:
            path = group_path / str(normalized.get("path") or "")
            if not path.exists():
                changed = True
                continue
        by_seq[seq] = normalized
    for seq, entry in discovered.items():
        if seq not in by_seq:
            by_seq[seq] = dict(entry)
            changed = True
    normalized_segments = [by_seq[seq] for seq in sorted(by_seq)]
    return normalized_segments, changed


def ensure_ledger_layout(group_path: Path) -> Dict[str, Any]:
    group_path.mkdir(parents=True, exist_ok=True)
    ledger_state_dir(group_path).mkdir(parents=True, exist_ok=True)
    ledger_segments_dir(group_path).mkdir(parents=True, exist_ok=True)
    active_ledger_path(group_path).touch(exist_ok=True)
    manifest_path = ledger_manifest_path(group_path)
    if manifest_path.exists():
        return load_ledger_manifest(group_path)
    manifest = {
        "schema": _MANIFEST_SCHEMA,
        "active": {"path": "ledger.jsonl"},
        "next_segment_seq": 1,
        "segments": [],
        "updated_at": "",
    }
    atomic_write_json(manifest_path, manifest, indent=2)
    return manifest


def load_ledger_manifest(group_path: Path) -> Dict[str, Any]:
    manifest_path = ledger_manifest_path(group_path)
    if not manifest_path.exists():
        return ensure_ledger_layout(group_path)
    try:
        doc = json.loads(manifest_path.read_text(encoding="utf-8"))
    except Exception:
        doc = {}
    if not isinstance(doc, dict):
        doc = {}
    schema = int(doc.get("schema") or 0)
    if schema != _MANIFEST_SCHEMA:
        doc = {}
    active = doc.get("active") if isinstance(doc.get("active"), dict) else {"path": "ledger.jsonl"}
    segments = doc.get("segments") if isinstance(doc.get("segments"), list) else []
    normalized_segments, healed = _normalize_manifest_segments(group_path, [dict(item) for item in segments if isinstance(item, dict)])
    next_seq = max(1, int(doc.get("next_segment_seq") or 1))
    if normalized_segments:
        next_seq = max(next_seq, max(int(item.get("seq") or 0) for item in normalized_segments) + 1)
    out = {
        "schema": _MANIFEST_SCHEMA,
        "active": {"path": str(active.get("path") or "ledger.jsonl")},
        "next_segment_seq": next_seq,
        "segments": normalized_segments,
        "updated_at": str(doc.get("updated_at") or ""),
    }
    if healed:
        out["updated_at"] = _stamp()
    if healed:
        atomic_write_json(manifest_path, out, indent=2)
    elif not manifest_path.exists():
        atomic_write_json(manifest_path, out, indent=2)
    return out


def save_ledger_manifest(group_path: Path, manifest: Dict[str, Any]) -> Dict[str, Any]:
    ensure_ledger_layout(group_path)
    out = {
        "schema": _MANIFEST_SCHEMA,
        "active": {
            "path": str(((manifest.get("active") if isinstance(manifest.get("active"), dict) else {}) or {}).get("path") or "ledger.jsonl"),
        },
        "next_segment_seq": max(1, int(manifest.get("next_segment_seq") or 1)),
        "segments": [dict(item) for item in (manifest.get("segments") if isinstance(manifest.get("segments"), list) else []) if isinstance(item, dict)],
        "updated_at": str(manifest.get("updated_at") or ""),
    }
    atomic_write_json(ledger_manifest_path(group_path), out, indent=2)
    return out


def _segment_rel_path(*, stamp: str, seq: int, compressed: bool) -> str:
    base = f"state/ledger/segments/ledger.{stamp}.{int(seq):06d}.jsonl"
    return base + (".gz" if compressed else "")


def list_sealed_segments(group_path: Path) -> List[Dict[str, Any]]:
    manifest = load_ledger_manifest(group_path)
    items: List[Dict[str, Any]] = []
    for item in manifest.get("segments", []):
        if not isinstance(item, dict):
            continue
        rel_path = str(item.get("path") or "").strip()
        if not rel_path:
            continue
        abs_path = group_path / rel_path
        items.append({**dict(item), "path": rel_path, "abs_path": abs_path})
    return items


def list_ledger_sources(group_path: Path, *, include_active: bool = True) -> List[Dict[str, Any]]:
    ensure_ledger_layout(group_path)
    sources: List[Dict[str, Any]] = []
    for item in list_sealed_segments(group_path):
        abs_path = item.get("abs_path")
        if isinstance(abs_path, Path) and abs_path.exists():
            sources.append(
                {
                    "kind": "segment",
                    "path": str(item.get("path") or ""),
                    "abs_path": abs_path,
                    "compressed": bool(item.get("compressed")),
                    "seq": int(item.get("seq") or 0),
                }
            )
    if include_active:
        active = active_ledger_path(group_path)
        sources.append({"kind": "active", "path": "ledger.jsonl", "abs_path": active, "compressed": False, "seq": ACTIVE_SOURCE_SEQ})
    return sources


def rotate_active_ledger(group_path: Path, *, reason: str = "auto") -> Dict[str, Any]:
    ensure_ledger_layout(group_path)
    active = active_ledger_path(group_path)
    if not active.exists():
        active.touch(exist_ok=True)
    try:
        size_bytes = int(active.stat().st_size)
    except Exception:
        size_bytes = 0
    if size_bytes <= 0:
        return {"rotated": False, "reason": "empty_active"}
    manifest = load_ledger_manifest(group_path)
    seq = max(1, int(manifest.get("next_segment_seq") or 1))
    stamp = _stamp()
    rel_path = _segment_rel_path(stamp=stamp, seq=seq, compressed=False)
    dst = group_path / rel_path
    dst.parent.mkdir(parents=True, exist_ok=True)
    os.replace(active, dst)
    active.touch(exist_ok=True)
    line_count = 0
    try:
        with dst.open("r", encoding="utf-8", errors="replace") as handle:
            for _ in handle:
                line_count += 1
    except Exception:
        line_count = 0
    segment = {
        "id": f"{seq:06d}",
        "seq": seq,
        "path": rel_path,
        "compressed": False,
        "created_at": stamp,
        "sealed_at": stamp,
        "reason": str(reason or "auto"),
        "size_bytes": size_bytes,
        "line_count": line_count,
    }
    manifest["segments"] = [*manifest.get("segments", []), segment]
    manifest["next_segment_seq"] = seq + 1
    manifest["updated_at"] = stamp
    save_ledger_manifest(group_path, manifest)
    return {"rotated": True, "segment": segment}


def compress_sealed_segments(
    group_path: Path,
    *,
    keep_recent: int = 1,
    force: bool = False,
) -> Dict[str, Any]:
    ensure_ledger_layout(group_path)
    manifest = load_ledger_manifest(group_path)
    segments = [dict(item) for item in manifest.get("segments", []) if isinstance(item, dict)]
    compressed: List[str] = []
    eligible = segments if force else segments[:-max(0, int(keep_recent or 0))]
    for item in eligible:
        if bool(item.get("compressed")):
            continue
        rel_path = str(item.get("path") or "").strip()
        if not rel_path:
            continue
        src = group_path / rel_path
        if not src.exists():
            continue
        dst_rel = rel_path + ".gz"
        dst = group_path / dst_rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        with src.open("rb") as in_handle, gzip.open(dst, "wb", compresslevel=6) as out_handle:
            while True:
                chunk = in_handle.read(1024 * 1024)
                if not chunk:
                    break
                out_handle.write(chunk)
        try:
            size_bytes = int(dst.stat().st_size)
        except Exception:
            size_bytes = 0
        src.unlink(missing_ok=True)
        item["path"] = dst_rel
        item["compressed"] = True
        item["size_bytes"] = size_bytes
        compressed.append(str(item.get("id") or rel_path))
    manifest["segments"] = segments
    manifest["updated_at"] = _stamp()
    save_ledger_manifest(group_path, manifest)
    return {"compressed_segments": compressed, "count": len(compressed)}


def open_ledger_source_text(path: Path):
    if str(path.name).endswith(".gz"):
        return gzip.open(path, "rt", encoding="utf-8", errors="replace")
    return path.open("r", encoding="utf-8", errors="replace")


def iter_source_lines(path: Path) -> Iterator[str]:
    if not path.exists():
        return
    with open_ledger_source_text(path) as handle:
        for line in handle:
            yield line


def read_last_lines_across_sources(group_path: Path, n: int) -> List[str]:
    if n <= 0:
        return []
    keep: Deque[str] = deque(maxlen=n)
    for source in list_ledger_sources(group_path):
        abs_path = source.get("abs_path")
        if not isinstance(abs_path, Path) or not abs_path.exists():
            continue
        for raw_line in iter_source_lines(abs_path):
            line = raw_line.rstrip("\n")
            if line:
                keep.append(line)
    return list(keep)
