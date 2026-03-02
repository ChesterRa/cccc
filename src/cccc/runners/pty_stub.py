from __future__ import annotations

import socket
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Dict, Iterable, Optional, Tuple

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
        max_backlog_bytes: int = 2_000_000,
    ) -> PtySession:
        raise RuntimeError("pty runner is not supported on this platform; use runner='headless' (Windows: install pywinpty for ConPTY)")

    def stop_actor(self, *, group_id: str, actor_id: str) -> None:
        return None

    def stop_group(self, *, group_id: str) -> None:
        return None

    def stop_all(self) -> None:
        return None

    def attach(self, *, group_id: str, actor_id: str, sock: socket.socket) -> None:
        raise RuntimeError("pty runner is not supported on this platform (Windows: install pywinpty for ConPTY)")

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
