"""Cross-process serialization for the canonical Group Bridge file set."""

from __future__ import annotations

import inspect
import threading
from contextlib import contextmanager
from functools import wraps
from pathlib import Path
from typing import Callable, Iterator, Optional, ParamSpec, TypeVar

from ...paths import ensure_home
from ...util.file_lock import acquire_lockfile, release_lockfile

_P = ParamSpec("_P")
_R = TypeVar("_R")
_PROCESS_LOCK = threading.RLock()
_HELD_LOCKS = threading.local()


def _lock_path(home: Optional[Path] = None) -> Path:
    base = Path(home) if home is not None else ensure_home()
    return base / "group_bridge_state.lock"


@contextmanager
def group_bridge_state_lock(home: Optional[Path] = None) -> Iterator[None]:
    """Hold Rust's canonical Group Bridge lock, reentrantly within this process."""
    path = _lock_path(home).resolve()
    key = str(path)
    with _PROCESS_LOCK:
        held = getattr(_HELD_LOCKS, "value", None)
        if held is None:
            held = {}
            _HELD_LOCKS.value = held
        entry = held.get(key)
        if entry is None:
            handle = acquire_lockfile(path, blocking=True)
            held[key] = [handle, 1]
        else:
            entry[1] += 1
        try:
            yield
        finally:
            entry = held[key]
            entry[1] -= 1
            if entry[1] == 0:
                release_lockfile(entry[0])
                del held[key]


def serialized_group_bridge_state(function: Callable[_P, _R]) -> Callable[_P, _R]:
    """Run a store operation under the canonical lock named by its ``home`` argument."""
    signature = inspect.signature(function)

    @wraps(function)
    def wrapped(*args: _P.args, **kwargs: _P.kwargs) -> _R:
        bound = signature.bind_partial(*args, **kwargs)
        home = bound.arguments.get("home")
        with group_bridge_state_lock(Path(home) if home is not None else None):
            return function(*args, **kwargs)

    return wrapped
