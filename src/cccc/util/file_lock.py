from __future__ import annotations

import logging
import os
from pathlib import Path
from typing import IO

_LOG = logging.getLogger("cccc.util.file_lock")


class LockUnavailableError(RuntimeError):
    """Raised when a non-blocking lock cannot be acquired."""


def _ensure_lock_region(f: IO[bytes]) -> None:
    """Ensure the lock file has at least 1 byte so region locks work on Windows."""
    try:
        f.seek(0, os.SEEK_END)
        if f.tell() <= 0:
            f.write(b"\0")
            f.flush()
        f.seek(0)
    except Exception as e:
        # Best-effort: even if this fails, locking may still work depending on platform.
        _LOG.debug("lockfile region init failed: %s", e)


def _lock_posix(fd: int, *, blocking: bool) -> None:
    import fcntl  # POSIX only

    flags = fcntl.LOCK_EX
    if not blocking:
        flags |= fcntl.LOCK_NB
    fcntl.flock(fd, flags)


def _unlock_posix(fd: int) -> None:
    import fcntl  # POSIX only

    fcntl.flock(fd, fcntl.LOCK_UN)


def _lock_windows(fd: int, *, blocking: bool) -> None:
    import msvcrt  # Windows only

    mode = msvcrt.LK_LOCK if blocking else msvcrt.LK_NBLCK
    msvcrt.locking(fd, mode, 1)


def _unlock_windows(fd: int) -> None:
    import msvcrt  # Windows only

    msvcrt.locking(fd, msvcrt.LK_UNLCK, 1)


def acquire_lockfile(path: Path, *, blocking: bool = True) -> IO[bytes]:
    """Open + lock a lockfile. Keep the returned handle open to hold the lock."""
    path.parent.mkdir(parents=True, exist_ok=True)
    # Use a non-append mode so callers can update lock contents (pid, etc).
    f = path.open("r+b") if path.exists() else path.open("w+b")
    _ensure_lock_region(f)
    try:
        if os.name == "nt":
            _lock_windows(f.fileno(), blocking=blocking)
        else:
            _lock_posix(f.fileno(), blocking=blocking)
    except BlockingIOError as e:
        try:
            f.close()
        except Exception:
            pass
        if not blocking:
            raise LockUnavailableError(str(e)) from e
        raise
    except OSError as e:
        try:
            f.close()
        except Exception:
            pass
        if not blocking:
            raise LockUnavailableError(str(e)) from e
        raise
    except Exception:
        try:
            f.close()
        except Exception:
            pass
        raise
    return f


def release_lockfile(f: IO[bytes]) -> None:
    """Release a lockfile acquired via acquire_lockfile (best-effort)."""
    # On Windows, msvcrt.locking() locks/unlocks from the current file position.
    # Seek to 0 so we reliably unlock the 1-byte region we lock in acquire_lockfile().
    try:
        f.seek(0)
    except Exception:
        pass
    try:
        if os.name == "nt":
            _unlock_windows(f.fileno())
        else:
            _unlock_posix(f.fileno())
    except Exception as e:
        _LOG.debug("lockfile unlock failed: %s", e)
    try:
        f.close()
    except Exception as e:
        _LOG.debug("lockfile close failed: %s", e)
