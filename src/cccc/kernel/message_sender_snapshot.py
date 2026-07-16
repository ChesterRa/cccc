from __future__ import annotations

import mimetypes
from pathlib import Path
from typing import Any, Dict

from .actor_avatar_assets import resolve_actor_avatar_path
from .actors import find_actor
from .blobs import store_blob_bytes
from .group import Group


def _snapshot_actor_avatar_blob_path(group: Group, actor: Dict[str, Any]) -> str:
    rel_path = str(actor.get("avatar_asset_path") or "").strip()
    if not rel_path:
        return ""
    try:
        abs_path = resolve_actor_avatar_path(rel_path)
        raw = abs_path.read_bytes()
    except Exception:
        return ""
    filename = Path(rel_path).name or f"{str(actor.get('id') or 'avatar').strip() or 'avatar'}.bin"
    mime_type = str(mimetypes.guess_type(filename)[0] or "").strip()
    try:
        stored = store_blob_bytes(
            group,
            data=raw,
            filename=filename,
            mime_type=mime_type,
            kind="image",
        )
    except Exception:
        return ""
    return str(stored.get("path") or "").strip()


def build_sender_snapshot(group: Group, *, by: str) -> Dict[str, Any]:
    sender_id = str(by or "").strip()
    if not sender_id:
        return {}
    actor = find_actor(group, sender_id)
    if not isinstance(actor, dict):
        return {}

    sender_title = str(actor.get("title") or "").strip()
    sender_runtime = str(actor.get("runtime") or "").strip()
    sender_avatar_path = _snapshot_actor_avatar_blob_path(group, actor)
    out: Dict[str, Any] = {}
    if sender_title:
        out["sender_title"] = sender_title
    if sender_runtime:
        out["sender_runtime"] = sender_runtime
    if sender_avatar_path:
        out["sender_avatar_path"] = sender_avatar_path
    return out


def copy_sender_snapshot(data: Any) -> Dict[str, Any]:
    if not isinstance(data, dict):
        return {}
    out: Dict[str, Any] = {}
    for key in ("sender_title", "sender_runtime", "sender_avatar_path"):
        value = str(data.get(key) or "").strip()
        if value:
            out[key] = value
    return out
