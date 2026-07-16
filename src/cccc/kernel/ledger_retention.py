from __future__ import annotations

import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict

from ..util.fs import atomic_write_json, read_json
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.time import parse_utc_iso, utc_now_iso
from .group import Group
from .ledger import read_last_lines
from .ledger_state_snapshot import build_state_payload, current_ledger_basis
from .ledger_segments import (
    compress_sealed_segments,
    ensure_ledger_layout,
    load_ledger_manifest,
    rotate_active_ledger,
)


@dataclass(frozen=True)
class LedgerRetentionConfig:
    max_active_bytes: int = 5_000_000
    keep_recent_segments_uncompressed: int = 1
    min_interval_seconds: int = 300


def _cfg(group: Group) -> LedgerRetentionConfig:
    doc = group.doc.get("ledger")
    d = doc if isinstance(doc, dict) else {}

    def _int(key: str, default: int) -> int:
        try:
            v = int(d.get(key) if key in d else default)
        except Exception:
            v = int(default)
        return max(0, v)

    return LedgerRetentionConfig(
        max_active_bytes=_int("max_active_bytes", LedgerRetentionConfig.max_active_bytes),
        keep_recent_segments_uncompressed=_int(
            "keep_recent_segments_uncompressed",
            LedgerRetentionConfig.keep_recent_segments_uncompressed,
        ),
        min_interval_seconds=_int("min_interval_seconds", LedgerRetentionConfig.min_interval_seconds),
    )


def _stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def _ledger_lock_path(group: Group) -> Path:
    return group.path / "state" / "ledger" / "ledger.lock"


def snapshot(group: Group, *, reason: str = "manual") -> Dict[str, Any]:
    ensure_ledger_layout(group.path)
    active = group.ledger_path
    try:
        size = int(active.stat().st_size) if active.exists() else 0
    except Exception:
        size = 0
    last_event: Dict[str, Any] = {}
    last = read_last_lines(active, 1)
    if last:
        try:
            obj = json.loads(last[0])
            if isinstance(obj, dict):
                last_event = {k: obj.get(k) for k in ("id", "ts", "kind", "by")}
        except Exception:
            pass

    snap_dir = group.path / "state" / "ledger" / "snapshots"
    snap_dir.mkdir(parents=True, exist_ok=True)
    name = f"snapshot.{_stamp()}.json"
    p = snap_dir / name
    doc = {
        "v": 1,
        "kind": "ledger.snapshot",
        "group_id": group.group_id,
        "created_at": utc_now_iso(),
        "reason": str(reason or "manual"),
        "manifest": load_ledger_manifest(group.path),
        "ledger_basis": current_ledger_basis(group),
        "state": build_state_payload(group),
        "active_ledger": {"path": "ledger.jsonl", "size_bytes": size},
        "last_event": last_event,
    }
    atomic_write_json(p, doc)
    atomic_write_json(group.path / "state" / "ledger" / "snapshot.latest.json", doc)
    return {
        "snapshot_path": str(p),
        "size_bytes": size,
        "last_event": last_event,
        "manifest": doc.get("manifest"),
        "ledger_basis": doc.get("ledger_basis"),
    }


def compact(group: Group, *, reason: str = "auto", force: bool = False) -> Dict[str, Any]:
    active = group.ledger_path
    if not active.exists():
        return {"ok": False, "skipped": True, "reason": "missing_ledger"}

    cfg = _cfg(group)
    state_path = group.path / "state" / "ledger" / "compaction.json"
    state = read_json(state_path)
    if not isinstance(state, dict):
        state = {}

    if not force and cfg.min_interval_seconds > 0:
        last_ts = str(state.get("last_compacted_at") or "").strip()
        last_dt = parse_utc_iso(last_ts) if last_ts else None
        if last_dt is not None:
            age = (datetime.now(timezone.utc) - last_dt).total_seconds()
            if age < float(cfg.min_interval_seconds):
                return {"ok": True, "skipped": True, "reason": "min_interval"}

    size = int(active.stat().st_size)
    manifest = load_ledger_manifest(group.path)
    segments = [dict(item) for item in manifest.get("segments", []) if isinstance(item, dict)]
    eligible_for_compression = segments[:-max(0, int(cfg.keep_recent_segments_uncompressed or 0))]
    has_pending_compression = any(not bool(item.get("compressed")) for item in eligible_for_compression)
    if not force and cfg.max_active_bytes > 0 and size < int(cfg.max_active_bytes) and not has_pending_compression:
        return {"ok": True, "skipped": True, "reason": "below_threshold", "size_bytes": size}

    lock = _ledger_lock_path(group)
    lk = acquire_lockfile(lock, blocking=True)
    try:
        rotation = rotate_active_ledger(group.path, reason=reason)
        compressed = compress_sealed_segments(
            group.path,
            keep_recent=max(0, int(cfg.keep_recent_segments_uncompressed or 0)),
            force=force,
        )
        if not bool(rotation.get("rotated")) and int(compressed.get("count") or 0) <= 0:
            return {"ok": True, "skipped": True, "reason": str(rotation.get("reason") or "nothing_to_do")}
        result = {
            "v": 1,
            "last_compacted_at": utc_now_iso(),
            "reason": str(reason or "auto"),
            "rotation": rotation,
            "compression": compressed,
            "manifest": load_ledger_manifest(group.path),
        }
        atomic_write_json(state_path, result)
        snap = snapshot(group, reason=f"compact:{reason}")
        return {"ok": True, "skipped": False, "result": result, "snapshot": snap}
    finally:
        release_lockfile(lk)
