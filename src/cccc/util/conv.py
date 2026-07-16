from __future__ import annotations

import math
from typing import Any

_TRUE = {"1", "true", "yes", "y", "on"}
_FALSE = {"0", "false", "no", "n", "off"}


def coerce_bool(value: Any, *, default: bool = False) -> bool:
    """Coerce a loosely-typed value into a boolean.

    This is used for user-authored config (YAML/JSON) where values may arrive as
    strings like "false"/"0". We treat unknown strings as the provided default
    to avoid the common pitfall where bool("false") == True.
    """
    if value is None:
        return bool(default)
    if isinstance(value, bool):
        return value
    if isinstance(value, int):
        return value != 0
    if isinstance(value, float):
        if math.isnan(value):
            return bool(default)
        return value != 0.0
    if isinstance(value, str):
        s = value.strip().lower()
        if not s:
            return bool(default)
        if s in _TRUE:
            return True
        if s in _FALSE:
            return False
        try:
            return int(s) != 0
        except Exception:
            return bool(default)
    return bool(value)
