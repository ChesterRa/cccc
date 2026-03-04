"""Memory source types."""

from enum import Enum


class MemorySource(str, Enum):
    """Source of memory data."""

    MEMORY = "memory"

    SESSIONS = "sessions"
