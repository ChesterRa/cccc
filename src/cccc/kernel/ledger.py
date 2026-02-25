from __future__ import annotations

import hashlib
import json
import logging
import os
import time
from pathlib import Path
from typing import Any, Callable, Dict, Iterable, Optional

from ..contracts.v1 import Event
from ..contracts.v1.event import normalize_event_data
from ..util.fs import atomic_write_text
from ..util.file_lock import acquire_lockfile, release_lockfile


MAX_EVENT_BYTES = 256_000
MAX_CHAT_TEXT_BYTES = 32_000

AppendHook = Callable[[Dict[str, Any]], None]

_APPEND_HOOK: Optional[AppendHook] = None
LOGGER = logging.getLogger(__name__)


def set_append_hook(hook: Optional[AppendHook]) -> None:
    """Set a best-effort callback invoked after a successful append_event().

    This is intended for in-process observers (e.g., daemon streaming) and MUST
    NOT be used as a correctness dependency (the ledger file is the source of truth).
    """
    global _APPEND_HOOK
    _APPEND_HOOK = hook


def _notify_append(event: Dict[str, Any]) -> None:
    hook = _APPEND_HOOK
    if hook is None:
        return
    try:
        hook(event)
    except Exception:
        return


def _spill_text(group_dir: Path, *, event_id: str, text: str) -> Dict[str, Any]:
    raw = text or ""
    b = raw.encode("utf-8", errors="replace")
    rel = Path("state") / "ledger" / "blobs" / f"chat.{event_id}.txt"
    abs_path = group_dir / rel
    atomic_write_text(abs_path, raw.rstrip("\n") + "\n")
    return {
        "kind": "text",
        "path": str(rel),
        "bytes": len(b),
        "sha256": hashlib.sha256(b).hexdigest(),
    }


def _lock_path(ledger_path: Path) -> Path:
    return ledger_path.parent / "state" / "ledger" / "ledger.lock"


def append_event(
    ledger_path: Path,
    *,
    kind: str,
    group_id: str,
    scope_key: str,
    by: str,
    data: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    payload = normalize_event_data(kind, data or {})
    event = Event(kind=kind, group_id=group_id, scope_key=scope_key, by=by, data=payload)

    # Hard rules: keep the ledger small and stable. Large payloads belong in files referenced from the ledger.
    if kind == "chat.message":
        text = event.data.get("text")
        if isinstance(text, str):
            b = text.encode("utf-8", errors="replace")
            if len(b) > MAX_CHAT_TEXT_BYTES:
                att = _spill_text(ledger_path.parent, event_id=event.id, text=text)
                event.data["text"] = f"[cccc] (chat text stored at {att.get('path')})"
                attachments = event.data.get("attachments")
                if not isinstance(attachments, list):
                    attachments = []
                attachments.append(att)
                event.data["attachments"] = attachments

    ledger_path.parent.mkdir(parents=True, exist_ok=True)
    out = event.model_dump()
    line = json.dumps(out, ensure_ascii=False)
    if len(line.encode("utf-8", errors="replace")) > MAX_EVENT_BYTES:
        raise ValueError(f"ledger event too large (>{MAX_EVENT_BYTES} bytes): {kind}")
    lock = _lock_path(ledger_path)
    lk = acquire_lockfile(lock, blocking=True)
    try:
        with ledger_path.open("a", encoding="utf-8") as f:
            f.write(line + "\n")
    finally:
        release_lockfile(lk)
    _notify_append(out)
    return out


def read_last_lines(path: Path, n: int) -> list[str]:
    if n <= 0:
        return []
    if not path.exists():
        return []
    try:
        with path.open("rb") as f:
            f.seek(0, 2)
            size = f.tell()
            block = 8192
            data = b""
            while size > 0 and data.count(b"\n") <= n:
                step = min(block, size)
                f.seek(size - step)
                data = f.read(step) + data
                size -= step
        lines = data.splitlines()[-n:]
        return [ln.decode("utf-8", errors="replace") for ln in lines]
    except Exception as e:
        LOGGER.warning("failed to read ledger tail with binary scan: path=%s err=%s", path, e)
        try:
            return path.read_text(encoding="utf-8", errors="replace").splitlines()[-n:]
        except Exception as e2:
            LOGGER.error("failed to read ledger tail fallback: path=%s err=%s", path, e2)
            return []


def follow(path: Path, *, sleep_seconds: float = 0.2) -> Iterable[str]:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch(exist_ok=True)
    inode = -1
    f = None

    def _open() -> None:
        nonlocal f, inode
        if f is not None:
            try:
                f.close()
            except Exception:
                pass
        f = path.open("r", encoding="utf-8", errors="replace")
        try:
            st = os.fstat(f.fileno())
            inode = int(getattr(st, "st_ino", -1) or -1)
        except Exception:
            inode = -1
        f.seek(0, 2)

    _open()
    assert f is not None

    while True:
        line = f.readline()
        if line:
            yield line.rstrip("\n")
            continue

        time.sleep(sleep_seconds)
        try:
            st = path.stat()
            cur_inode = int(getattr(st, "st_ino", -1) or -1)
            if inode != -1 and cur_inode != -1 and cur_inode != inode:
                _open()
                continue
            if st.st_size < f.tell():
                _open()
                continue
        except Exception:
            try:
                path.touch(exist_ok=True)
            except Exception:
                pass
            _open()
