from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, List

from ..paths import ensure_home
from ..util.fs import atomic_write_json, read_json
from .actors import get_effective_role, list_actors, list_visible_actors
from .context import ContextStorage
from .group import Group, load_group
from .registry import Registry, load_registry


_GROUPS_SCHEMA = 1
_ACTORS_SCHEMA = 1


def _safe_mtime_ns(path: Path) -> int:
    try:
        return max(0, int(path.stat().st_mtime_ns))
    except Exception:
        return 0


def _groups_projection_path() -> Path:
    return ensure_home() / "state" / "projections" / "groups.json"


def _actors_projection_path(group: Group) -> Path:
    return group.path / "state" / "projections" / "actors.json"


def _registry_group_yaml_path(group_id: str, meta: Dict[str, Any]) -> Path:
    gid = str(group_id or "").strip()
    raw_path = str(meta.get("path") or "").strip() if isinstance(meta, dict) else ""
    if raw_path:
        return Path(raw_path).expanduser() / "group.yaml"
    return ensure_home() / "groups" / gid / "group.yaml"


def _groups_basis(reg: Registry) -> Dict[str, Any]:
    out: Dict[str, Any] = {}
    for group_id, meta in reg.groups.items():
        gid = str(group_id or "").strip()
        if not gid or not isinstance(meta, dict):
            continue
        out[gid] = {"group_yaml_mtime_ns": _safe_mtime_ns(_registry_group_yaml_path(gid, meta))}
    return {
        "registry_mtime_ns": _safe_mtime_ns(reg.path),
        "groups": out,
    }


def _actors_basis(group: Group) -> Dict[str, Any]:
    try:
        actors_rev = max(0, int(ContextStorage(group).load_version_state().get("actors_rev") or 0))
    except Exception:
        actors_rev = 0
    return {
        "group_yaml_mtime_ns": _safe_mtime_ns(group.path / "group.yaml"),
        "actors_rev": actors_rev,
    }


def _load_snapshot(path: Path) -> Dict[str, Any]:
    raw = read_json(path)
    return raw if isinstance(raw, dict) else {}


def _save_snapshot(path: Path, *, schema: int, basis: Dict[str, Any], result: Dict[str, Any]) -> Dict[str, Any]:
    snapshot = {"schema": schema, "basis": basis, "result": result}
    atomic_write_json(path, snapshot, indent=2)
    return result


def get_groups_projection() -> Dict[str, Any]:
    reg = load_registry()
    basis = _groups_basis(reg)
    path = _groups_projection_path()
    snapshot = _load_snapshot(path)
    if (
        int(snapshot.get("schema") or 0) == _GROUPS_SCHEMA
        and isinstance(snapshot.get("basis"), dict)
        and snapshot.get("basis") == basis
        and isinstance(snapshot.get("result"), dict)
    ):
        return dict(snapshot.get("result") or {})

    groups = list(reg.groups.values())
    groups.sort(key=lambda g: (g.get("updated_at") or "", g.get("created_at") or ""), reverse=True)
    out: List[Dict[str, Any]] = []
    missing_ids: List[str] = []
    corrupt_ids: List[str] = []

    for group_meta in groups:
        if not isinstance(group_meta, dict):
            continue
        gid = str(group_meta.get("group_id") or "").strip()
        if not gid:
            continue
        item = dict(group_meta)
        group_yaml = _registry_group_yaml_path(gid, group_meta)
        if not group_yaml.exists():
            missing_ids.append(gid)
            item["registry_health"] = "missing"
            continue
        group = load_group(gid)
        if group is None:
            corrupt_ids.append(gid)
            item["registry_health"] = "corrupt"
            continue
        item["registry_health"] = "ok"
        item["state"] = str(group.doc.get("state") or "active")
        out.append(item)

    result = {
        "groups": out,
        "registry_health": {
            "missing_group_ids": missing_ids,
            "corrupt_group_ids": corrupt_ids,
        },
    }
    return _save_snapshot(path, schema=_GROUPS_SCHEMA, basis=basis, result=result)


def get_actor_list_projection(group: Group) -> List[Dict[str, Any]]:
    basis = _actors_basis(group)
    path = _actors_projection_path(group)
    snapshot = _load_snapshot(path)
    if (
        int(snapshot.get("schema") or 0) == _ACTORS_SCHEMA
        and isinstance(snapshot.get("basis"), dict)
        and snapshot.get("basis") == basis
        and isinstance(snapshot.get("result"), dict)
        and isinstance(snapshot.get("result", {}).get("actors"), list)
    ):
        return [dict(item) for item in snapshot.get("result", {}).get("actors", []) if isinstance(item, dict)]

    actors_out: List[Dict[str, Any]] = []
    for actor in list_visible_actors(group):
        if not isinstance(actor, dict):
            continue
        aid = str(actor.get("id") or "").strip()
        if not aid:
            continue
        item = dict(actor)
        item["role"] = get_effective_role(group, aid)
        actors_out.append(item)

    _save_snapshot(path, schema=_ACTORS_SCHEMA, basis=basis, result={"actors": actors_out})
    return [dict(item) for item in actors_out]
