from __future__ import annotations

from typing import Any, Dict


TERMINAL_TRANSCRIPT_VISIBILITY_VALUES = ("off", "foreman", "all")


DEFAULT_TERMINAL_TRANSCRIPT_SETTINGS: Dict[str, Any] = {
    # Which actors can read other actors' terminal transcripts.
    # Note: the human user is always allowed (not governed by this flag).
    "visibility": "foreman",
    # Whether to include tail snippets in idle notifications.
    "notify_tail": True,
    # How many lines to include in notification tail snippets.
    "notify_lines": 20,
}


def _as_bool(v: Any, default: bool) -> bool:
    if isinstance(v, bool):
        return v
    if isinstance(v, (int, float)):
        return bool(v)
    if isinstance(v, str):
        s = v.strip().lower()
        if s in ("1", "true", "yes", "y", "on"):
            return True
        if s in ("0", "false", "no", "n", "off"):
            return False
    return default


def _as_int(v: Any, default: int, *, min_value: int, max_value: int) -> int:
    try:
        n = int(v)
    except Exception:
        n = int(default)
    if n < min_value:
        n = min_value
    if n > max_value:
        n = max_value
    return int(n)


def _as_visibility(v: Any, default: str) -> str:
    s = str(v).strip().lower() if v is not None else ""
    return s if s in TERMINAL_TRANSCRIPT_VISIBILITY_VALUES else str(default)


def get_terminal_transcript_settings(group_doc: Any) -> Dict[str, Any]:
    """Return merged/validated terminal transcript settings from a group doc."""
    base = dict(DEFAULT_TERMINAL_TRANSCRIPT_SETTINGS)
    if not isinstance(group_doc, dict):
        return base
    raw = group_doc.get("terminal_transcript")
    if not isinstance(raw, dict):
        return base

    base["visibility"] = _as_visibility(raw.get("visibility"), base["visibility"])
    base["notify_tail"] = _as_bool(raw.get("notify_tail"), bool(base["notify_tail"]))
    base["notify_lines"] = _as_int(raw.get("notify_lines"), int(base["notify_lines"]), min_value=1, max_value=80)

    return base


def apply_terminal_transcript_patch(group_doc: Dict[str, Any], patch: Any) -> Dict[str, Any]:
    """Apply a patch (dict) to group_doc['terminal_transcript'] and return merged settings."""
    current = get_terminal_transcript_settings(group_doc)
    if not isinstance(patch, dict) or not patch:
        group_doc["terminal_transcript"] = dict(current)
        return dict(current)

    merged = dict(current)
    if "visibility" in patch:
        merged["visibility"] = _as_visibility(patch.get("visibility"), merged["visibility"])
    if "notify_tail" in patch:
        merged["notify_tail"] = _as_bool(patch.get("notify_tail"), bool(merged["notify_tail"]))
    if "notify_lines" in patch:
        merged["notify_lines"] = _as_int(patch.get("notify_lines"), int(merged["notify_lines"]), min_value=1, max_value=80)

    group_doc["terminal_transcript"] = dict(merged)
    return dict(merged)
