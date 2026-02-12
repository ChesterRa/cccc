from __future__ import annotations

import fcntl
import os
import pty
import queue
import selectors
import signal
import socket
import struct
import subprocess
import threading
import time
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Dict, Iterable, Optional, Tuple

import termios

PTY_SUPPORTED = True


def _set_winsize(fd: int, *, cols: int, rows: int) -> None:
    try:
        winsize = struct.pack("HHHH", int(rows), int(cols), 0, 0)
        fcntl.ioctl(fd, termios.TIOCSWINSZ, winsize)
    except Exception:
        pass


def _best_effort_killpg(pid: int, sig: signal.Signals) -> None:
    if pid <= 0:
        return
    try:
        os.killpg(pid, sig)
    except Exception:
        try:
            os.kill(pid, sig)
        except Exception:
            pass


@dataclass
class _PtyClient:
    sock: socket.socket
    writer: bool
    outbuf: bytearray


class PtySession:
    def __init__(
        self,
        *,
        group_id: str,
        actor_id: str,
        cwd: Path,
        command: Iterable[str],
        env: Dict[str, str],
        on_exit: Optional[Callable[["PtySession"], None]] = None,
        max_backlog_bytes: int = 2_000_000,
        max_client_buffer_bytes: int = 8_000_000,
        cols: int = 120,
        rows: int = 40,
    ) -> None:
        self.group_id = group_id
        self.actor_id = actor_id
        self._on_exit = on_exit
        self._started_at = time.monotonic()
        self._first_output_at: Optional[float] = None
        self._last_output_at: Optional[float] = None
        self._max_backlog_bytes = int(max_backlog_bytes)
        # Allow some slack beyond the initial backlog so clients are not immediately detached if the
        # PTY produces output while we are still draining the attach-time backlog.
        slack = max(2_000_000, int(self._max_backlog_bytes // 8))
        self._max_client_buffer_bytes = max(int(max_client_buffer_bytes), int(self._max_backlog_bytes + slack))

        self._selector = selectors.DefaultSelector()
        self._lock = threading.Lock()
        self._clients: Dict[int, _PtyClient] = {}
        self._writer_fd: Optional[int] = None
        self._attach_q: queue.Queue[socket.socket] = queue.Queue()
        self._cmd_r, self._cmd_w = os.pipe()
        os.set_blocking(self._cmd_r, False)
        os.set_blocking(self._cmd_w, False)

        self._backlog: deque[bytes] = deque()
        self._backlog_bytes = 0
        self._mode_tail = b""
        self._query_tail = b""
        self._bracketed_paste = False
        self._bracketed_paste_changed_at: Optional[float] = None

        master_fd, slave_fd = pty.openpty()
        _set_winsize(master_fd, cols=cols, rows=rows)
        os.set_blocking(master_fd, False)

        cmd = [str(x) for x in command if isinstance(x, str) and str(x).strip()]
        if not cmd:
            cmd = ["bash"] if Path("/bin/bash").exists() else ["sh"]

        proc_env = os.environ.copy()
        proc_env.update({k: v for k, v in env.items() if isinstance(k, str) and isinstance(v, str)})
        proc_env.setdefault("TERM", "xterm-256color")

        def _preexec() -> None:
            try:
                os.setsid()
            except Exception:
                pass
            try:
                fcntl.ioctl(0, termios.TIOCSCTTY, 0)
            except Exception:
                pass

        self._proc = subprocess.Popen(
            cmd,
            stdin=slave_fd,
            stdout=slave_fd,
            stderr=slave_fd,
            cwd=str(cwd),
            env=proc_env,
            close_fds=True,
            preexec_fn=_preexec,
        )
        try:
            os.close(slave_fd)
        except Exception:
            pass

        self._master_fd = master_fd
        self._running = True

        self._selector.register(master_fd, selectors.EVENT_READ, data=("pty", None))
        self._selector.register(self._cmd_r, selectors.EVENT_READ, data=("cmd", None))

        self._thread = threading.Thread(target=self._loop, name=f"cccc-pty:{group_id}:{actor_id}", daemon=True)
        self._thread.start()

    @property
    def pid(self) -> int:
        return int(getattr(self._proc, "pid", 0) or 0)

    def is_running(self) -> bool:
        return bool(self._running) and self._proc.poll() is None

    def started_at_monotonic(self) -> float:
        return float(self._started_at)

    def first_output_at_monotonic(self) -> Optional[float]:
        with self._lock:
            return None if self._first_output_at is None else float(self._first_output_at)

    def bracketed_paste_enabled(self) -> bool:
        with self._lock:
            return bool(self._bracketed_paste)

    def bracketed_paste_changed_at_monotonic(self) -> Optional[float]:
        with self._lock:
            return None if self._bracketed_paste_changed_at is None else float(self._bracketed_paste_changed_at)

    def last_output_at_monotonic(self) -> Optional[float]:
        """Return monotonic timestamp of last PTY output (or None if no output yet)."""
        with self._lock:
            return None if self._last_output_at is None else float(self._last_output_at)

    def idle_seconds(self) -> float:
        """Return seconds since last PTY output (or since session start if no output)."""
        now = time.monotonic()
        with self._lock:
            if self._last_output_at is not None:
                return now - self._last_output_at
            return now - self._started_at

    def tail_output(self, *, max_bytes: int = 2_000_000) -> bytes:
        """Return the latest PTY output bytes (bounded).

        This is intended for developer-mode diagnostics (e.g. terminal transcript tail).
        """
        limit = int(max_bytes or 0)
        if limit <= 0:
            limit = int(self._max_backlog_bytes or 0) or 2_000_000
        with self._lock:
            chunks = list(self._backlog)
        if not chunks:
            return b""
        out: list[bytes] = []
        total = 0
        for chunk in reversed(chunks):
            out.append(chunk)
            total += len(chunk)
            if total >= limit:
                break
        data = b"".join(reversed(out))
        if len(data) > limit:
            data = data[-limit:]
        return data

    def clear_backlog(self) -> None:
        """Clear the in-memory PTY backlog/ring buffer (developer-mode only)."""
        with self._lock:
            try:
                self._backlog.clear()
            except Exception:
                self._backlog = deque()
            self._backlog_bytes = 0
            self._mode_tail = b""

    def resize(self, *, cols: int, rows: int) -> None:
        if cols <= 0 or rows <= 0:
            return
        _set_winsize(self._master_fd, cols=int(cols), rows=int(rows))
        _best_effort_killpg(self.pid, signal.SIGWINCH)

    def write_input(self, data: bytes) -> bool:
        """Write input data to the PTY master fd.

        Handles non-blocking mode by:
        1. Retrying on BlockingIOError (EAGAIN/EWOULDBLOCK)
        2. Handling partial writes
        3. Using a reasonable timeout to avoid infinite loops
        """
        if not data:
            return True

        remaining = data
        max_attempts = 50  # ~5 seconds max with 0.1s sleep
        attempt = 0

        while remaining and attempt < max_attempts:
            try:
                written = os.write(self._master_fd, remaining)
                if written <= 0:
                    # Shouldn't happen, but treat as failure
                    return False
                remaining = remaining[written:]
                attempt = 0  # Reset attempt counter on successful write
            except BlockingIOError:
                # Buffer full, wait and retry
                attempt += 1
                time.sleep(0.1)
            except OSError:
                # Real error (fd closed, etc.)
                return False

        return len(remaining) == 0

    def stop(self) -> None:
        self._running = False
        _best_effort_killpg(self.pid, signal.SIGTERM)
        deadline = time.time() + 1.0
        while time.time() < deadline:
            if self._proc.poll() is not None:
                break
            time.sleep(0.05)
        if self._proc.poll() is None:
            _best_effort_killpg(self.pid, signal.SIGKILL)
        try:
            os.close(self._master_fd)
        except Exception:
            pass

    def attach_client(self, sock: socket.socket) -> None:
        try:
            self._attach_q.put_nowait(sock)
        except Exception:
            return
        try:
            os.write(self._cmd_w, b"x")
        except Exception:
            pass

    def detach_client(self, fileno: int) -> None:
        with self._lock:
            client = self._clients.pop(fileno, None)
            if self._writer_fd == fileno:
                self._writer_fd = None

        if client is not None:
            try:
                self._selector.unregister(client.sock)
            except Exception:
                pass
            try:
                client.sock.close()
            except Exception:
                pass

        self._maybe_promote_writer()

    def _maybe_promote_writer(self) -> None:
        with self._lock:
            if self._writer_fd is not None:
                return
            for fd, c in self._clients.items():
                self._writer_fd = fd
                c.writer = True
                try:
                    current = 0
                    try:
                        current = self._selector.get_key(c.sock).events
                    except Exception:
                        current = 0
                    self._selector.modify(c.sock, current | selectors.EVENT_READ, data=("client", fd))
                except Exception:
                    pass
                break

    def _append_backlog(self, chunk: bytes) -> None:
        if not chunk:
            return
        now = time.monotonic()
        if self._first_output_at is None:
            self._first_output_at = now
        self._last_output_at = now
        self._backlog.append(chunk)
        self._backlog_bytes += len(chunk)
        limit = max(0, self._max_backlog_bytes)
        while limit and self._backlog_bytes > limit and self._backlog:
            drop = self._backlog.popleft()
            self._backlog_bytes -= len(drop)

    def _close_all(self) -> None:
        try:
            self._selector.unregister(self._master_fd)
        except Exception:
            pass
        try:
            os.close(self._master_fd)
        except Exception:
            pass
        try:
            self._selector.unregister(self._cmd_r)
        except Exception:
            pass
        try:
            os.close(self._cmd_r)
        except Exception:
            pass
        try:
            os.close(self._cmd_w)
        except Exception:
            pass

        with self._lock:
            items = list(self._clients.items())
            self._clients.clear()
            self._writer_fd = None

        for fileno, client in items:
            _ = fileno
            try:
                self._selector.unregister(client.sock)
            except Exception:
                pass
            try:
                client.sock.close()
            except Exception:
                pass

        try:
            self._selector.close()
        except Exception:
            pass
        while True:
            try:
                sock = self._attach_q.get_nowait()
            except queue.Empty:
                break
            try:
                sock.close()
            except Exception:
                pass

    def _on_pty_readable(self) -> None:
        while True:
            try:
                chunk = os.read(self._master_fd, 65536)
            except BlockingIOError:
                return
            except OSError:
                self._running = False
                return
            if not chunk:
                self._running = False
                return

            self._maybe_reply_to_terminal_queries(chunk)
            self._update_input_modes(chunk)
            with self._lock:
                self._append_backlog(chunk)
                clients = list(self._clients.items())

            for fileno, client in clients:
                if self._max_client_buffer_bytes and (len(client.outbuf) + len(chunk) > self._max_client_buffer_bytes):
                    self.detach_client(fileno)
                    continue
                client.outbuf.extend(chunk)
                try:
                    events = self._selector.get_key(client.sock).events
                    self._selector.modify(client.sock, events | selectors.EVENT_WRITE, data=("client", fileno))
                except Exception:
                    self.detach_client(fileno)

    def _update_input_modes(self, chunk: bytes) -> None:
        if not chunk:
            return
        enable = b"\x1b[?2004h"
        disable = b"\x1b[?2004l"
        with self._lock:
            data = (self._mode_tail or b"") + chunk
            last_enable = data.rfind(enable)
            last_disable = data.rfind(disable)
            if last_enable >= 0 or last_disable >= 0:
                new_state = last_enable > last_disable
                if new_state != self._bracketed_paste:
                    self._bracketed_paste = new_state
                    self._bracketed_paste_changed_at = time.monotonic()
            keep = max(len(enable), len(disable)) - 1
            self._mode_tail = data[-keep:] if keep > 0 else b""

    def _maybe_reply_to_terminal_queries(self, chunk: bytes) -> None:
        """Best-effort responses for programs that expect a terminal emulator.

        Our PTY runner is usually driven by a browser terminal (xterm.js). When no terminal client
        is attached, TUI programs that rely on Device Status Reports (DSR) can hang or exit.

        We only emulate a tiny subset of queries to keep headless operation workable.
        """
        if not chunk:
            return
        with self._lock:
            # If a real terminal is attached, let it handle DSR replies (avoid duplicate responses).
            if self._writer_fd is not None:
                return
            data = (self._query_tail or b"") + chunk

            # DSR: "Report Cursor Position" (CPR). Reply "ESC[{row};{col}R".
            # We do not attempt to track an actual cursor; a stable 1;1 is enough to unblock many TUIs.
            query = b"\x1b[6n"
            if query in data:
                try:
                    os.write(self._master_fd, b"\x1b[1;1R")
                except Exception:
                    pass

            keep = len(query) - 1
            self._query_tail = data[-keep:] if keep > 0 else b""

    def _on_cmd_readable(self) -> None:
        try:
            os.read(self._cmd_r, 65536)
        except Exception:
            pass

        while True:
            try:
                sock = self._attach_q.get_nowait()
            except queue.Empty:
                return
            self._attach_client_now(sock)

    def _attach_client_now(self, sock: socket.socket) -> None:
        fileno = int(sock.fileno())
        if fileno < 0:
            try:
                sock.close()
            except Exception:
                pass
            return
        try:
            sock.setblocking(False)
        except Exception:
            pass

        with self._lock:
            if fileno in self._clients:
                return
            writer = self._writer_fd is None
            if writer:
                self._writer_fd = fileno
            backlog = b"".join(self._backlog) if self._backlog else b""
            outbuf = bytearray(backlog)
            client = _PtyClient(sock=sock, writer=writer, outbuf=outbuf)
            self._clients[fileno] = client

        # Always register READ so the socket stays attached and disconnects can be observed.
        # WRITE is enabled only when there is pending backlog to flush.
        events = selectors.EVENT_READ
        if outbuf:
            events |= selectors.EVENT_WRITE
        try:
            self._selector.register(sock, events, data=("client", fileno))
        except Exception:
            self.detach_client(fileno)

    def _on_client_readable(self, fileno: int) -> None:
        with self._lock:
            client = self._clients.get(fileno)
            is_writer = bool(client and client.writer and self._writer_fd == fileno)
        if client is None:
            return

        try:
            data = client.sock.recv(65536)
        except BlockingIOError:
            return
        except Exception:
            self.detach_client(fileno)
            return
        if not data:
            self.detach_client(fileno)
            return
        if not is_writer:
            return
        try:
            os.write(self._master_fd, data)
        except Exception:
            pass

    def _on_client_writable(self, fileno: int) -> None:
        with self._lock:
            client = self._clients.get(fileno)
        if client is None:
            return

        if not client.outbuf:
            try:
                events = self._selector.get_key(client.sock).events
                self._selector.modify(client.sock, events & ~selectors.EVENT_WRITE, data=("client", fileno))
            except Exception:
                self.detach_client(fileno)
            return

        try:
            sent = client.sock.send(client.outbuf)
        except BlockingIOError:
            return
        except Exception:
            self.detach_client(fileno)
            return
        if sent > 0:
            del client.outbuf[:sent]

    def _loop(self) -> None:
        try:
            while self._running and self._proc.poll() is None:
                for key, mask in self._selector.select(timeout=0.1):
                    kind, meta = key.data if isinstance(key.data, tuple) else ("", None)
                    if kind == "pty":
                        if mask & selectors.EVENT_READ:
                            self._on_pty_readable()
                        continue
                    if kind == "cmd":
                        if mask & selectors.EVENT_READ:
                            self._on_cmd_readable()
                        continue
                    if kind == "client":
                        fileno = int(meta or -1)
                        if fileno < 0:
                            continue
                        if mask & selectors.EVENT_READ:
                            self._on_client_readable(fileno)
                        if mask & selectors.EVENT_WRITE:
                            self._on_client_writable(fileno)
        finally:
            self._running = False
            self._close_all()
            if self._on_exit is not None:
                try:
                    self._on_exit(self)
                except Exception:
                    pass


class PtySupervisor:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._sessions: Dict[Tuple[str, str], PtySession] = {}
        self._exit_hook: Optional[Callable[[PtySession], None]] = None

    def set_exit_hook(self, hook: Optional[Callable[[PtySession], None]]) -> None:
        with self._lock:
            self._exit_hook = hook

    def _drop_if_same(self, group_id: str, actor_id: str, session: PtySession) -> None:
        key = (group_id, actor_id)
        with self._lock:
            if self._sessions.get(key) is session:
                self._sessions.pop(key, None)

    def _on_session_exit(self, session: PtySession) -> None:
        try:
            self._drop_if_same(session.group_id, session.actor_id, session)
        finally:
            hook: Optional[Callable[[PtySession], None]] = None
            with self._lock:
                hook = self._exit_hook
            if hook is not None:
                try:
                    hook(session)
                except Exception:
                    pass

    def group_running(self, group_id: str) -> bool:
        gid = str(group_id or "").strip()
        if not gid:
            return False
        with self._lock:
            for (g, _), s in self._sessions.items():
                if g == gid and s.is_running():
                    return True
        return False

    def actor_running(self, group_id: str, actor_id: str) -> bool:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.get(key)
        return bool(s and s.is_running())

    def tail_output(self, *, group_id: str, actor_id: str, max_bytes: int = 2_000_000) -> bytes:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        if not key[0] or not key[1]:
            return b""
        with self._lock:
            s = self._sessions.get(key)
        if s is None:
            return b""
        try:
            return s.tail_output(max_bytes=int(max_bytes or 0))
        except Exception:
            return b""

    def clear_backlog(self, *, group_id: str, actor_id: str) -> bool:
        """Clear an actor's PTY backlog (returns False if actor not running)."""
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        if not key[0] or not key[1]:
            return False
        with self._lock:
            s = self._sessions.get(key)
        if s is None or not s.is_running():
            return False
        try:
            s.clear_backlog()
            return True
        except Exception:
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
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        if not key[0] or not key[1]:
            raise ValueError("missing group_id/actor_id")
        with self._lock:
            existing = self._sessions.get(key)
        if existing is not None and existing.is_running():
            return existing
        session = PtySession(
            group_id=key[0],
            actor_id=key[1],
            cwd=cwd,
            command=command,
            env=env,
            on_exit=self._on_session_exit,
            max_backlog_bytes=int(max_backlog_bytes or 0),
        )
        with self._lock:
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

    def attach(self, *, group_id: str, actor_id: str, sock: socket.socket) -> None:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.get(key)
        if s is None or not s.is_running():
            raise RuntimeError("actor not running")
        s.attach_client(sock)

    def bracketed_paste_enabled(self, *, group_id: str, actor_id: str) -> bool:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.get(key)
        return bool(s and s.is_running() and s.bracketed_paste_enabled())

    def bracketed_paste_status(self, *, group_id: str, actor_id: str) -> Tuple[bool, Optional[float]]:
        """Return (enabled, changed_at_monotonic) for bracketed paste mode."""
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.get(key)
        if s is None or not s.is_running():
            return (False, None)
        try:
            return (bool(s.bracketed_paste_enabled()), s.bracketed_paste_changed_at_monotonic())
        except Exception:
            return (False, None)

    def startup_times(self, *, group_id: str, actor_id: str) -> Tuple[Optional[float], Optional[float]]:
        """Return (started_at_monotonic, first_output_at_monotonic) for a running actor."""
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.get(key)
        if s is None or not s.is_running():
            return (None, None)
        try:
            return (s.started_at_monotonic(), s.first_output_at_monotonic())
        except Exception:
            return (None, None)

    def idle_seconds(self, *, group_id: str, actor_id: str) -> Optional[float]:
        """Return seconds since last PTY output for a running actor.

        Returns None if actor is not running.
        Used by automation to detect truly idle actors vs those actively working.
        """
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.get(key)
        if s is None or not s.is_running():
            return None
        try:
            return s.idle_seconds()
        except Exception:
            return None

    def session_key(self, *, group_id: str, actor_id: str) -> Optional[str]:
        """Return a stable key for the current PTY session (changes on restart).

        Used to scope "lazy preamble sent" state to a specific PTY session, not just actor_id.
        """
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        if not key[0] or not key[1]:
            return None
        with self._lock:
            s = self._sessions.get(key)
        if s is None or not s.is_running():
            return None
        try:
            pid = int(s.pid or 0)
            started_us = int(float(s.started_at_monotonic()) * 1_000_000)
            if pid > 0 and started_us > 0:
                return f"{pid}:{started_us}"
            if started_us > 0:
                return str(started_us)
            if pid > 0:
                return str(pid)
        except Exception:
            return None
        return None

    def resize(self, *, group_id: str, actor_id: str, cols: int, rows: int) -> None:
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.get(key)
        if s is None:
            return
        s.resize(cols=int(cols), rows=int(rows))

    def write_input(self, *, group_id: str, actor_id: str, data: bytes) -> bool:
        if not data:
            return True
        key = (str(group_id or "").strip(), str(actor_id or "").strip())
        with self._lock:
            s = self._sessions.get(key)
        if s is None or not s.is_running():
            return False
        return bool(s.write_input(data))


SUPERVISOR = PtySupervisor()
