"""Bounded stdout/stderr readers for the DeepSeek ACP supervisor."""

from __future__ import annotations

import os
import queue
from typing import Any, Mapping

from ..kernel.deepseek_acp import ACPProtocolError

STDERR_TAIL_BYTES = 16 * 1024


def merge_subprocess_env(actor_env: Mapping[str, str]) -> dict[str, str]:
    env = dict(os.environ)
    env.update(actor_env)
    return env


class DeepSeekStreamMixin:
    def _read_stdout(self, process: Any, generation: str, frames: Any) -> None:
        if process is None or process.stdout is None:
            return
        try:
            buffer = bytearray()
            while True:
                read_chunk = getattr(process.stdout, "read1", process.stdout.read)
                chunk = read_chunk(4096)
                if not chunk:
                    break
                for byte in chunk:
                    buffer.append(byte)
                    if len(buffer) > 64 * 1024:
                        raise ACPProtocolError("frame exceeds byte cap")
                    if byte != 10:
                        continue
                    if self.generation != generation or self._process is not process:
                        return
                    with self._protocol_lock:
                        frame = self._protocol.feed_line(
                            bytes(buffer[:-1]).rstrip(b"\\r")
                        )
                    try:
                        frames.put_nowait(frame)
                    except queue.Full as exc:
                        raise ACPProtocolError("deepseek frame queue is full") from exc
                    buffer.clear()
            if buffer and self.generation == generation and self._process is process:
                with self._protocol_lock:
                    frame = self._protocol.feed_line(bytes(buffer))
                try:
                    frames.put_nowait(frame)
                except queue.Full as exc:
                    raise ACPProtocolError("deepseek frame queue is full") from exc
        except (ACPProtocolError, UnicodeError, ValueError, OSError) as exc:
            self._reader_error = exc
            self._stopping.set()
        finally:
            if self.generation == generation and self._process is process:
                self._stopping.set()
                try:
                    frames.put_nowait(None)
                except queue.Full:
                    pass

    def _read_stderr(self) -> None:
        process = self._process
        if process is None or process.stderr is None:
            return
        while True:
            chunk = process.stderr.read(4096)
            if not chunk:
                return
            with self._lock:
                self._stderr_tail.extend(chunk)
                if len(self._stderr_tail) > STDERR_TAIL_BYTES:
                    del self._stderr_tail[:-STDERR_TAIL_BYTES]
