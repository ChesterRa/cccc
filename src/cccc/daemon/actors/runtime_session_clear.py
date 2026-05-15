"""Runtime-native session clearing for interactive PTY actors."""

from __future__ import annotations

import time
from typing import Any, Dict

from ...runners import pty as pty_runner

_SUPPORTED_CLEAR_RUNTIMES = {"claude", "codex", "gemini"}
_CLEAR_COMMAND = "/clear"


def supports_runtime_session_clear(runtime: object) -> bool:
    return str(runtime or "").strip().lower() in _SUPPORTED_CLEAR_RUNTIMES


def _input_payload(*, group_id: str, actor_id: str) -> bytes:
    payload = _CLEAR_COMMAND.encode("utf-8")
    try:
        bracketed = bool(pty_runner.SUPERVISOR.bracketed_paste_enabled(group_id=group_id, actor_id=actor_id))
    except Exception:
        bracketed = False
    if bracketed:
        return b"\x1b[200~" + payload + b"\x1b[201~"
    return payload


def clear_running_pty_runtime_session(*, group_id: str, actor_id: str, actor: Dict[str, Any]) -> bool:
    """Ask a running supported PTY runtime to clear its own session."""
    if not supports_runtime_session_clear((actor or {}).get("runtime")):
        return False
    if not pty_runner.SUPERVISOR.actor_running(group_id, actor_id):
        return False
    if not pty_runner.SUPERVISOR.write_input(group_id=group_id, actor_id=actor_id, data=_input_payload(group_id=group_id, actor_id=actor_id)):
        return False
    time.sleep(0.05)
    return bool(pty_runner.SUPERVISOR.write_input(group_id=group_id, actor_id=actor_id, data=b"\r"))
