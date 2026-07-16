"""Processing feedback lifecycle for IM bridge messages."""

from __future__ import annotations

import time
from dataclasses import dataclass
from typing import Dict, Optional

from .adapters.base import IMAdapter, IMProcessingContext, IMProcessingOutcome


@dataclass
class _ActiveProcessing:
    context: IMProcessingContext
    handle: Optional[str]
    last_action_ts: float = 0.0


class IMProcessingLifecycle:
    """Owns platform feedback state for accepted inbound IM messages."""

    def __init__(self, adapter: IMAdapter, *, action_interval_seconds: float = 4.0) -> None:
        self._adapter = adapter
        self._action_interval_seconds = action_interval_seconds
        self._active: Dict[str, _ActiveProcessing] = {}

    def start(self, *, chat_id: str, thread_id: int = 0, message_id: str = "") -> None:
        context = IMProcessingContext(
            chat_id=str(chat_id),
            thread_id=int(thread_id or 0),
            message_id=str(message_id or ""),
            platform=str(getattr(self._adapter, "platform", "") or "unknown"),
        )
        start = getattr(self._adapter, "on_processing_start", None)
        handle = start(context) if callable(start) else self._legacy_start(context)
        self._active[context.chat_id] = _ActiveProcessing(
            context=context,
            handle=handle,
            last_action_ts=time.time() if handle == "typing" else 0.0,
        )
        self._send_action_if_due(context.chat_id, force=not bool(handle))

    def refresh(self) -> None:
        for chat_id in list(self._active):
            self._send_action_if_due(chat_id)

    def complete(self, chat_id: str, outcome: IMProcessingOutcome = IMProcessingOutcome.SUCCESS) -> None:
        active = self._active.pop(str(chat_id), None)
        if not active:
            return
        complete = getattr(self._adapter, "on_processing_complete", None)
        if callable(complete):
            complete(active.context, outcome, active.handle)
        else:
            self._legacy_complete(active.context, active.handle)

    def clear(self, chat_id: str) -> None:
        self.complete(chat_id, IMProcessingOutcome.CANCELLED)

    def _send_action_if_due(self, chat_id: str, *, force: bool = False) -> None:
        active = self._active.get(str(chat_id))
        if not active:
            return
        now = time.time()
        if not force and now - active.last_action_ts < self._action_interval_seconds:
            return
        send_chat_action = getattr(self._adapter, "send_chat_action", None)
        if callable(send_chat_action) and send_chat_action(active.context.chat_id, "typing"):
            active.last_action_ts = now

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
