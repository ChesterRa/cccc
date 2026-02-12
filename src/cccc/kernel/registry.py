from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Optional

from ..paths import ensure_home
from ..util.fs import atomic_write_json, read_json
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
        self.doc.setdefault("v", 1)
        self.doc["updated_at"] = utc_now_iso()
        atomic_write_json(self.path, self.doc)


def load_registry() -> Registry:
    home = ensure_home()
    path = home / "registry.json"
    raw = read_json(path)
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
    if dirty:
        atomic_write_json(path, doc)
    return Registry(path=path, doc=doc)


def default_group_id_for_scope(reg: Registry, scope_key: str) -> Optional[str]:
    return reg.defaults.get(scope_key) or None


def set_default_group_for_scope(reg: Registry, scope_key: str, group_id: str) -> None:
    reg.defaults[scope_key] = group_id
    reg.save()
