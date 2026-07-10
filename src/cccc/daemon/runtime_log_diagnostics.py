"""Runtime-specific local log snippets for startup diagnostics."""
from __future__ import annotations

import os
from pathlib import Path
from typing import Any, Mapping


_EXIT_WITHOUT_OUTPUT_MARKER = "before producing terminal output"


def terminal_output_needs_runtime_log(text: str) -> bool:
    value = str(text or "").strip()
    return _EXIT_WITHOUT_OUTPUT_MARKER in value


def _home_dir(env: Mapping[str, Any] | None) -> Path:
    raw = ""
    if isinstance(env, Mapping):
        raw = str(env.get("HOME") or "").strip()
    if not raw:
        raw = str(os.environ.get("HOME") or "").strip()
    return Path(raw or "~").expanduser()


def _kimi_log_path(env: Mapping[str, Any] | None) -> Path:
    raw = ""
    if isinstance(env, Mapping):
        raw = str(env.get("KIMI_SHARE_DIR") or "").strip()
    if raw:
        return Path(raw).expanduser() / "logs" / "kimi.log"
    return _home_dir(env) / ".kimi" / "logs" / "kimi.log"


def _read_tail(path: Path, *, max_chars: int) -> str:
    limit = max(1, int(max_chars or 0))
    try:
        data = path.read_bytes()
    except Exception:
        return ""
    text = data[-limit:].decode("utf-8", errors="replace")
    return text.strip()


def runtime_log_tail(runtime: str, *, env: Mapping[str, Any] | None = None, max_chars: int = 6000) -> str:
    runtime_key = str(runtime or "").strip().lower()
    if runtime_key != "kimi":
        return ""
    path = _kimi_log_path(env)
    text = _read_tail(path, max_chars=max_chars)
    if not text:
        return ""
    return f"Runtime log ({path}):\n{text}"
