"""Cross-process authority helpers for group-local IM product state."""

from __future__ import annotations

import threading
from contextlib import contextmanager
from pathlib import Path
from typing import Any, Dict, Iterator

from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.fs import atomic_write_json
from .group import Group, load_group


_DURABLE_SHADOW_KEYS = ("config", "enabled", "authorized", "pending", "subscribers")
_STATE_FILES = (
    "im_authorized_chats.json",
    "im_pending_keys.json",
    "im_subscribers.json",
)
_LOCAL = threading.local()


def im_state_lock_path(state_dir: Path) -> Path:
    return Path(state_dir) / "im_state.lock"


@contextmanager
def im_state_lock(state_dir: Path) -> Iterator[None]:
    """Serialize IM state and allow same-thread compound transactions."""

    path = im_state_lock_path(state_dir).resolve(strict=False)
    held: Dict[Path, list[Any]] = getattr(_LOCAL, "held", {})
    if not hasattr(_LOCAL, "held"):
        _LOCAL.held = held
    current = held.get(path)
    if current is not None:
        current[1] += 1
        try:
            yield
        finally:
            current[1] -= 1
        return

    handle = acquire_lockfile(path, blocking=True)
    held[path] = [handle, 1]
    try:
        yield
    finally:
        held.pop(path, None)
        release_lockfile(handle)


def set_im_configuration(group_id: str, config: Dict[str, Any]) -> Group:
    """Replace canonical IM configuration under the shared state locks."""

    group = load_group(group_id)
    if group is None:
        raise ValueError(f"group not found: {group_id}")
    state_dir = group.path / "state"
    with im_state_lock(state_dir):
        current = load_group(group_id)
        if current is None:
            raise ValueError(f"group not found: {group_id}")
        current.doc["im"] = dict(config)
        current.save()
        return current


def set_im_enabled(group_id: str, enabled: bool) -> Group:
    """Update only the desired IM run state under the shared state locks."""

    group = load_group(group_id)
    if group is None:
        raise ValueError(f"group not found: {group_id}")
    state_dir = group.path / "state"
    with im_state_lock(state_dir):
        current = load_group(group_id)
        if current is None:
            raise ValueError(f"group not found: {group_id}")
        raw_config = current.doc.get("im")
        if not isinstance(raw_config, dict):
            return current
        config = dict(raw_config)
        config["enabled"] = bool(enabled)
        current.doc["im"] = config
        current.save()
        return current


def retire_im_configuration(group_id: str) -> Group:
    """Clear canonical IM authority and consume former Rust durable shadows."""

    group = load_group(group_id)
    if group is None:
        raise ValueError(f"group not found: {group_id}")
    state_dir = group.path / "state"
    with im_state_lock(state_dir):
        current = load_group(group_id)
        if current is None:
            raise ValueError(f"group not found: {group_id}")
        for name in _STATE_FILES:
            atomic_write_json(state_dir / name, {}, indent=2)
        current.doc.pop("im", None)
        shadow = current.doc.get("im_bridge")
        if isinstance(shadow, dict):
            for key in _DURABLE_SHADOW_KEYS:
                shadow.pop(key, None)
            if not shadow:
                current.doc.pop("im_bridge", None)
        current.save()
        return current
