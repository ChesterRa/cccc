from __future__ import annotations

import logging
from typing import Any

from ..kernel.experience import SOURCE_KIND_HEADLESS_TURN_SUCCESS, extract_experience_candidate
from ..kernel.group import load_group

logger = logging.getLogger(__name__)


def maybe_capture_headless_turn_success_experience(
    *,
    group_id: str,
    actor_id: str,
    runtime: str,
    turn_id: str,
    event_id: str,
    prompt_text: str,
) -> None:
    normalized_group_id = str(group_id or "").strip()
    normalized_actor_id = str(actor_id or "").strip()
    normalized_turn_id = str(turn_id or "").strip()
    normalized_prompt = str(prompt_text or "").strip()
    if not normalized_group_id or not normalized_actor_id or not normalized_turn_id or not normalized_prompt:
        return
    group = load_group(normalized_group_id)
    if group is None:
        return
    try:
        extract_experience_candidate(
            group=group,
            source_kind=SOURCE_KIND_HEADLESS_TURN_SUCCESS,
            payload={
                "prompt_text": normalized_prompt,
                "by": normalized_actor_id,
                "actor_id": normalized_actor_id,
                "runtime": str(runtime or "").strip().lower(),
                "turn_id": normalized_turn_id,
                "event_id": str(event_id or "").strip(),
                "source_refs": [
                    "headless:turn.success",
                    f"actor:{normalized_actor_id}",
                ],
            },
            dry_run=False,
        )
    except Exception:
        logger.exception(
            "headless turn experience capture failed: group=%s actor=%s turn=%s",
            normalized_group_id,
            normalized_actor_id,
            normalized_turn_id,
        )
