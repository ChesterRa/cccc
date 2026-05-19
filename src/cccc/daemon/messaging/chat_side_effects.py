"""Post-ledger side effects for chat operations."""

from __future__ import annotations

from typing import Any, Callable

from ..pet.profile_refresh import record_user_chat_message
from ..pet.review_scheduler import request_pet_review
from .post_commit import run_chat_post_commit


def schedule_chat_side_effects(
    *,
    group: Any,
    by: str,
    event_id: str,
    event_ts: str,
    text: str,
    pet_review_reason: str,
    pet_review_immediate: bool,
    automation_on_new_message: Callable[[Any], None],
) -> None:
    run_chat_post_commit("chat-automation", lambda: automation_on_new_message(group))
    try:
        request_pet_review(
            group.group_id,
            reason=pet_review_reason,
            source_event_id=event_id,
            immediate=pet_review_immediate,
        )
    except Exception:
        pass
    if by != "user":
        return
    run_chat_post_commit(
        "user-profile-record",
        lambda: record_user_chat_message(
            group.group_id,
            event_id=event_id,
            ts=event_ts,
            text=text,
        ),
    )
