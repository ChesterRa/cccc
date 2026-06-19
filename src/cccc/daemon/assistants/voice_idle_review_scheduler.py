from __future__ import annotations

from typing import Set

from .. import assistive_jobs
from . import assistant_ops


VOICE_IDLE_REVIEW_DEBOUNCE_SECONDS = 0.0
VOICE_IDLE_REVIEW_MIN_INTERVAL_SECONDS = 0.0
VOICE_IDLE_REVIEW_MAX_DELAY_SECONDS = 0.0
VOICE_IDLE_REVIEW_LEASE_SECONDS = 180.0


def _can_idle_review_now(group_id: str) -> bool:
    return bool(assistant_ops.voice_idle_review_available(group_id))


def _idle_review_unavailable_reason(group_id: str) -> str:
    return str(assistant_ops.voice_idle_review_unavailable_reason(group_id) or "idle_review_unavailable")


def _dispatch_idle_review(
    group_id: str,
    *,
    reasons: Set[str],
    source_event_id: str,
    trigger_class: str,
) -> bool:
    return bool(
        assistant_ops.dispatch_voice_idle_review(
            group_id,
            reasons=set(reasons),
            source_event_id=source_event_id,
            trigger_class=trigger_class,
        )
    )


def request_voice_idle_review(
    group_id: str,
    *,
    reason: str = "document_updated",
    source_event_id: str = "",
    immediate: bool = False,
) -> bool:
    return assistive_jobs.request_job(
        group_id,
        job_kind=assistive_jobs.JOB_KIND_VOICE_IDLE_REVIEW,
        trigger_class=assistive_jobs.TRIGGER_EVENT,
        reason=reason,
        source_event_id=source_event_id,
        immediate=immediate,
    )


def mark_voice_idle_review_completed(group_id: str) -> bool:
    return assistive_jobs.mark_job_completed(group_id, assistive_jobs.JOB_KIND_VOICE_IDLE_REVIEW)


def recover_pending_voice_idle_reviews() -> None:
    assistive_jobs.recover_jobs(job_kinds=(assistive_jobs.JOB_KIND_VOICE_IDLE_REVIEW,))
