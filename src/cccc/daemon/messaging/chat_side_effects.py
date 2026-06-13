"""Post-ledger side effects for chat operations."""

from __future__ import annotations

from typing import Any, Callable

from .post_commit import run_chat_post_commit


def schedule_chat_side_effects(
    *,
    group: Any,
    automation_on_new_message: Callable[[Any], None],
) -> None:
    run_chat_post_commit("chat-automation", lambda: automation_on_new_message(group))
