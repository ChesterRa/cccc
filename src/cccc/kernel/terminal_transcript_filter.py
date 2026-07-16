from __future__ import annotations

import re

from .pty_terminal_state import strip_ansi

_CODEX_WORKING_LINE_RE = re.compile(r"^\s*[◦·•]\s*(?:�\s*)?Working\b.*$", re.IGNORECASE)


def strip_codex_working_status_lines(text: str, *, runtime: str = "") -> str:
    if str(runtime or "").strip().lower() != "codex":
        return str(text or "")
    stripped = "\n".join(
        line
        for line in str(text or "").splitlines()
        if not _CODEX_WORKING_LINE_RE.match(strip_ansi(line))
    )
    return stripped if stripped.strip() else ""
