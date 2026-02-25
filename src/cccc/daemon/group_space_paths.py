from __future__ import annotations

from pathlib import Path
from typing import Any, Optional

from ..kernel.group import load_group

SPACE_DIR_NAME = "space"
SPACE_INDEX_FILENAME = ".space-index.json"
SPACE_STATE_FILENAME = ".space-sync-state.json"
SPACE_STATUS_FILENAME = ".space-status.json"


def resolve_scope_root_from_group(group: Any) -> Optional[Path]:
    scopes = group.doc.get("scopes") if isinstance(group.doc.get("scopes"), list) else []
    active_scope_key = str(group.doc.get("active_scope_key") or "").strip()
    candidates: list[str] = []
    for item in scopes:
        if not isinstance(item, dict):
            continue
        url = str(item.get("url") or "").strip()
        scope_key = str(item.get("scope_key") or "").strip()
        if not url:
            continue
        if active_scope_key and scope_key == active_scope_key:
            candidates.insert(0, url)
        else:
            candidates.append(url)
    for raw in candidates:
        try:
            root = Path(raw).expanduser().resolve()
        except Exception:
            continue
        if root.exists() and root.is_dir():
            return root
    return None


def resolve_scope_root(group_id: str) -> Optional[Path]:
    gid = str(group_id or "").strip()
    if not gid:
        return None
    group = load_group(gid)
    if group is None:
        return None
    return resolve_scope_root_from_group(group)


def resolve_space_root_from_group(group: Any, *, create: bool = False) -> Optional[Path]:
    scope_root = resolve_scope_root_from_group(group)
    if scope_root is None:
        return None
    space_root = scope_root / SPACE_DIR_NAME
    if create:
        try:
            space_root.mkdir(parents=True, exist_ok=True)
        except Exception:
            return None
    return space_root


def resolve_space_root(group_id: str, *, create: bool = False) -> Optional[Path]:
    gid = str(group_id or "").strip()
    if not gid:
        return None
    group = load_group(gid)
    if group is None:
        return None
    return resolve_space_root_from_group(group, create=create)


def space_index_path(space_root: Path) -> Path:
    return space_root / SPACE_INDEX_FILENAME


def space_state_path(space_root: Path) -> Path:
    return space_root / SPACE_STATE_FILENAME


def space_status_path(space_root: Path) -> Path:
    return space_root / SPACE_STATUS_FILENAME

