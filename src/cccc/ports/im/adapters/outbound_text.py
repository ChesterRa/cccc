"""Lossless outbound text chunking and bounded stream previews."""

from __future__ import annotations

import time
from typing import Any, List, Optional, Tuple


def text_limit(configured: int, hard_limit: int) -> int:
    """Return a positive configured limit capped by the platform hard limit."""
    hard = max(1, int(hard_limit))
    try:
        requested = int(configured)
    except (TypeError, ValueError):
        requested = hard
    return min(requested, hard) if requested > 0 else hard


def split_text_chunks(
    text: str,
    *,
    max_chars: int,
    hard_limit: int,
    max_lines: Optional[int] = None,
) -> List[str]:
    """Split text without dropping or rewriting any Unicode characters."""
    value = str(text or "")
    if not value:
        return []

    char_limit = text_limit(max_chars, hard_limit)
    line_limit = None
    if max_lines is not None:
        try:
            parsed_lines = int(max_lines)
        except (TypeError, ValueError):
            parsed_lines = 0
        if parsed_lines > 0:
            line_limit = parsed_lines

    chunks: List[str] = []
    start = 0
    chars = 0
    lines = 1

    for index, character in enumerate(value):
        exceeds_chars = chars >= char_limit
        exceeds_lines = (
            character == "\n" and line_limit is not None and lines >= line_limit
        )
        if index > start and (exceeds_chars or exceeds_lines):
            chunks.append(value[start:index])
            start = index
            chars = 0
            lines = 1
        chars += 1
        if character == "\n":
            lines += 1

    if start < len(value):
        chunks.append(value[start:])
    return chunks


def stream_preview(
    text: str,
    *,
    max_chars: int,
    hard_limit: int,
    max_lines: Optional[int] = None,
    placeholder: str = "…",
) -> Tuple[str, bool]:
    """Return a bounded preview plus whether it exactly represents ``text``."""
    value = str(text or "")
    chunks = split_text_chunks(
        value,
        max_chars=max_chars,
        hard_limit=hard_limit,
        max_lines=max_lines,
    )
    if not chunks:
        return placeholder[: text_limit(max_chars, hard_limit)], False
    if len(chunks) == 1:
        return chunks[0], True

    limit = text_limit(max_chars, hard_limit)
    first = chunks[0]
    marker = placeholder[:limit]
    if len(first) + len(marker) <= limit:
        return first + marker, False
    return first[: max(0, limit - len(marker))] + marker, False


def utf8_stream_preview(
    text: str,
    *,
    max_bytes: int,
    placeholder: str = "\n…",
) -> Tuple[str, bool]:
    """Return a UTF-8-byte-bounded preview without splitting a code point."""
    value = str(text or "")
    limit = max(1, int(max_bytes))
    encoded = value.encode("utf-8")
    if encoded and len(encoded) <= limit:
        return value, True
    if not encoded:
        marker = placeholder.encode("utf-8")[:limit]
        return marker.decode("utf-8", errors="ignore"), not bool(marker)

    marker = placeholder.encode("utf-8")
    if len(marker) > limit:
        marker = "…".encode("utf-8")[:limit]
    end = max(0, limit - len(marker))
    while end > 0 and (encoded[end] & 0b1100_0000) == 0b1000_0000:
        end -= 1
    prefix = encoded[:end].decode("utf-8", errors="ignore")
    return prefix + marker.decode("utf-8", errors="ignore"), False


def stream_update_due(handle: dict[str, Any], *, interval: float) -> bool:
    """Return whether a mutable stream handle may send another preview frame."""
    platform_handle = handle.get("platform_handle")
    if not isinstance(platform_handle, dict):
        return False
    try:
        last_update = float(platform_handle.get("last_update_at") or 0.0)
    except (TypeError, ValueError):
        last_update = 0.0
    return time.monotonic() - last_update >= max(0.0, float(interval))


def mark_stream_updated(handle: dict[str, Any]) -> None:
    """Record a successful preview update on a mutable stream handle."""
    platform_handle = handle.get("platform_handle")
    if isinstance(platform_handle, dict):
        platform_handle["last_update_at"] = time.monotonic()
