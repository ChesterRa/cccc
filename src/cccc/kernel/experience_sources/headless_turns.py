from __future__ import annotations

from typing import Any, Dict, List, Optional

from ..context import _utc_now_iso

SOURCE_KIND_HEADLESS_TURN_SUCCESS = "headless.turn.success"


def is_headless_turn_prompt_candidate(prompt_text: str) -> bool:
    text = " ".join(str(prompt_text or "").split())
    if len(text) < 16:
        return False
    alpha_count = sum(1 for ch in text if ch.isalnum())
    return alpha_count >= 8


def build_headless_turn_success_candidate(
    *,
    prompt_text: str,
    by: str,
    actor_id: str,
    runtime: str,
    turn_id: str,
    event_id: str,
    source_refs: Optional[List[str]] = None,
    normalize_source_refs: Any,
    build_candidate_id: Any,
) -> Optional[Dict[str, Any]]:
    normalized_prompt = " ".join(str(prompt_text or "").split()).strip()
    if not is_headless_turn_prompt_candidate(normalized_prompt):
        return None
    refs = normalize_source_refs(source_refs or [])
    normalized_turn_id = str(turn_id or "").strip()
    normalized_event_id = str(event_id or "").strip()
    normalized_runtime = str(runtime or "").strip().lower()
    normalized_actor_id = str(actor_id or "").strip()
    stable_refs = normalize_source_refs(
        [
            f"actor:{normalized_actor_id}" if normalized_actor_id else "",
            f"runtime:{normalized_runtime}" if normalized_runtime else "",
            "headless:turn.success",
        ]
    )
    if normalized_turn_id and f"turn:{normalized_turn_id}" not in refs:
        refs.append(f"turn:{normalized_turn_id}")
    if normalized_event_id and f"event:{normalized_event_id}" not in refs:
        refs.append(f"event:{normalized_event_id}")
    if normalized_actor_id and f"actor:{normalized_actor_id}" not in refs:
        refs.append(f"actor:{normalized_actor_id}")
    now = _utc_now_iso()
    title = normalized_prompt[:72]
    summary = normalized_prompt[:240]
    applicability = f"Successful {normalized_runtime or 'headless'} turn pattern"
    recommended_action = normalized_prompt[:240]
    return {
        "id": build_candidate_id(
            source_kind=SOURCE_KIND_HEADLESS_TURN_SUCCESS,
            task_id="",
            summary=summary,
            source_refs=stable_refs,
        ),
        "status": "proposed",
        "source_kind": SOURCE_KIND_HEADLESS_TURN_SUCCESS,
        "task_id": normalized_turn_id,
        "source_refs": refs,
        "title": title,
        "summary": summary,
        "applicability": applicability,
        "recommended_action": recommended_action,
        "failure_signals": [],
        "proposed_by": str(by or "").strip(),
        "detail": {
            "by": str(by or "").strip(),
            "actor_id": normalized_actor_id,
            "runtime": normalized_runtime,
            "turn_id": normalized_turn_id,
            "event_id": normalized_event_id,
            "at": now,
        },
        "created_at": now,
        "updated_at": now,
    }
