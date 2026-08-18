"""Bounded DeepSeek ACP subprocess supervisor.

The provider adapter owns only process/lifecycle concerns.  Ledger durability
and cursor advancement remain the caller's responsibility, so a prompt is not
considered read merely because it entered this queue.
"""
from __future__ import annotations

import os
import queue
import signal
import subprocess
import threading
import uuid
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from .deepseek_streams import DeepSeekStreamMixin, merge_subprocess_env
from .deepseek_requests import DeepSeekRequestMixin
from ..kernel.deepseek_acp import (
    NDJSONSession,
    initialize_request,
    session_new_request,
    validate_initialize_result,
    validate_session_new_result,
)

QUEUE_CAPACITY = 256
STDERR_TAIL_BYTES = 16 * 1024


@dataclass(frozen=True)
class DeepSeekTurn:
    request_id: int
    prompt: str
    generation: str


class DeepSeekSupervisor(DeepSeekRequestMixin, DeepSeekStreamMixin):
    """One actor, one ACP process, one active prompt at a time."""

    def __init__(self, command: List[str], *, cwd: str, env: Dict[str, str], queue_capacity: int = QUEUE_CAPACITY) -> None:
        self.command = list(command)
        self.cwd = cwd
        self.env = dict(env)
        self.queue: "queue.Queue[DeepSeekTurn]" = queue.Queue(maxsize=max(1, int(queue_capacity)))
        self.generation = ""
        self._next_id = 1
        self._process: Optional[subprocess.Popen[bytes]] = None
        self._worker: Optional[threading.Thread] = None
        self._stdout_reader: Optional[threading.Thread] = None
        self._frames: "queue.Queue[Optional[Dict[str, Any]]]" = queue.Queue(maxsize=512)
        self._protocol = NDJSONSession()
        self._protocol_lock = threading.Lock()
        self._reader_error: Optional[BaseException] = None
        self._session_id = ""
        self._active_request_id: Optional[int] = None
        self._pending_permissions: set[Any] = set()
        self._stopping = threading.Event()
        self._stderr_tail = bytearray()
        self._lock = threading.Lock()

    @property
    def stderr_tail(self) -> str:
        with self._lock:
            return bytes(self._stderr_tail).decode("utf-8", errors="replace")

    def is_running(self) -> bool:
        process = self._process
        return bool(process is not None and process.poll() is None and not self._stopping.is_set())

    def start(self) -> str:
        if self._process is not None and self._process.poll() is None:
            return self.generation
        if not self.command:
            raise ValueError("deepseek command is empty")
        self.generation = uuid.uuid4().hex
        self._stopping.clear()
        self._process = subprocess.Popen(
            self.command,
            cwd=self.cwd,
            env=merge_subprocess_env(self.env),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=(os.name != "nt"),
        )
        self._next_id = 1
        self._frames = queue.Queue(maxsize=512)
        threading.Thread(target=self._read_stderr, daemon=True).start()
        self._protocol = NDJSONSession()
        self._reader_error = None
        self._session_id = ""
        self._active_request_id = None
        self._pending_permissions = set()
        generation = self.generation
        frames = self._frames
        process = self._process
        if process is None:
            raise RuntimeError("deepseek process failed to start")
        self._stdout_reader = threading.Thread(
            target=self._read_stdout,
            args=(process, generation, frames),
            daemon=True,
        )
        self._stdout_reader.start()
        self._worker = threading.Thread(target=self._run_queue, daemon=True)
        self._worker.start()
        return self.generation

    @property
    def session_id(self) -> str:
        return self._session_id

    def handshake(self, *, timeout: float = 5.0) -> str:
        """Perform initialize + session/new; failure leaves this generation unusable."""
        if not self.is_running():
            raise RuntimeError("deepseek supervisor is not running")
        self._send_request(initialize_request(), request_id=1)
        initialize = self._recv_response(1, timeout)
        validate_initialize_result(initialize)
        self._send_request(session_new_request(self.cwd), request_id=2)
        created = self._recv_response(2, timeout)
        self._session_id = validate_session_new_result(created, seen=set())
        self._next_id = max(self._next_id, 3)
        return self._session_id

    def submit(self, prompt: str) -> int:
        if self._process is None or self._process.poll() is not None or self._stopping.is_set():
            raise RuntimeError("deepseek supervisor is not running")
        if not self._session_id:
            raise RuntimeError("deepseek ACP handshake is required before submit")
        request_id = self._next_id
        self._next_id += 1
        turn = DeepSeekTurn(request_id=request_id, prompt=str(prompt), generation=self.generation)
        try:
            self.queue.put_nowait(turn)
        except queue.Full as exc:
            raise RuntimeError("deepseek prompt queue is full") from exc
        return request_id

    def cancel(self) -> None:
        if not self._session_id:
            raise RuntimeError("deepseek ACP handshake is required before cancel")
        self._send_notification(
            {
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": {"sessionId": self._session_id},
            }
        )

    def stop(self, *, timeout: float = 2.0) -> None:
        process = self._process
        if process is None:
            return
        # ACP stop is ordered: ask the active session to cancel and resolve
        # every permission request before closing stdin/terminating the group.
        if self._session_id and not self._stopping.is_set():
            try:
                self.cancel()
            except (OSError, RuntimeError):
                pass
            for permission_id in tuple(self._pending_permissions):
                try:
                    self.respond_permission(permission_id, [], stopping=True)
                except (OSError, RuntimeError):
                    break
        self._stopping.set()
        if process.stdin is not None:
            try:
                process.stdin.close()
            except OSError:
                pass
        try:
            if process.poll() is None and os.name != "nt":
                os.killpg(process.pid, signal.SIGTERM)
            elif process.poll() is None:
                process.terminate()
            process.wait(timeout=max(0.1, timeout))
        except subprocess.TimeoutExpired:
            if os.name != "nt":
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            else:
                process.kill()
            process.wait(timeout=max(0.1, timeout))
        finally:
            self.generation = ""
            self._process = None
            self._session_id = ""
            self._active_request_id = None
            self._pending_permissions.clear()
            if self._stdout_reader is not None:
                self._stdout_reader.join(timeout=0.5)
                self._stdout_reader = None
            while True:
                try:
                    self.queue.get_nowait()
                except queue.Empty:
                    break
