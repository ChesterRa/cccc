"""SSE streaming utilities for the web port."""
from __future__ import annotations

import asyncio
import os
import time
from pathlib import Path
from typing import AsyncIterator, Dict, Optional, Set, TextIO, Tuple

from starlette.responses import StreamingResponse


async def sse_jsonl_tail(path: Path, *, event_name: str, heartbeat_s: float = 30.0) -> AsyncIterator[bytes]:
    """Tail a JSONL file and yield SSE events for each appended line."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.touch(exist_ok=True)

    inode = -1
    f: TextIO | None = None
    last_send = time.monotonic()

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

    yield b": connected\n\n"

    while True:
        line = f.readline()
        if line:
            raw = line.rstrip("\n")
            if raw:
                yield f"event: {event_name}\n".encode("utf-8")
                yield b"data: " + raw.encode("utf-8", errors="replace") + b"\n\n"
                last_send = time.monotonic()
            continue

        now = time.monotonic()
        if heartbeat_s > 0 and now - last_send >= heartbeat_s:
            yield b": heartbeat\n\n"
            last_send = now

        await asyncio.sleep(0.2)
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


async def sse_ledger_tail(path: Path) -> AsyncIterator[bytes]:
    async for item in sse_jsonl_tail_shared(path, event_name="ledger", heartbeat_s=30.0):
        yield item


async def sse_global_events_tail(home: Path | None = None) -> AsyncIterator[bytes]:
    from ...kernel.events import global_events_path

    path = global_events_path(home)
    async for item in sse_jsonl_tail_shared(path, event_name="event", heartbeat_s=30.0):
        yield item


def create_sse_response(generator: AsyncIterator[bytes]) -> StreamingResponse:
    """Create a properly configured SSE StreamingResponse."""
    return StreamingResponse(
        generator,
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "X-Accel-Buffering": "no",
            "Connection": "keep-alive",
        },
    )


# -----------------------------------------------------------------------------
# Shared tailer (fan-out) for SSE scalability
# -----------------------------------------------------------------------------


class _SharedJSONLTailer:
    def __init__(self, path: Path, *, event_name: str, heartbeat_s: float) -> None:
        self._path = path
        self._event_name = event_name
        self._heartbeat_s = float(heartbeat_s)
        self._inode: int = -1
        self._f: TextIO | None = None
        self._last_send = time.monotonic()
        self._subscribers: Set[asyncio.Queue[bytes | None]] = set()
        self._task: Optional[asyncio.Task[None]] = None
        self._has_subscribers = asyncio.Event()

    def _ensure_open(self) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        self._path.touch(exist_ok=True)

        if self._f is not None:
            try:
                self._f.close()
            except Exception:
                pass
        self._f = self._path.open("r", encoding="utf-8", errors="replace")
        try:
            st = os.fstat(self._f.fileno())
            self._inode = int(getattr(st, "st_ino", -1) or -1)
        except Exception:
            self._inode = -1
        self._f.seek(0, 2)

    def _encode_event(self, raw: str) -> bytes:
        # Note: SSE requires \n\n to terminate an event.
        return f"event: {self._event_name}\n".encode("utf-8") + b"data: " + raw.encode("utf-8", errors="replace") + b"\n\n"

    def _broadcast(self, item: bytes) -> None:
        # Protect the daemon/web from slow consumers: if a subscriber queue is full, close it.
        removed_any = False
        for q in list(self._subscribers):
            try:
                q.put_nowait(item)
            except asyncio.QueueFull:
                try:
                    while True:
                        _ = q.get_nowait()
                except asyncio.QueueEmpty:
                    pass
                try:
                    q.put_nowait(b"event: error\ndata: {\"code\":\"slow_consumer\",\"message\":\"stream consumer too slow\"}\n\n")
                    q.put_nowait(None)
                except Exception:
                    pass
                self._subscribers.discard(q)
                removed_any = True

        # If we removed the last subscriber (e.g. due to slow-consumer eviction), ensure the idle
        # gate is reset so the tailer can stop after the idle timeout.
        if removed_any and not self._subscribers:
            self._has_subscribers.clear()

    async def _run(self) -> None:
        try:
            self._ensure_open()
            assert self._f is not None
        except Exception:
            return

        while True:
            # Stop when idle (no subscribers) for a while to avoid leaking tasks for long-gone groups.
            if not self._subscribers:
                try:
                    await asyncio.wait_for(self._has_subscribers.wait(), timeout=60.0)
                except asyncio.TimeoutError:
                    break

            if self._f is None:
                try:
                    self._ensure_open()
                except Exception:
                    await asyncio.sleep(0.5)
                    continue

            line = ""
            try:
                line = self._f.readline() if self._f is not None else ""
            except Exception:
                line = ""

            if line:
                raw = line.rstrip("\n")
                if raw:
                    self._broadcast(self._encode_event(raw))
                    self._last_send = time.monotonic()
                continue

            now = time.monotonic()
            if self._heartbeat_s > 0 and now - self._last_send >= self._heartbeat_s:
                self._broadcast(b": heartbeat\n\n")
                self._last_send = now

            await asyncio.sleep(0.2)

            # Detect file rotation / truncation.
            try:
                st = self._path.stat()
                cur_inode = int(getattr(st, "st_ino", -1) or -1)
                if self._inode != -1 and cur_inode != -1 and cur_inode != self._inode:
                    self._ensure_open()
                    continue
                if self._f is not None and st.st_size < self._f.tell():
                    self._ensure_open()
                    continue
            except Exception:
                try:
                    self._path.touch(exist_ok=True)
                except Exception:
                    pass
                try:
                    self._ensure_open()
                except Exception:
                    pass

        # Close: notify subscribers and release resources.
        for q in list(self._subscribers):
            try:
                q.put_nowait(None)
            except Exception:
                pass
        self._subscribers.clear()
        if self._f is not None:
            try:
                self._f.close()
            except Exception:
                pass
        self._f = None
        self._task = None

        # Remove stale tailer entry from the global registry so long-running web
        # processes do not accumulate dead per-path tailers.
        try:
            key = (str(self._event_name), str(self._path))
            async with _TAILERS_LOCK:
                if _TAILERS.get(key) is self:
                    _TAILERS.pop(key, None)
        except Exception:
            pass

    def ensure_started(self) -> None:
        if self._task is None or self._task.done():
            self._task = asyncio.create_task(self._run())

    def subscribe(self, q: asyncio.Queue[bytes | None]) -> None:
        self._subscribers.add(q)
        self._has_subscribers.set()
        self.ensure_started()

    def unsubscribe(self, q: asyncio.Queue[bytes | None]) -> None:
        self._subscribers.discard(q)
        if not self._subscribers:
            self._has_subscribers.clear()


_TAILERS: Dict[Tuple[str, str], _SharedJSONLTailer] = {}
_TAILERS_LOCK = asyncio.Lock()


async def _get_tailer(path: Path, *, event_name: str, heartbeat_s: float) -> _SharedJSONLTailer:
    key = (str(event_name), str(path))
    async with _TAILERS_LOCK:
        t = _TAILERS.get(key)
        if t is None:
            t = _SharedJSONLTailer(path, event_name=event_name, heartbeat_s=heartbeat_s)
            _TAILERS[key] = t
        t.ensure_started()
        return t


async def sse_jsonl_tail_shared(path: Path, *, event_name: str, heartbeat_s: float = 30.0) -> AsyncIterator[bytes]:
    """Tail a JSONL file with a single shared reader and fan-out to many SSE clients."""
    tailer = await _get_tailer(path, event_name=event_name, heartbeat_s=heartbeat_s)
    q: asyncio.Queue[bytes | None] = asyncio.Queue(maxsize=256)
    tailer.subscribe(q)
    try:
        yield b": connected\n\n"
        while True:
            item = await q.get()
            if item is None:
                break
            yield item
    finally:
        tailer.unsubscribe(q)
