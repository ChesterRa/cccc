from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, Optional

from ..kernel.working_state import DEFAULT_PTY_ACTIVITY_SIGNAL_TAIL_BYTES


@dataclass(frozen=True)
class PtyActivitySignal:
    terminal_override: Optional[Dict[str, Any]]
    terminal_text: str


def read_pty_activity_signal(
    pty_supervisor: Any,
    *,
    group_id: str,
    actor_id: str,
    max_tail_bytes: int = DEFAULT_PTY_ACTIVITY_SIGNAL_TAIL_BYTES,
) -> PtyActivitySignal:
    terminal_override = None
    try:
        terminal_override = pty_supervisor.terminal_override(group_id=group_id, actor_id=actor_id)
    except Exception:
        terminal_override = None

    if terminal_override:
        return PtyActivitySignal(terminal_override=terminal_override, terminal_text="")

    try:
        tail = pty_supervisor.tail_output(
            group_id=group_id,
            actor_id=actor_id,
            max_bytes=max_tail_bytes,
        )
        if isinstance(tail, bytes):
            terminal_text = tail.decode("utf-8", errors="replace")
        else:
            terminal_text = str(tail or "")
    except Exception:
        terminal_text = ""

    return PtyActivitySignal(terminal_override=None, terminal_text=terminal_text)
