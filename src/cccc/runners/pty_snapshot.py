from __future__ import annotations

import codecs
from collections import OrderedDict
from dataclasses import dataclass
from typing import Dict, Hashable, Optional


MAX_EXIT_SNAPSHOT_COUNT = 64
MAX_EXIT_SNAPSHOT_BYTES = 256 * 1024
MAX_EXIT_SNAPSHOT_CACHE_BYTES = 8 * 1024 * 1024


def complete_utf8_prefix_len(data: bytes) -> int:
    """Return the byte length that is safe to expose as complete UTF-8 text."""
    decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
    decoder.decode(data, final=False)
    pending, _ = decoder.getstate()
    return len(data) - len(pending)


def cursor_preserving_text(data: bytes) -> str:
    """Decode arbitrary PTY bytes while keeping one placeholder per bad byte."""
    output: list[str] = []
    offset = 0
    while offset < len(data):
        remaining = data[offset:]
        try:
            output.append(remaining.decode("utf-8"))
            break
        except UnicodeDecodeError as error:
            if error.start:
                output.append(remaining[: error.start].decode("utf-8"))
            invalid_len = max(1, error.end - error.start)
            output.append("?" * invalid_len)
            offset += error.start + invalid_len
    return "".join(output)


@dataclass(frozen=True)
class PtyBacklogSnapshot:
    data: bytes
    start_cursor: int
    end_cursor: int

    def trimmed(self, *, max_bytes: int) -> "PtyBacklogSnapshot":
        limit = max(0, int(max_bytes or 0))
        if len(self.data) <= limit:
            return self
        removed = len(self.data) - limit
        return PtyBacklogSnapshot(
            data=self.data[-limit:] if limit else b"",
            start_cursor=min(self.end_cursor, self.start_cursor + removed),
            end_cursor=self.end_cursor,
        )

    def tail_output(self, *, max_bytes: int) -> bytes:
        limit = int(max_bytes or 0)
        if limit <= 0:
            return self.data
        return self.data[-limit:]

    def history_page(self, *, before: Optional[int], limit_bytes: int) -> Dict[str, object]:
        limit = int(limit_bytes or 0)
        if limit <= 0:
            limit = 64_000
        try:
            page_end = self.end_cursor if before is None else int(before)
        except (TypeError, ValueError):
            page_end = self.end_cursor
        if page_end < self.start_cursor:
            return {
                "data": b"",
                "start_cursor": self.start_cursor,
                "end_cursor": self.start_cursor,
                "has_more": False,
                "cursor_expired": True,
            }
        page_end = min(page_end, self.end_cursor)
        page_start = max(self.start_cursor, page_end - limit)
        rel_start = max(0, page_start - self.start_cursor)
        rel_end = max(0, page_end - self.start_cursor)
        return {
            "data": self.data[rel_start:rel_end],
            "start_cursor": page_start,
            "end_cursor": page_end,
            "has_more": page_start > self.start_cursor,
            "cursor_expired": False,
        }

    def history_since_page(self, *, after: int, limit_bytes: int) -> Dict[str, object]:
        limit = max(1, int(limit_bytes or 0) or 64_000)
        try:
            requested_start = int(after)
        except (TypeError, ValueError):
            requested_start = self.end_cursor
        page_start = min(max(requested_start, self.start_cursor), self.end_cursor)
        candidate_end = min(page_start + limit, self.end_cursor)
        lookahead_end = min(candidate_end + 3, self.end_cursor)
        rel_start = max(0, page_start - self.start_cursor)
        rel_candidate_end = max(0, candidate_end - self.start_cursor)
        rel_lookahead_end = max(0, lookahead_end - self.start_cursor)
        lookahead = self.data[rel_start:rel_lookahead_end]
        requested_len = max(0, rel_candidate_end - rel_start)
        while requested_len < len(lookahead) and lookahead[requested_len] & 0b1100_0000 == 0b1000_0000:
            requested_len += 1
        complete_len = complete_utf8_prefix_len(lookahead[:requested_len])
        page_end = page_start + complete_len
        return {
            "data": lookahead[:complete_len],
            "start_cursor": page_start,
            "end_cursor": page_end,
            "has_more": page_end < self.end_cursor,
            "cursor_expired": requested_start < self.start_cursor,
        }


class PtyBacklogSnapshotCache:
    def __init__(
        self,
        *,
        max_items: int = MAX_EXIT_SNAPSHOT_COUNT,
        max_snapshot_bytes: int = MAX_EXIT_SNAPSHOT_BYTES,
        max_total_bytes: int = MAX_EXIT_SNAPSHOT_CACHE_BYTES,
    ) -> None:
        self._max_items = max(0, int(max_items or 0))
        self._max_snapshot_bytes = max(0, int(max_snapshot_bytes or 0))
        self._max_total_bytes = max(0, int(max_total_bytes or 0))
        self._entries: "OrderedDict[Hashable, PtyBacklogSnapshot]" = OrderedDict()
        self._total_bytes = 0

    @property
    def total_bytes(self) -> int:
        return self._total_bytes

    def __len__(self) -> int:
        return len(self._entries)

    def get(self, key: Hashable) -> Optional[PtyBacklogSnapshot]:
        snapshot = self._entries.get(key)
        if snapshot is not None:
            self._entries.move_to_end(key)
        return snapshot

    def discard(self, key: Hashable) -> None:
        snapshot = self._entries.pop(key, None)
        if snapshot is not None:
            self._total_bytes -= len(snapshot.data)

    def remember(self, key: Hashable, snapshot: PtyBacklogSnapshot) -> None:
        retained = snapshot.trimmed(max_bytes=self._max_snapshot_bytes)
        self.discard(key)
        self._entries[key] = retained
        self._total_bytes += len(retained.data)
        while len(self._entries) > self._max_items or self._total_bytes > self._max_total_bytes:
            _, evicted = self._entries.popitem(last=False)
            self._total_bytes -= len(evicted.data)
