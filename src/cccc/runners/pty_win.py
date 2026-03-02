from __future__ import annotations

import os
import queue
import selectors
import socket
import subprocess
import threading
import time
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Dict, Iterable, Optional, Tuple

_WINPTY_PROCESS = None
try:
    from winpty import PtyProcess as _WINPTY_PROCESS  # type: ignore
except Exception:
    _WINPTY_PROCESS = None

PTY_SUPPORTED = bool(os.name == "nt" and _WINPTY_PROCESS is not None)


def _coerce_bytes(data: object) -> bytes:
    if isinstance(data, bytes):
        return data
    if isinstance(data, str):
        return data.encode("utf-8", errors="replace")
    return str(data or "").encode("utf-8", errors="replace")


def _looks_like_timeout(exc: BaseException) -> bool:
    if isinstance(exc, TimeoutError):
        return True
    msg = str(exc).strip().lower()
    return bool(msg and "timeout" in msg)


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
        if not PTY_SUPPORTED:
            raise RuntimeError("pty runner is not supported on this platform; install pywinpty for Windows ConPTY")

        self.group_id = group_id
        self.actor_id = actor_id
        self._on_exit = on_exit
        self._started_at = time.monotonic()
        self._first_output_at: Optional[float] = None
        self._last_output_at: Optional[float] = None
        self._max_backlog_bytes = int(max_backlog_bytes)
        slack = max(2_000_000, int(self._max_backlog_bytes // 8))
        self._max_client_buffer_bytes = max(int(max_client_buffer_bytes), int(self._max_backlog_bytes + slack))

        self._selector = selectors.DefaultSelector()
        self._lock = threading.Lock()
        self._clients: Dict[int, _PtyClient] = {}
        self._writer_fd: Optional[int] = None
        self._attach_q: "queue.Queue[socket.socket]" = queue.Queue()

        self._backlog: deque[bytes] = deque()
        self._backlog_bytes = 0
        self._mode_tail = b""
        self._query_tail = b""
        self._bracketed_paste = False
        self._bracketed_paste_changed_at: Optional[float] = None

        self._output_q: "queue.Queue[Optional[bytes]]" = queue.Queue()
        self._running = True

        wake_r, wake_w = socket.socketpair()
        wake_r.setblocking(False)
        wake_w.setblocking(False)
        self._wake_r = wake_r
        self._wake_w = wake_w
        self._selector.register(self._wake_r, selectors.EVENT_READ, data=("wake", None))

        cmd = [str(x) for x in command if isinstance(x, str) and str(x).strip()]
        if not cmd:
            cmd = ["cmd.exe"]
        cmdline = subprocess.list2cmdline(cmd)

        proc_env = os.environ.copy()
        proc_env.update({k: v for k, v in env.items() if isinstance(k, str) and isinstance(v, str)})
        proc_env.setdefault("TERM", "xterm-256color")

        spawn_err: Optional[Exception] = None
        proc = None
        for attempt in (
            lambda: _WINPTY_PROCESS.spawn(cmdline, cwd=str(cwd), env=proc_env, dimensions=(int(cols), int(rows))),  # type: ignore[misc]
            lambda: _WINPTY_PROCESS.spawn(cmdline, cwd=str(cwd), env=proc_env),  # type: ignore[misc]
            lambda: _WINPTY_PROCESS.spawn(cmdline, cwd=str(cwd)),  # type: ignore[misc]
            lambda: _WINPTY_PROCESS.spawn(cmdline),  # type: ignore[misc]
        ):
            try:
                proc = attempt()
                break
            except TypeError as e:
                spawn_err = e
                continue
            except Exception as e:
                spawn_err = e
                continue
        if proc is None:
            raise RuntimeError(f"failed to start ConPTY process: {spawn_err or 'spawn failed'}")

        self._proc = proc
        self._reader_thread = threading.Thread(
            target=self._reader_loop,
            name=f"cccc-conpty-read:{group_id}:{actor_id}",
            daemon=True,
        )
        self._reader_thread.start()

        self._thread = threading.Thread(
            target=self._loop,
            name=f"cccc-conpty:{group_id}:{actor_id}",
            daemon=True,
        )
        self._thread.start()

    @property
    def pid(self) -> int:
        try:
            return int(getattr(self._proc, "pid", 0) or 0)
        except Exception:
            return 0

    def _proc_alive(self) -> bool:
        try:
            fn = getattr(self._proc, "isalive", None)
            if callable(fn):
                return bool(fn())
        except Exception:
            return False
        try:
            status = getattr(self._proc, "exitstatus", None)
            if status is None:
                return True
            return False
        except Exception:
            pass
        try:
            return self.pid > 0
        except Exception:
            return False

    def is_running(self) -> bool:
        return bool(self._running) and self._proc_alive()

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
        with self._lock:
            return None if self._last_output_at is None else float(self._last_output_at)

    def idle_seconds(self) -> float:
        now = time.monotonic()
        with self._lock:
            if self._last_output_at is not None:
                return now - self._last_output_at
            return now - self._started_at

    def tail_output(self, *, max_bytes: int = 2_000_000) -> bytes:
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
        with self._lock:
            try:
                self._backlog.clear()
            except Exception:
                self._backlog = deque()
            self._backlog_bytes = 0
            self._mode_tail = b""
            self._query_tail = b""

    def _notify_wake(self) -> None:
        try:
            self._wake_w.send(b"x")
        except Exception:
            pass

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

    def _reader_loop(self) -> None:
        try:
            while self._running and self._proc_alive():
                try:
                    chunk = self._proc.read(65536)
                except Exception as e:
                    if _looks_like_timeout(e):
                        continue
                    break
                data = _coerce_bytes(chunk)
                if not data:
                    time.sleep(0.01)
                    continue
                self._maybe_reply_to_terminal_queries(data)
                self._update_input_modes(data)
                self._output_q.put(data)
                self._notify_wake()
        finally:
            self._output_q.put(None)
            self._notify_wake()

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
        if not chunk:
            return
        query = b"\x1b[6n"
        should_reply = False
        with self._lock:
            # If a real terminal is attached, avoid duplicating DSR replies.
            if self._writer_fd is not None:
                data = (self._query_tail or b"") + chunk
                keep = len(query) - 1
                self._query_tail = data[-keep:] if keep > 0 else b""
                return
            data = (self._query_tail or b"") + chunk
            if query in data:
                should_reply = True
            keep = len(query) - 1
            self._query_tail = data[-keep:] if keep > 0 else b""
        if should_reply:
            self.write_input(b"\x1b[1;1R")

    def _on_wake_readable(self) -> None:
        while True:
            try:
                if not self._wake_r.recv(65536):
                    break
            except BlockingIOError:
                break
            except Exception:
                break
        while True:
            try:
                sock = self._attach_q.get_nowait()
            except queue.Empty:
                break
            self._attach_client_now(sock)
        while True:
            try:
                chunk = self._output_q.get_nowait()
            except queue.Empty:
                break
            if chunk is None:
                self._running = False
                break
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

    def resize(self, *, cols: int, rows: int) -> None:
        if cols <= 0 or rows <= 0:
            return
        for method_name, args in (
            ("setwinsize", (int(rows), int(cols))),
            ("set_size", (int(cols), int(rows))),
        ):
            fn = getattr(self._proc, method_name, None)
            if callable(fn):
                try:
                    fn(*args)
                    return
                except Exception:
                    continue

    def write_input(self, data: bytes) -> bool:
        if not data:
            return True
        text = data.decode("utf-8", errors="replace")
        try:
            self._proc.write(text)
            return True
        except Exception:
            return False

    def _terminate_process(self) -> None:
        for name, args in (
            ("terminate", (True,)),
            ("terminate", ()),
            ("kill", ()),
            ("close", ()),
        ):
            fn = getattr(self._proc, name, None)
            if not callable(fn):
                continue
            try:
                fn(*args)
                if not self._proc_alive():
                    return
            except TypeError:
                continue
            except Exception:
                continue

    def stop(self) -> None:
        self._running = False
        self._terminate_process()
        self._notify_wake()
        try:
            if self._thread.is_alive():
                self._thread.join(timeout=1.0)
        except Exception:
            pass
        try:
            if self._reader_thread.is_alive():
                self._reader_thread.join(timeout=1.0)
        except Exception:
            pass

    def attach_client(self, sock: socket.socket) -> None:
        try:
            self._attach_q.put_nowait(sock)
        except Exception:
            try:
                sock.close()
            except Exception:
                pass
            return
        self._notify_wake()

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
        if not self.write_input(data):
            self.detach_client(fileno)

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

    def _close_all(self) -> None:
        try:
            self._selector.unregister(self._wake_r)
        except Exception:
            pass
        try:
            self._wake_r.close()
        except Exception:
            pass
        try:
            self._wake_w.close()
        except Exception:
            pass

        with self._lock:
            items = list(self._clients.items())
            self._clients.clear()
            self._writer_fd = None
        for _, client in items:
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

    def _loop(self) -> None:
        try:
            while self._running:
                if not self._proc_alive() and self._output_q.empty():
                    break
                for key, mask in self._selector.select(timeout=0.1):
                    kind, meta = key.data if isinstance(key.data, tuple) else ("", None)
                    if kind == "wake":
                        if mask & selectors.EVENT_READ:
                            self._on_wake_readable()
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
            self._terminate_process()
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
