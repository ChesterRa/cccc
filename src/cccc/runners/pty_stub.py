from __future__ import annotations

import socket
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Dict, Iterable, Optional, Tuple

from .platform_support import pty_support_error_message

PTY_SUPPORTED = False


@dataclass
class PtySession:
    group_id: str = ""
    actor_id: str = ""
    pid: int = 0


class PtySupervisor:
    def set_exit_hook(self, hook: Optional[Callable[[PtySession], None]]) -> None:
        return None

    def group_running(self, group_id: str) -> bool:
        return False

    def actor_running(self, group_id: str, actor_id: str) -> bool:
        return False

    def tail_output(self, *, group_id: str, actor_id: str, max_bytes: int = 2_000_000) -> bytes:
        return b""

    def history_page(
        self,
        *,
        group_id: str,
        actor_id: str,
        before: Optional[int] = None,
        limit_bytes: int = 64_000,
    ):
        return {"data": b"", "start_cursor": 0, "end_cursor": 0, "has_more": False, "cursor_expired": False}

    def terminal_override(self, *, group_id: str, actor_id: str):
        return None

    def clear_backlog(self, *, group_id: str, actor_id: str) -> bool:
        return False

    def start_actor(
        self,
        *,
        group_id: str,
        actor_id: str,
        cwd: Path,
        command: Iterable[str],
        env: Dict[str, str],
        runtime: str = "",
        max_backlog_bytes: int = 2_000_000,
    ) -> PtySession:
        raise RuntimeError(pty_support_error_message() or "PTY runner is not supported in this environment.")

    def stop_actor(self, *, group_id: str, actor_id: str) -> None:
        return None

    def stop_group(self, *, group_id: str) -> None:
        return None

    def stop_all(self) -> None:
        return None

    def attach(self, *, group_id: str, actor_id: str, sock: socket.socket, since: Optional[int] = None) -> None:
        _ = since
        raise RuntimeError(pty_support_error_message() or "PTY runner is not supported in this environment.")

    def bracketed_paste_enabled(self, *, group_id: str, actor_id: str) -> bool:
        return False

    def bracketed_paste_status(self, *, group_id: str, actor_id: str) -> Tuple[bool, Optional[float]]:
        return (False, None)

    def startup_times(self, *, group_id: str, actor_id: str) -> Tuple[Optional[float], Optional[float]]:
        return (None, None)

    def session_key(self, *, group_id: str, actor_id: str) -> Optional[str]:
        return None

    def resize(self, *, group_id: str, actor_id: str, cols: int, rows: int) -> None:
        return None

    def write_input(self, *, group_id: str, actor_id: str, data: bytes) -> bool:
        return False


SUPERVISOR = PtySupervisor()
