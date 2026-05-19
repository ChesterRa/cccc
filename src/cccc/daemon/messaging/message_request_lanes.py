"""Group-keyed execution lanes for visible chat request handling."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
import logging
import os
import threading
import time
from typing import Any, Callable, Deque, Dict, Tuple

from ...contracts.v1 import DaemonError, DaemonResponse


@dataclass
class _QueuedMessageRequest:
    conn: Any
    req: Any
    submitted_at: float


def default_message_request_max_concurrent_groups() -> int:
    raw = str(os.environ.get("CCCC_MESSAGE_REQUEST_MAX_CONCURRENT_GROUPS") or "").strip()
    try:
        value = int(raw)
    except Exception:
        value = 4
    return max(1, min(value, 32))


def message_request_lane_key(req: Any) -> str:
    args = getattr(req, "args", None)
    if isinstance(args, dict):
        group_id = str(args.get("group_id") or "").strip()
        if group_id:
            return group_id
    return "global"


def _request_arg(req: Any, key: str) -> str:
    args = getattr(req, "args", None)
    if not isinstance(args, dict):
        return ""
    return str(args.get(key) or "").strip()


class MessageRequestLanes:
    """Serialize message writes per group while allowing different groups to run concurrently."""

    def __init__(
        self,
        *,
        stop_event: threading.Event,
        handle_request: Callable[[Any], Tuple[Any, bool]],
        send_json: Callable[[Any, Dict[str, Any]], None],
        dump_response: Callable[[Any], Dict[str, Any]],
        logger: logging.Logger,
        on_should_exit: Callable[[], None],
        max_concurrent_groups: int | None = None,
        slow_request_seconds: float = 5.0,
        diagnostics_enabled: Callable[[], bool] | None = None,
    ) -> None:
        self._stop_event = stop_event
        self._handle_request = handle_request
        self._send_json = send_json
        self._dump_response = dump_response
        self._logger = logger
        self._on_should_exit = on_should_exit
        self._diagnostics_enabled = diagnostics_enabled
        self._max_concurrent_groups = max(1, int(max_concurrent_groups or default_message_request_max_concurrent_groups()))
        self._slow_request_seconds = max(0.1, float(slow_request_seconds or 5.0))
        self._condition = threading.Condition()
        self._pending: Dict[str, Deque[_QueuedMessageRequest]] = {}
        self._running: set[str] = set()

    def submit(self, *, conn: Any, req: Any) -> bool:
        if self._stop_event.is_set():
            return False
        lane_key = message_request_lane_key(req)
        should_start = False
        with self._condition:
            self._pending.setdefault(lane_key, deque()).append(
                _QueuedMessageRequest(conn=conn, req=req, submitted_at=time.monotonic())
            )
            should_start = self._can_start_locked(lane_key)
            if should_start:
                self._running.add(lane_key)
        if should_start:
            self._start_lane(lane_key)
        return True

    def wait_for_idle_for_tests(self, *, timeout: float = 2.0) -> bool:
        deadline = time.monotonic() + max(0.0, float(timeout))
        with self._condition:
            while self._pending or self._running:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    return False
                self._condition.wait(timeout=remaining)
            return True

    def _can_start_locked(self, lane_key: str) -> bool:
        if lane_key in self._running:
            return False
        if len(self._running) >= self._max_concurrent_groups:
            return False
        return bool(self._pending.get(lane_key))

    def _start_lane(self, lane_key: str) -> None:
        thread = threading.Thread(
            target=self._drain_lane,
            args=(lane_key,),
            name=f"cccc-message-request-{lane_key}",
            daemon=True,
        )
        try:
            thread.start()
        except Exception:
            self._logger.exception("message request lane could not start key=%s", lane_key)
            self._drain_lane(lane_key)

    def _start_waiting_lanes_locked(self, *, yielded_lane_key: str = "") -> list[str]:
        lane_keys: list[str] = []
        while len(self._running) < self._max_concurrent_groups:
            next_key = ""
            for key, queue in self._pending.items():
                if key != yielded_lane_key and key not in self._running and queue:
                    next_key = key
                    break
            if not next_key and yielded_lane_key:
                queue = self._pending.get(yielded_lane_key)
                if yielded_lane_key not in self._running and queue:
                    next_key = yielded_lane_key
            if not next_key:
                break
            self._running.add(next_key)
            lane_keys.append(next_key)
        return lane_keys

    def _has_waiting_lane_locked(self, lane_key: str) -> bool:
        for key, queue in self._pending.items():
            if key != lane_key and key not in self._running and queue:
                return True
        return False

    def _drain_lane(self, lane_key: str) -> None:
        next_lanes: list[str] = []
        while not self._stop_event.is_set():
            with self._condition:
                queue = self._pending.get(lane_key)
                if not queue:
                    self._pending.pop(lane_key, None)
                    self._running.discard(lane_key)
                    next_lanes = self._start_waiting_lanes_locked()
                    self._condition.notify_all()
                    break
                item = queue.popleft()

            should_exit = False
            started_at = time.monotonic()
            try:
                resp, should_exit = self._handle_request(item.req)
                try:
                    self._send_json(item.conn, self._dump_response(resp))
                except (BrokenPipeError, ConnectionResetError, OSError):
                    pass
            except Exception as exc:
                self._logger.exception("Unexpected error in message request lane: %s", exc)
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
                elapsed = time.monotonic() - started_at
                op = str(getattr(item.req, "op", "") or "unknown").strip() or "unknown"
                queue_wait_ms = int(max(0.0, started_at - item.submitted_at) * 1000)
                run_ms = int(elapsed * 1000)
                if self._diagnostics_enabled and self._diagnostics_enabled():
                    self._logger.info(
                        "message request done op=%s group=%s client_id=%s reply_to=%s queue_wait_ms=%d run_ms=%d",
                        op,
                        lane_key,
                        _request_arg(item.req, "client_id"),
                        _request_arg(item.req, "reply_to"),
                        queue_wait_ms,
                        run_ms,
                    )
                if elapsed >= self._slow_request_seconds:
                    self._logger.warning(
                        "slow message request op=%s group=%s queue_wait_ms=%d run_ms=%d",
                        op,
                        lane_key,
                        queue_wait_ms,
                        run_ms,
                    )
                try:
                    item.conn.close()
                except Exception:
                    pass

            if should_exit:
                self._on_should_exit()
                self._stop_event.set()
                break

            with self._condition:
                queue = self._pending.get(lane_key)
                if queue and self._has_waiting_lane_locked(lane_key):
                    self._running.discard(lane_key)
                    next_lanes = self._start_waiting_lanes_locked(yielded_lane_key=lane_key)
                    self._condition.notify_all()
                    break

        for next_lane in next_lanes:
            self._start_lane(next_lane)
