"""Headless runner for MCP-driven agent execution.

Unlike PTY runner which provides an interactive terminal session,
headless runner is designed for agents that operate purely through
MCP tools (inbox/send/context) without needing a shell.

The loop: idle → receive message → working → report → waiting → decision → continue/stop
"""
from __future__ import annotations

import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable, Dict, Literal, Optional

from ..contracts.v1.actor import HeadlessState
from ..util.fs import atomic_write_json, read_json
from ..util.time import utc_now_iso


HeadlessStatus = Literal["idle", "working", "waiting", "stopped"]


@dataclass
class HeadlessSession:
    """A headless actor session managed by the daemon."""
    group_id: str
    actor_id: str
    cwd: Path
    env: Dict[str, str]
    on_exit: Optional[Callable[["HeadlessSession"], None]] = None

    _lock: threading.Lock = field(default_factory=threading.Lock, repr=False)
    _status: HeadlessStatus = "idle"
    _current_task_id: Optional[str] = None
    _last_message_id: Optional[str] = None
    _started_at: str = field(default_factory=utc_now_iso)
    _updated_at: str = field(default_factory=utc_now_iso)
    _running: bool = True

    def __post_init__(self) -> None:
        self._started_at = utc_now_iso()
        self._updated_at = self._started_at

    @property
    def status(self) -> HeadlessStatus:
        with self._lock:
            return self._status

    @property
    def current_task_id(self) -> Optional[str]:
        with self._lock:
            return self._current_task_id

    @property
    def last_message_id(self) -> Optional[str]:
        with self._lock:
            return self._last_message_id

    def is_running(self) -> bool:
        with self._lock:
            return self._running and self._status != "stopped"

    def get_state(self) -> HeadlessState:
        """Get current state as a contract model."""
        with self._lock:
            return HeadlessState(
                group_id=self.group_id,
                actor_id=self.actor_id,
                status=self._status,
                current_task_id=self._current_task_id,
                last_message_id=self._last_message_id,
                started_at=self._started_at,
                updated_at=self._updated_at,
            )

    def set_status(self, status: HeadlessStatus, *, task_id: Optional[str] = None) -> None:
        """Update session status (called by agent via MCP)."""
        with self._lock:
            self._status = status
            self._updated_at = utc_now_iso()
            if task_id is not None:
                self._current_task_id = task_id

    def set_last_message(self, message_id: str) -> None:
        """Record the last processed message ID."""
        with self._lock:
            self._last_message_id = message_id
            self._updated_at = utc_now_iso()

    def stop(self) -> None:
        """Stop the session."""
        with self._lock:
            self._running = False
            self._status = "stopped"
            self._updated_at = utc_now_iso()

        hook = self.on_exit
        if hook is not None:
            try:
                hook(self)
            except Exception:
                pass


class HeadlessSupervisor:
    """Manages headless actor sessions."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._sessions: Dict[tuple[str, str], HeadlessSession] = {}
        self._exit_hook: Optional[Callable[[HeadlessSession], None]] = None

    def set_exit_hook(self, hook: Optional[Callable[[HeadlessSession], None]]) -> None:
        with self._lock:
            self._exit_hook = hook

    def _on_session_exit(self, session: HeadlessSession) -> None:
        key = (session.group_id, session.actor_id)
        with self._lock:
            if self._sessions.get(key) is session:
                self._sessions.pop(key, None)
            hook = self._exit_hook
        if hook is not None:
            try:
                hook(session)
            except Exception:
                pass

    def actor_running(self, group_id: str, actor_id: str) -> bool:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.get(key)
        return bool(s and s.is_running())

    def group_running(self, group_id: str) -> bool:
        gid = str(group_id or "").strip()
        if not gid:
            return False
        with self._lock:
            for (g, _), s in self._sessions.items():
                if g == gid and s.is_running():
                    return True
        return False

    def start_actor(
        self,
        *,
        group_id: str,
        actor_id: str,
        cwd: Path,
        env: Dict[str, str],
    ) -> HeadlessSession:
        """Start a headless session for an actor."""
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        if not key[0] or not key[1]:
            raise ValueError("missing group_id/actor_id")

        with self._lock:
            existing = self._sessions.get(key)
            if existing is not None and existing.is_running():
                return existing

            session = HeadlessSession(
                group_id=key[0],
                actor_id=key[1],
                cwd=cwd,
                env=env,
                on_exit=self._on_session_exit,
            )
            self._sessions[key] = session
        return session

    def stop_actor(self, *, group_id: str, actor_id: str) -> None:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.pop(key, None)
        if s is not None:
            s.stop()

    def stop_group(self, *, group_id: str) -> None:
        gid = str(group_id or "").strip()
        if not gid:
            return
        with self._lock:
            items = [(k, s) for k, s in self._sessions.items() if k[0] == gid]
            for k, _ in items:
                self._sessions.pop(k, None)
        for _, s in items:
            try:
                s.stop()
            except Exception:
                pass

    def stop_all(self) -> None:
        with self._lock:
            items = list(self._sessions.items())
            self._sessions.clear()
        for _, s in items:
            try:
                s.stop()
            except Exception:
                pass

    def get_session(self, *, group_id: str, actor_id: str) -> Optional[HeadlessSession]:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            return self._sessions.get(key)

    def get_state(self, *, group_id: str, actor_id: str) -> Optional[HeadlessState]:
        session = self.get_session(group_id=group_id, actor_id=actor_id)
        if session is None:
            return None
        return session.get_state()

    def set_status(
        self,
        *,
        group_id: str,
        actor_id: str,
        status: HeadlessStatus,
        task_id: Optional[str] = None,
    ) -> bool:
        """Update status for a headless session. Returns True if session exists."""
        session = self.get_session(group_id=group_id, actor_id=actor_id)
        if session is None:
            return False
        session.set_status(status, task_id=task_id)
        return True

    def set_last_message(
        self,
        *,
        group_id: str,
        actor_id: str,
        message_id: str,
    ) -> bool:
        """Record last processed message. Returns True if session exists."""
        session = self.get_session(group_id=group_id, actor_id=actor_id)
        if session is None:
            return False
        session.set_last_message(message_id)
        return True


# Global supervisor instance
SUPERVISOR = HeadlessSupervisor()
