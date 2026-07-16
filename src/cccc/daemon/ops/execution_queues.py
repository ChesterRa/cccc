"""Thin execution queues for daemon request-path separation and latency relief."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
import logging
from queue import Empty, Queue
import threading
from typing import Any, Callable, Deque, Dict, Optional, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse, build_async_result_fields


@dataclass
class _QueuedRequest:
    conn: Any
    req: Any


@dataclass
class GroupSpaceSyncRunTask:
    group_id: str
    provider: str
    force: bool
    by: str


class DaemonRequestExecutionQueue:
    """Single-writer request executor for daemon request handling."""

    def __init__(
        self,
        *,
        stop_event: threading.Event,
        handle_request: Callable[[Any], Tuple[Any, bool]],
        send_json: Callable[[Any, Dict[str, Any]], None],
        dump_response: Callable[[Any], Dict[str, Any]],
        logger: logging.Logger,
        on_should_exit: Callable[[], None],
    ) -> None:
        self._stop_event = stop_event
        self._handle_request = handle_request
        self._send_json = send_json
        self._dump_response = dump_response
        self._logger = logger
        self._on_should_exit = on_should_exit
        self._queue: Queue[_QueuedRequest] = Queue()

    def submit(self, *, conn: Any, req: Any) -> bool:
        if self._stop_event.is_set():
            return False
        self._queue.put(_QueuedRequest(conn=conn, req=req))
        return True

    def run_forever(self) -> None:
        while not self._stop_event.is_set():
            try:
                item = self._queue.get(timeout=0.2)
            except Empty:
                continue

            should_exit = False
            try:
                resp, should_exit = self._handle_request(item.req)
                try:
                    self._send_json(item.conn, self._dump_response(resp))
                except (BrokenPipeError, ConnectionResetError, OSError):
                    pass
            except Exception as exc:
                self._logger.exception("Unexpected error in request worker: %s", exc)
                try:
                    error_resp = DaemonResponse(
                        ok=False,
                        error=DaemonError(
                            code="internal_error",
                            message=f"internal error: {type(exc).__name__}: {exc}",
                        ),
                    )
                    self._send_json(item.conn, self._dump_response(error_resp))
                except Exception:
                    pass
            finally:
                try:
                    item.conn.close()
                except Exception:
                    pass
                self._queue.task_done()

            if should_exit:
                self._on_should_exit()
                self._stop_event.set()


class GroupSpaceSyncRunQueue:
    """Serialized queue for manual work-lane space sync runs."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._pending: Deque[GroupSpaceSyncRunTask] = deque()
        self._pending_by_group: Dict[Tuple[str, str], GroupSpaceSyncRunTask] = {}
        self._in_flight: set[Tuple[str, str]] = set()
        self._wake_event = threading.Event()

    @property
    def wake_event(self) -> threading.Event:
        return self._wake_event

    def submit(self, *, group_id: str, provider: str, force: bool, by: str) -> Dict[str, Any]:
        gid = str(group_id or "").strip()
        prv = str(provider or "notebooklm").strip() or "notebooklm"
        sender = str(by or "user").strip() or "user"
        if not gid:
            return {
                **build_async_result_fields(accepted=False, completed=False, queued=False),
                "reason": "missing_group_id",
            }

        key = (gid, prv)
        with self._lock:
            current = self._pending_by_group.get(key)
            if current is not None:
                if force and not current.force:
                    current.force = True
                if sender:
                    current.by = sender
                self._wake_event.set()
                return {
                    **build_async_result_fields(
                        accepted=True,
                        completed=False,
                        queued=False,
                        background=True,
                    ),
                    "reason": "already_pending",
                    "group_id": gid,
                    "provider": prv,
                    "lane": "work",
                    "force": bool(current.force),
                }
            if key in self._in_flight:
                task = GroupSpaceSyncRunTask(group_id=gid, provider=prv, force=bool(force), by=sender)
                self._pending.append(task)
                self._pending_by_group[key] = task
                self._wake_event.set()
                return {
                    **build_async_result_fields(
                        accepted=True,
                        completed=False,
                        queued=True,
                        background=True,
                    ),
                    "reason": "queued_after_running",
                    "group_id": gid,
                    "provider": prv,
                    "lane": "work",
                    "force": bool(task.force),
                }

            task = GroupSpaceSyncRunTask(group_id=gid, provider=prv, force=bool(force), by=sender)
            self._pending.append(task)
            self._pending_by_group[key] = task
            self._wake_event.set()
            return {
                **build_async_result_fields(
                    accepted=True,
                    completed=False,
                    queued=True,
                    background=True,
                ),
                "reason": "queued",
                "group_id": gid,
                "provider": prv,
                "lane": "work",
                "force": bool(force),
            }

    def drain(
        self,
        *,
        limit: int,
        runner: Callable[[GroupSpaceSyncRunTask], None],
        logger: Optional[logging.Logger] = None,
    ) -> int:
        processed = 0
        max_items = max(1, int(limit or 1))
        while processed < max_items:
            with self._lock:
                if not self._pending:
                    self._wake_event.clear()
                    break
                task = self._pending.popleft()
                key = (task.group_id, task.provider)
                self._pending_by_group.pop(key, None)
                self._in_flight.add(key)

            try:
                runner(task)
            except Exception as exc:
                if logger is not None:
                    logger.warning("group_space_sync_run failed group=%s provider=%s: %s", task.group_id, task.provider, exc)
            finally:
                with self._lock:
                    self._in_flight.discard(key)
                    if self._pending:
                        self._wake_event.set()
                    else:
                        self._wake_event.clear()
            processed += 1
        return processed
