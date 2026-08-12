from __future__ import annotations

from pathlib import Path
from typing import Any, Dict

from ..paths import ensure_home
from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso


def active_path() -> Path:
    return ensure_home() / "active.json"


def _write_committed(path: Path, document: Dict[str, Any]) -> None:
    try:
        atomic_write_json(path, document)
    except Exception:
        if read_json(path) != document:
            raise


def load_active() -> Dict[str, Any]:
    p = active_path()
    raw = read_json(p)
    doc = raw if isinstance(raw, dict) else {}
    if "active_group_id" in doc:
        active_group_id = str(doc.get("active_group_id") or "").strip()
    else:
        # Rust previews before 0.4.34-rc2 wrote the same shared file with the
        # shorter key. Preserve that selection while normalizing the document.
        active_group_id = str(doc.get("group_id") or "").strip()
    normalized = {
        "v": 1,
        "active_group_id": active_group_id,
        "updated_at": str(doc.get("updated_at") or utc_now_iso()),
    }
    return normalized


def set_active_group_id(group_id: str) -> Dict[str, Any]:
    p = active_path()
    doc = {"v": 1, "active_group_id": group_id.strip(), "updated_at": utc_now_iso()}
    _write_committed(p, doc)
    return doc
