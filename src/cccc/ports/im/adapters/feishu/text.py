"""Feishu outbound text helpers."""

from __future__ import annotations

from typing import Callable


FEISHU_MAX_MESSAGE_LENGTH = 30720


def compose_safe_text(
    text: str,
    *,
    max_chars: int,
    max_lines: int,
    summarize_fn: Callable[[str, int, int], str],
) -> str:
    summarized = summarize_fn(text, max_chars, max_lines)
    if len(summarized) > FEISHU_MAX_MESSAGE_LENGTH:
        summarized = summarized[: FEISHU_MAX_MESSAGE_LENGTH - 1] + "..."
    return summarized
