"""Shared request/response helper for Group Bridge WebSocket peers."""

from __future__ import annotations

import asyncio
import threading
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable, Dict

from .ws_session import next_request_id

FrameSender = Callable[[Dict[str, Any]], Awaitable[None]]
SyncFrameSender = Callable[[Dict[str, Any]], None]


class AsyncGroupBridgeWsPeer:
    def __init__(self, send_frame: FrameSender) -> None:
        self._send_frame = send_frame
        self._pending: dict[str, asyncio.Future[Dict[str, Any]]] = {}

    async def send_request(self, request_doc: Dict[str, Any], timeout: float) -> Dict[str, Any]:
        request_id = next_request_id()
        loop = asyncio.get_running_loop()
        future: asyncio.Future[Dict[str, Any]] = loop.create_future()
        self._pending[request_id] = future
        await self._send_frame({"type": "request", "request_id": request_id, **dict(request_doc or {})})
        try:
            return await asyncio.wait_for(future, timeout=max(0.1, float(timeout or 0.1)))
        finally:
            self._pending.pop(request_id, None)

    def receive_response(self, frame: Dict[str, Any]) -> bool:
        response_to = str((frame or {}).get("response_to") or "").strip()
        future = self._pending.get(response_to)
        if future is None or future.done():
            return False
        future.set_result(dict((frame or {}).get("result") or {}))
        return True


class ThreadGroupBridgeWsPeer:
    def __init__(self, send_frame: SyncFrameSender) -> None:
        self._send_frame = send_frame
        self._pending: dict[str, _PendingSyncResponse] = {}
        self._lock = threading.Lock()

    def send_request(self, request_doc: Dict[str, Any], timeout: float) -> Dict[str, Any]:
        request_id = next_request_id()
        pending = _PendingSyncResponse()
        with self._lock:
            self._pending[request_id] = pending
        self._send_frame({"type": "request", "request_id": request_id, **dict(request_doc or {})})
        if not pending.ready.wait(max(0.1, float(timeout or 0.1))):
            with self._lock:
                self._pending.pop(request_id, None)
            return {"ok": False, "error": {"code": "peer_session_timeout", "message": "Group Bridge WebSocket session timed out"}}
        with self._lock:
            self._pending.pop(request_id, None)
        return dict(pending.result or {})

    def receive_response(self, frame: Dict[str, Any]) -> bool:
        response_to = str((frame or {}).get("response_to") or "").strip()
        with self._lock:
            pending = self._pending.get(response_to)
            if pending is None:
                return False
            pending.result = dict((frame or {}).get("result") or {})
            pending.ready.set()
        return True


@dataclass
class _PendingSyncResponse:
    ready: threading.Event = field(default_factory=threading.Event)
    result: Dict[str, Any] = field(default_factory=dict)
