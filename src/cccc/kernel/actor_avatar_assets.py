from __future__ import annotations

import hashlib
import mimetypes
from pathlib import Path
from typing import Any, Dict

from ..paths import ensure_home
from ..util.fs import atomic_write_bytes

_ACTOR_AVATAR_MAX_BYTES = 2 * 1024 * 1024

_ALLOWED_MIME_TYPES: set[str] = {
    "image/svg+xml",
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/avif",
    "image/x-icon",
    "image/vnd.microsoft.icon",
}

_EXTENSION_BY_MIME: dict[str, str] = {
    "image/svg+xml": ".svg",
    "image/png": ".png",
    "image/jpeg": ".jpg",
    "image/webp": ".webp",
    "image/gif": ".gif",
    "image/avif": ".avif",
    "image/x-icon": ".ico",
    "image/vnd.microsoft.icon": ".ico",
}


def actor_avatar_dir(group_id: str) -> Path:
    gid = str(group_id or "").strip()
    if not gid:
        raise ValueError("missing group_id")
    return ensure_home() / "groups" / gid / "state" / "actor_avatars"


def _actor_avatar_rel_path(group_id: str, filename: str) -> str:
    gid = str(group_id or "").strip()
    return str(Path("groups") / gid / "state" / "actor_avatars" / filename).replace("\\", "/")


def resolve_actor_avatar_path(rel_path: str) -> Path:
    normalized = str(rel_path or "").strip().replace("\\", "/")
    if not normalized:
        raise FileNotFoundError("actor avatar path is empty")
    base = ensure_home().resolve()
    target = (base / Path(*Path(normalized).parts)).resolve()
    try:
        target.relative_to(base)
    except ValueError as exc:
        raise FileNotFoundError("actor avatar path is outside CCCC_HOME") from exc
    return target


def store_actor_avatar(*, group_id: str, data: bytes, content_type: str, filename: str = "") -> Dict[str, Any]:
    mime_type = str(content_type or "").strip().lower()
    if not mime_type:
        guessed, _ = mimetypes.guess_type(str(filename or "").strip())
        mime_type = str(guessed or "").strip().lower()
    if mime_type not in _ALLOWED_MIME_TYPES:
        raise ValueError(f"unsupported avatar type: {mime_type or 'unknown'}")
    if len(data) > _ACTOR_AVATAR_MAX_BYTES:
        raise ValueError("avatar file too large")

    digest = hashlib.sha256(data).hexdigest()
    ext = _EXTENSION_BY_MIME.get(mime_type) or Path(str(filename or "").strip()).suffix.lower() or ".bin"
    stored_name = f"avatar_{digest[:16]}{ext}"
    abs_path = actor_avatar_dir(group_id) / stored_name
    atomic_write_bytes(abs_path, data)
    return {
        "mime_type": mime_type,
        "bytes": len(data),
        "sha256": digest,
        "rel_path": _actor_avatar_rel_path(group_id, stored_name),
    }


def delete_actor_avatar(rel_path: str) -> None:
    try:
        target = resolve_actor_avatar_path(rel_path)
        if target.exists():
            target.unlink()
    except Exception:
        pass
