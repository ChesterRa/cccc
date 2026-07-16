"""
Memory model helpers (file-first ReMe mode).

This module intentionally contains no runtime DB logic.
Long-term memory is represented by:
- state/memory/MEMORY.md
- state/memory/daily/YYYY-MM-DD__<group_label>.md
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import List


MEMORY_ENTRY_KINDS = (
    "conversation",
    "task_event",
    "stable_knowledge",
    "daily_note",
)


@dataclass
class MemoryFragment:
    entry_id: str
    date: str
    group_label: str
    kind: str
    summary: str
    actor_id: str = ""
    source_refs: List[str] = field(default_factory=list)
    tags: List[str] = field(default_factory=list)
    supersedes: List[str] = field(default_factory=list)
    created_at: str = ""
