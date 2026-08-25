from __future__ import annotations

import threading
from contextlib import contextmanager
from functools import wraps
from pathlib import Path
from typing import Any, Callable, Dict, Iterator

from ...kernel.context import ContextStorage
from ...kernel.group import load_group
from ...util.file_lock import acquire_lockfile, release_lockfile

_LOCKS_GUARD = threading.Lock()
_PROCESS_LOCKS: Dict[str, threading.RLock] = {}
_HELD_LOCKS = threading.local()


def _lock_path(storage: Any) -> Path:
    # Rust ContextStore uses the same advisory lock for reads and writes.
    return (Path(storage.context_dir) / ".rust-context.lock").resolve()


def _process_lock(path: Path) -> threading.RLock:
    key = str(path)
    with _LOCKS_GUARD:
        return _PROCESS_LOCKS.setdefault(key, threading.RLock())


@contextmanager
def context_state_lock(storage: Any) -> Iterator[None]:
    """Serialize one group's context snapshot across Python threads and Rust."""
    path = _lock_path(storage)
    key = str(path)
    with _process_lock(path):
        held = getattr(_HELD_LOCKS, "value", None)
        if held is None:
            held = {}
            _HELD_LOCKS.value = held
        entry = held.get(key)
        if entry is None:
            entry = [acquire_lockfile(path, blocking=True), 0]
            held[key] = entry
        entry[1] += 1
        try:
            yield
        finally:
            entry[1] -= 1
            if entry[1] == 0:
                release_lockfile(entry[0])
                del held[key]


def serialized_context_state(function: Callable[..., Any]) -> Callable[..., Any]:
    """Run a daemon context handler under its group's canonical state lock."""

    @wraps(function)
    def wrapped(args: Dict[str, Any], *positional: Any, **keywords: Any) -> Any:
        group_id = str(args.get("group_id") or "").strip()
        group = load_group(group_id) if group_id else None
        if group is None:
            return function(args, *positional, **keywords)
        with context_state_lock(ContextStorage(group)):
            return function(args, *positional, **keywords)

    return wrapped
