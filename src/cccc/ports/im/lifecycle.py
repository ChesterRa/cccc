"""Processing feedback lifecycle for IM bridge messages."""

from __future__ import annotations

import time
from collections import deque
from dataclasses import dataclass
from typing import Deque, Dict, Optional

from .adapters.base import IMAdapter, IMProcessingContext, IMProcessingOutcome


@dataclass
class _ActiveProcessing:
    context: IMProcessingContext
    handle: Optional[str]
    started_at: float
    last_action_ts: float = 0.0


class IMProcessingLifecycle:
    """Owns platform feedback state for accepted inbound IM messages."""

    def __init__(
        self,
        adapter: IMAdapter,
        *,
        action_interval_seconds: float = 4.0,
        processing_timeout_seconds: float = 30 * 60,
    ) -> None:
        self._adapter = adapter
        self._action_interval_seconds = action_interval_seconds
        self._processing_timeout_seconds = max(0.0, processing_timeout_seconds)
        self._active: Dict[str, Deque[_ActiveProcessing]] = {}

    @staticmethod
    def _key(chat_id: str, thread_id: int = 0) -> str:
        return f"{str(chat_id)}:{int(thread_id or 0)}"

    def start(
        self,
        *,
        chat_id: str,
        thread_id: int = 0,
        message_id: str = "",
        source_event_id: str = "",
    ) -> None:
        now = time.monotonic()
        self._expire_stale(now)
        context = IMProcessingContext(
            chat_id=str(chat_id),
            thread_id=int(thread_id or 0),
            message_id=str(message_id or ""),
            platform=str(getattr(self._adapter, "platform", "") or "unknown"),
            source_event_id=str(source_event_id or ""),
        )
        start = getattr(self._adapter, "on_processing_start", None)
        handle = start(context) if callable(start) else self._legacy_start(context)
        key = self._key(context.chat_id, context.thread_id)
        self._active.setdefault(key, deque()).append(
            _ActiveProcessing(
                context=context,
                handle=handle,
                started_at=now,
                last_action_ts=now if handle == "typing" else 0.0,
            )
        )
        self._send_action_if_due(key, force=not bool(handle))

    def refresh(self) -> None:
        self._expire_stale(time.monotonic())
        for key in list(self._active):
            self._send_action_if_due(key)

    def complete(
        self,
        chat_id: str,
        outcome: IMProcessingOutcome = IMProcessingOutcome.SUCCESS,
        *,
        thread_id: int = 0,
        reply_to: str = "",
    ) -> None:
        self._expire_stale(time.monotonic())
        key = self._key(chat_id, thread_id)
        queue = self._active.get(key)
        if not queue:
            return
        normalized_reply_to = str(reply_to or "").strip()
        if normalized_reply_to:
            matched_index = next(
                (
                    index
                    for index, item in enumerate(queue)
                    if item.context.source_event_id == normalized_reply_to
                ),
                None,
            )
            if matched_index is None:
                return
            active = queue[matched_index]
            del queue[matched_index]
        elif len(queue) == 1:
            # Compatibility for old ledger events that predate reply_to.
            active = queue.popleft()
        else:
            # FIFO is unsafe when concurrent inbound messages share a chat.
            return
        if not queue:
            self._active.pop(key, None)
        self._finish(active, outcome)

    def _finish(self, active: _ActiveProcessing, outcome: IMProcessingOutcome) -> None:
        complete = getattr(self._adapter, "on_processing_complete", None)
        if callable(complete):
            complete(active.context, outcome, active.handle)
        else:
            self._legacy_complete(active.context, active.handle)

    def _expire_stale(self, now: float) -> None:
        for key in list(self._active):
            queue = self._active.get(key)
            if not queue:
                continue
            while queue and now - queue[0].started_at >= self._processing_timeout_seconds:
                self._finish(queue.popleft(), IMProcessingOutcome.FAILURE)
            if not queue:
                self._active.pop(key, None)

    def clear(self, chat_id: str, *, thread_id: int = 0) -> None:
        self.complete(chat_id, IMProcessingOutcome.CANCELLED, thread_id=thread_id)

    def _send_action_if_due(self, key: str, *, force: bool = False) -> None:
        queue = self._active.get(key)
        if not queue:
            return
        active = queue[0]
        now = time.monotonic()
        if not force and now - active.last_action_ts < self._action_interval_seconds:
            return
        send_chat_action = getattr(self._adapter, "send_chat_action", None)
        if callable(send_chat_action) and send_chat_action(active.context.chat_id, "typing"):
            for item in queue:
                item.last_action_ts = now

    def _legacy_start(self, context: IMProcessingContext) -> Optional[str]:
        add_reaction = getattr(self._adapter, "add_reaction", None)
        if context.message_id and callable(add_reaction):
            reaction_id = add_reaction(context.message_id)
            if reaction_id:
                return f"reaction:{reaction_id}"
        send_chat_action = getattr(self._adapter, "send_chat_action", None)
        if callable(send_chat_action) and send_chat_action(context.chat_id):
            return "typing"
        return None

    def _legacy_complete(self, context: IMProcessingContext, handle: Optional[str]) -> None:
        if not handle or not handle.startswith("reaction:"):
            return
        remove_reaction = getattr(self._adapter, "remove_reaction", None)
        reaction_id = handle.removeprefix("reaction:")
        if context.message_id and reaction_id and callable(remove_reaction):
            remove_reaction(context.message_id, reaction_id)
