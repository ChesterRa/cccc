from __future__ import annotations

from typing import Any, Dict, Optional


DEFAULT_ASYNC_COMPLETION_SIGNAL = "system.notify"


def build_async_result_fields(
    *,
    accepted: bool,
    completed: bool,
    queued: Optional[bool] = None,
    background: Optional[bool] = None,
    completion_signal: str = "",
    recommended_next_action: str = "",
    polling_discouraged: Optional[bool] = None,
    wait_guidance: str = "",
) -> Dict[str, Any]:
    payload: Dict[str, Any] = {
        "accepted": bool(accepted),
        "completed": bool(completed),
    }
    if queued is not None:
        payload["queued"] = bool(queued)
    if background is not None:
        payload["background"] = bool(background)
    if bool(accepted) and not bool(completed):
        signal = str(completion_signal or "").strip()
        if signal:
            payload["completion_signal"] = signal
        next_action = str(recommended_next_action or "").strip()
        if next_action:
            payload["recommended_next_action"] = next_action
        if polling_discouraged is not None:
            payload["polling_discouraged"] = bool(polling_discouraged)
        guidance = str(wait_guidance or "").strip()
        if guidance:
            payload["wait_guidance"] = guidance
    return payload