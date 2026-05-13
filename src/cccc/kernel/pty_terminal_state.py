from __future__ import annotations

import re
from typing import Any, Dict, Optional


DEFAULT_TERMINAL_SIGNAL_WINDOW_CHARS = 1_600


def strip_ansi(text: str) -> str:
    return re.sub(r"\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1B\\))", "", str(text or "")).replace("\r", "")


def _clean_text(value: Any) -> str:
    return str(value or "").strip()


def _recent_non_empty_lines(text: str, *, max_lines: int = 4) -> list[str]:
    lines: list[str] = []
    for raw_line in reversed(str(text or "").split("\n")):
        line = raw_line.strip()
        if not line:
            continue
        lines.append(line)
        if len(lines) >= max_lines:
            break
    return lines


def _is_terminal_prompt_line(line: str) -> bool:
    value = str(line or "").strip()
    if not value:
        return False
    if re.match(r"^(?:>|›)\s?.*", value):
        return True
    if re.match(r"^(?:\$|%|#|❯|➜|›)\s+.*$", value):
        return True
    if re.match(r"^[\w.@:/~-]+\s*(?:\$|%|#)\s*$", value):
        return True
    return False


def _has_terminal_prompt_visible(text: str) -> bool:
    for line in _recent_non_empty_lines(text):
        return _is_terminal_prompt_line(line)
    return False


def _recent_lines_have_terminal_prompt(text: str) -> bool:
    return any(_is_terminal_prompt_line(line) for line in _recent_non_empty_lines(text))


def _has_claude_prompt_visible(text: str) -> bool:
    for line in _recent_non_empty_lines(text, max_lines=8):
        value = str(line or "").strip()
        if re.match(r"^[│┃]\s*(?:>|›)(?:\s.*)?[│┃]?$", value):
            return True
    return False


def _tail_window_has_claude_working_indicator(text: str) -> bool:
    value = str(text or "")
    if not value:
        return False
    compact = re.sub(r"\s+", " ", value).lower()
    if "esc to interrupt" in compact:
        return True
    return bool(
        re.search(
            r"(?:✻|✽|✳|✶|✢|●|○)\s*(?:thinking|working|pondering|responding|coding|reading|searching|running)",
            compact,
            re.IGNORECASE,
        )
    )


def _tail_window_has_codex_working_banner(text: str) -> bool:
    value = str(text or "")
    if not value:
        return False
    compact = re.sub(r"\s+", " ", value)
    return bool(re.search(r"\bworking\s*\(", compact, re.IGNORECASE))


def _tail_window(text: str, *, max_chars: int = DEFAULT_TERMINAL_SIGNAL_WINDOW_CHARS) -> str:
    value = str(text or "")
    if max_chars <= 0 or len(value) <= max_chars:
        return value
    return value[-max_chars:]


def derive_pty_terminal_override(*, runtime: str, terminal_text: str) -> Optional[Dict[str, Any]]:
    runtime_id = _clean_text(runtime).lower()
    cleaned = strip_ansi(terminal_text)
    if runtime_id == "claude":
        tail_text = _tail_window(cleaned)
        if _tail_window_has_claude_working_indicator(tail_text):
            return {
                "effective_working_state": "working",
                "effective_working_reason": "pty_terminal_claude_working_indicator",
            }
        if _has_claude_prompt_visible(cleaned):
            return {
                "effective_working_state": "idle",
                "effective_working_reason": "pty_terminal_claude_prompt_visible",
            }
        return None

    tail_text = _tail_window(cleaned)
    if runtime_id == "codex" and (
        _has_terminal_prompt_visible(cleaned)
        or (_recent_lines_have_terminal_prompt(cleaned) and _tail_window_has_codex_working_banner(tail_text))
    ):
        return {
            "effective_working_state": "idle",
            "effective_working_reason": "pty_terminal_prompt_visible",
        }

    if runtime_id == "codex" and _tail_window_has_codex_working_banner(tail_text):
        return {
            "effective_working_state": "working",
            "effective_working_reason": "pty_terminal_codex_working_banner",
        }

    return None
