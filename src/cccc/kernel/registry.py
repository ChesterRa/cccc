from __future__ import annotations

import copy
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, Optional

from ..paths import ensure_home
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.fs import atomic_write_json, merge_concurrent_document_changes, read_json
from ..util.time import utc_now_iso


def _new_registry_doc() -> Dict[str, Any]:
    now = utc_now_iso()
    return {
        "v": 1,
        "created_at": now,
        "updated_at": now,
        "groups": {},
        "defaults": {},
    }


@dataclass
class Registry:
    path: Path
    doc: Dict[str, Any]
    _baseline: Dict[str, Any] = field(init=False, repr=False, compare=False)

    def __post_init__(self) -> None:
        self._baseline = copy.deepcopy(self.doc)

    @property
    def groups(self) -> Dict[str, Any]:
        d = self.doc.get("groups")
        if not isinstance(d, dict):
            d = {}
            self.doc["groups"] = d
        return d

    @property
    def defaults(self) -> Dict[str, str]:
        d = self.doc.get("defaults")
        if not isinstance(d, dict):
            d = {}
            self.doc["defaults"] = d
        return d

    def save(self) -> None:
        lock = acquire_lockfile(self.path.with_suffix(".json.lock"), blocking=True)
        try:
            current = read_json(self.path) if self.path.exists() else copy.deepcopy(self._baseline)
            if not isinstance(current, dict):
                current = {}
            baseline = copy.deepcopy(self._baseline)
            desired = copy.deepcopy(self.doc)
            for document in (baseline, desired, current):
                document.pop("updated_at", None)
            expected = merge_concurrent_document_changes(
                baseline,
                desired,
                current,
                document=str(self.path),
            )
            expected.setdefault("v", 1)
            expected["updated_at"] = utc_now_iso()
            try:
                atomic_write_json(self.path, expected)
            except Exception:
                if read_json(self.path) != expected:
                    raise
            self.doc = copy.deepcopy(expected)
            self._baseline = copy.deepcopy(expected)
        finally:
            release_lockfile(lock)


def _normalize_registry_doc(raw: Any) -> tuple[Dict[str, Any], bool]:
    dirty = False
    if not isinstance(raw, dict) or not raw:
        doc = _new_registry_doc()
        dirty = True
    else:
        doc = dict(raw)
        if not isinstance(doc.get("groups"), dict):
            doc["groups"] = {}
            dirty = True
        if not isinstance(doc.get("defaults"), dict):
            doc["defaults"] = {}
            dirty = True
        if "v" not in doc:
            doc["v"] = 1
            dirty = True
        if not str(doc.get("created_at") or "").strip():
            doc["created_at"] = utc_now_iso()
            dirty = True
        if not str(doc.get("updated_at") or "").strip():
            doc["updated_at"] = utc_now_iso()
            dirty = True
    return doc, dirty


def load_registry() -> Registry:
    home = ensure_home()
    path = home / "registry.json"
    doc, dirty = _normalize_registry_doc(read_json(path))
    if dirty:
        lock = acquire_lockfile(path.with_suffix(".json.lock"), blocking=True)
        try:
            doc, dirty = _normalize_registry_doc(read_json(path))
            if dirty:
                atomic_write_json(path, doc)
        finally:
            release_lockfile(lock)
    return Registry(path=path, doc=doc)


def default_group_id_for_scope(reg: Registry, scope_key: str) -> Optional[str]:
    return reg.defaults.get(scope_key) or None


def set_default_group_for_scope(reg: Registry, scope_key: str, group_id: str) -> None:
    reg.defaults[scope_key] = group_id
    reg.save()
