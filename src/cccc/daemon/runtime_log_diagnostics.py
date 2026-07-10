"""Fail-closed compatibility API for runtime log diagnostics.

Shared runtime log files cannot be attributed to one terminal session, so they
must never be exposed through terminal transcripts.
"""

from __future__ import annotations

from typing import Any, Mapping


_EXIT_WITHOUT_OUTPUT_MARKER = "before producing terminal output"


def terminal_output_needs_runtime_log(text: str) -> bool:
    value = str(text or "").strip()
    return _EXIT_WITHOUT_OUTPUT_MARKER in value


def runtime_log_tail(
    runtime: str,
    *,
    env: Mapping[str, Any] | None = None,
    max_chars: int = 6000,
) -> str:
    _ = runtime, env, max_chars
    return ""
