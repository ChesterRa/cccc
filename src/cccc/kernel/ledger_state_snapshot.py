from __future__ import annotations

from pathlib import Path
from typing import Any, Dict
import hashlib

from ..util.fs import read_json
from .context import ContextStorage
from .group import Group
from .ledger_segments import active_ledger_path, load_ledger_manifest


def _safe_mtime_ns(path: Path) -> int:
    try:
        return max(0, int(path.stat().st_mtime_ns))
    except Exception:
        return 0


def _active_prefix_sha256(path: Path, *, limit_bytes: int = 4096) -> str:
    try:
        with path.open("rb") as handle:
            chunk = handle.read(max(0, int(limit_bytes or 0)))
    except Exception:
        chunk = b""
    return hashlib.sha256(chunk).hexdigest()


_ACTIVE_PREFIX_GUARD_BYTES = 4096


def current_ledger_basis(group: Group) -> Dict[str, Any]:
    manifest = load_ledger_manifest(group.path)
    segments = manifest.get("segments") if isinstance(manifest.get("segments"), list) else []
    segment_ids = [str(item.get("id") or "").strip() for item in segments if isinstance(item, dict) and str(item.get("id") or "").strip()]
    active = active_ledger_path(group.path)
    try:
        active_size = max(0, int(active.stat().st_size))
    except Exception:
        active_size = 0
    return {
        "segment_ids": segment_ids,
        "active_size": active_size,
        "active_mtime_ns": _safe_mtime_ns(active),
        "active_prefix_sha256": _active_prefix_sha256(active),
    }


def can_replay_from_basis(previous: Dict[str, Any], current: Dict[str, Any]) -> bool:
    prev_segments = [str(item).strip() for item in (previous.get("segment_ids") if isinstance(previous.get("segment_ids"), list) else []) if str(item).strip()]
    cur_segments = [str(item).strip() for item in (current.get("segment_ids") if isinstance(current.get("segment_ids"), list) else []) if str(item).strip()]
    if prev_segments != cur_segments:
        return False
    try:
        prev_size = max(0, int(previous.get("active_size") or 0))
        cur_size = max(0, int(current.get("active_size") or 0))
    except Exception:
        return False
    prev_prefix = str(previous.get("active_prefix_sha256") or "").strip()
    cur_prefix = str(current.get("active_prefix_sha256") or "").strip()
    if prev_size >= _ACTIVE_PREFIX_GUARD_BYTES or cur_size == prev_size:
        if prev_prefix != cur_prefix:
            return False
    return cur_size >= prev_size


def _read_state_file(path: Path) -> Dict[str, Any]:
    raw = read_json(path)
    return raw if isinstance(raw, dict) else {}


def build_state_payload(group: Group) -> Dict[str, Any]:
    context = ContextStorage(group)
    unread_index = _read_state_file(group.path / "state" / "unread_index.json")
    if unread_index:
        unread_index["ledger_basis"] = current_ledger_basis(group)
    return {
        "version_state": context.load_version_state(),
        "cursors": _read_state_file(group.path / "state" / "read_cursors.json"),
        "unread_index": unread_index,
    }


def load_latest_ledger_snapshot(group: Group) -> Dict[str, Any]:
    raw = read_json(group.path / "state" / "ledger" / "snapshot.latest.json")
    return raw if isinstance(raw, dict) else {}
