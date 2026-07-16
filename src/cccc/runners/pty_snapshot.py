from __future__ import annotations

from collections import OrderedDict
from dataclasses import dataclass
from typing import Dict, Hashable, Optional


MAX_EXIT_SNAPSHOT_COUNT = 64
MAX_EXIT_SNAPSHOT_BYTES = 256 * 1024
MAX_EXIT_SNAPSHOT_CACHE_BYTES = 8 * 1024 * 1024


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
