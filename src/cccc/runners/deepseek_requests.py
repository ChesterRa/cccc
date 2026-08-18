"""Serialized request/response coordination for the DeepSeek ACP supervisor."""
from __future__ import annotations

import json
import queue
import time
from typing import Any, Dict

from ..kernel.deepseek_acp import ACPProtocolError


class DeepSeekRequestMixin:
    def next_frame(self, timeout: float = 5.0) -> Dict[str, Any]:
        try:
            frame = self._frames.get(timeout=max(0.1, float(timeout)))
        except queue.Empty as exc:
            raise TimeoutError("deepseek ACP frame timed out") from exc
        if frame is None:
            raise RuntimeError("deepseek ACP process ended") from self._reader_error
        if frame.get("method") == "session/request_permission" and "id" in frame:
            self._pending_permissions.add(frame.get("id"))
        if "id" in frame and frame.get("id") == self._active_request_id:
            self._active_request_id = None
        return frame

    def respond_permission(self, request_id: Any, options: Any, *, stopping: bool = False) -> None:
        from ..kernel.deepseek_acp import permission_outcome

        self._send_notification(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": permission_outcome(options, stopping=stopping),
            }
        )
        self._pending_permissions.discard(request_id)

    def _run_queue(self) -> None:
        while not self._stopping.is_set():
            if self._active_request_id is not None:
                self._stopping.wait(0.05)
                continue
            try:
                turn = self.queue.get(timeout=0.1)
            except queue.Empty:
                continue
            if turn.generation != self.generation or self._process is None or self._process.stdin is None:
                continue
            frame = {
                "jsonrpc": "2.0",
                "id": turn.request_id,
                "method": "session/prompt",
                "params": {
                    "sessionId": self._session_id,
                    "prompt": [{"type": "text", "text": turn.prompt}],
                },
            }
            try:
                self._send_request(frame, request_id=turn.request_id, activate=True)
            except (BrokenPipeError, OSError, ACPProtocolError, RuntimeError):
                self._stopping.set()

    def _send_request(
        self,
        frame: Dict[str, Any],
        *,
        request_id: int,
        activate: bool = False,
    ) -> None:
        process = self._process
        if process is None or process.stdin is None or self._stopping.is_set():
            raise RuntimeError("deepseek supervisor is not running")
        with self._protocol_lock:
            if activate:
                self._active_request_id = request_id
            try:
                self._protocol.register(request_id)
                process.stdin.write(
                    (json.dumps(frame, separators=(",", ":")) + "\n").encode("utf-8")
                )
                process.stdin.flush()
            except Exception:
                self._protocol.pending.discard(request_id)
                if activate and self._active_request_id == request_id:
                    self._active_request_id = None
                raise

    def _send_notification(self, frame: Dict[str, Any]) -> None:
        process = self._process
        if process is None or process.stdin is None or self._stopping.is_set():
            raise RuntimeError("deepseek supervisor is not running")
        process.stdin.write((json.dumps(frame, separators=(",", ":")) + "\n").encode("utf-8"))
        process.stdin.flush()

    def _recv_response(self, request_id: int, timeout: float) -> Dict[str, Any]:
        deadline = time.monotonic() + max(0.1, float(timeout))
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("deepseek ACP response timed out")
            try:
                frame = self._frames.get(timeout=remaining)
            except queue.Empty as exc:
                raise TimeoutError("deepseek ACP response timed out") from exc
            if frame is None:
                if self._reader_error is not None:
                    raise RuntimeError("deepseek ACP reader stopped") from self._reader_error
                raise RuntimeError("deepseek ACP process ended before responding")
            if "id" not in frame:
                continue
            if frame.get("id") != request_id:
                raise RuntimeError("deepseek ACP response id did not match request")
            if self._active_request_id == request_id:
                self._active_request_id = None
            return frame
