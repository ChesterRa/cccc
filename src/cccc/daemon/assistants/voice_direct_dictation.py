from __future__ import annotations

from typing import Any


DIRECT_COMPOSER_TARGET = "composer"


def normalize_voice_dispatch_target(value: Any) -> str:
    return str(value or "").strip().lower()


def is_direct_composer_dictation(value: Any) -> bool:
    return normalize_voice_dispatch_target(value) == DIRECT_COMPOSER_TARGET


def disabled_assistant_allows_recording(action: Any, dispatch_target: Any) -> bool:
    return (
        str(action or "").strip().lower() in {"acquire", "heartbeat"}
        and is_direct_composer_dictation(dispatch_target)
    )
