from __future__ import annotations

from datetime import datetime, timezone
from typing import Optional


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def parse_utc_iso(ts: str) -> Optional[datetime]:
    s = (ts or "").strip()
    if not s:
        return None
    try:
        if s.endswith("Z"):
            s = s[: -len("Z")] + "+00:00"
        dt = datetime.fromisoformat(s)
        if dt.tzinfo is None:
            dt = dt.replace(tzinfo=timezone.utc)
        return dt.astimezone(timezone.utc)
    except Exception:
        return None
