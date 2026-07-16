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
from .ledger_index import append_event_to_index
from .ledger_segments import read_last_lines_across_sources


MAX_EVENT_BYTES = 256_000
MAX_CHAT_TEXT_BYTES = 32_000

AppendHook = Callable[[Dict[str, Any]], None]

_APPEND_HOOK: Optional[AppendHook] = None
LOGGER = logging.getLogger(__name__)
_TAIL_READ_BLOCK_SIZE = 8192
_TAIL_READ_MAX_BLOCK_SIZE = 1024 * 1024


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
            start_offset = int(f.tell() or 0)
            f.write(line + "\n")
            next_offset = start_offset + len((line + "\n").encode("utf-8", errors="replace"))
    finally:
        release_lockfile(lk)
    try:
        append_event_to_index(ledger_path, out, next_offset_bytes=next_offset)
    except Exception:
        pass
    try:
        from .ledger_status_cache import update_message_status_cache_on_append

        update_message_status_cache_on_append(out)
    except Exception:
        pass
    _notify_append(out)
    return out


def read_last_lines(path: Path, n: int) -> list[str]:
    if n <= 0:
        return []
    if path.name == "ledger.jsonl" and (path.parent / "group.yaml").exists():
        try:
            return read_last_lines_across_sources(path.parent, n)
        except Exception as e:
            LOGGER.warning("failed to read ledger tail across sources: path=%s err=%s", path, e)
    try:
        if not path.exists():
            return []
        return _read_last_lines_from_regular_file(path, n)
    except Exception as e:
        LOGGER.error("failed to read text tail: path=%s err=%s", path, e)
        return []


def _read_last_lines_from_regular_file(path: Path, n: int, *, block_size: int = _TAIL_READ_BLOCK_SIZE) -> list[str]:
    if n <= 0:
        return []

    chunks: list[bytes] = []
    position = path.stat().st_size
    size = max(1, int(block_size))
    with path.open("rb") as handle:
        while position > 0:
            read_size = min(size, position)
            position -= read_size
            handle.seek(position)
            chunks.insert(0, handle.read(read_size))

            data = b"".join(chunks)
            candidate = data
            if position > 0:
                first_newline = candidate.find(b"\n")
                candidate = candidate[first_newline + 1 :] if first_newline >= 0 else b""
            parts = candidate.split(b"\n")
            if parts and parts[-1] == b"":
                parts = parts[:-1]
            if sum(1 for part in parts if part) >= n:
                break
            size = min(size * 2, _TAIL_READ_MAX_BLOCK_SIZE)

    data = b"".join(chunks)
    if position > 0:
        first_newline = data.find(b"\n")
        data = data[first_newline + 1 :] if first_newline >= 0 else b""
    parts = data.split(b"\n")
    if parts and parts[-1] == b"":
        parts = parts[:-1]
    lines = [part.decode("utf-8", errors="replace") for part in parts if part]
    return lines[-n:]


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
