from __future__ import annotations

from pathlib import Path
from typing import Any, Dict

from ..paths import ensure_home
from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso


def active_path() -> Path:
    return ensure_home() / "active.json"


def load_active() -> Dict[str, Any]:
    p = active_path()
    raw = read_json(p)
    doc = raw if isinstance(raw, dict) else {}
    normalized = {
        "v": 1,
        "active_group_id": str(doc.get("active_group_id") or "").strip(),
        "updated_at": str(doc.get("updated_at") or utc_now_iso()),
    }
    if doc != normalized:
        atomic_write_json(p, normalized)
    return normalized


def set_active_group_id(group_id: str) -> Dict[str, Any]:
    p = active_path()
    doc = {"v": 1, "active_group_id": group_id.strip(), "updated_at": utc_now_iso()}
    atomic_write_json(p, doc)
    return doc
