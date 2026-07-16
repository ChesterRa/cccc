"""Global event log for system-wide events (cross-process safe).

Design:
- Daemon (and other local writers) append JSONL entries to a single file under CCCC_HOME.
- The web port tails this file over SSE to invalidate cached UI state (e.g., group list).

This avoids in-memory pub/sub, which cannot work across processes.
"""
from __future__ import annotations

import json
import uuid
from pathlib import Path
from typing import Any, Dict, Optional

from ..paths import ensure_home
from ..util.file_lock import acquire_lockfile, release_lockfile
from ..util.time import utc_now_iso


def global_events_path(home: Optional[Path] = None) -> Path:
    h = home or ensure_home()
    return h / "daemon" / "ccccd.events.jsonl"


def global_events_lock_path(home: Optional[Path] = None) -> Path:
    h = home or ensure_home()
    return h / "daemon" / "ccccd.events.lock"


def publish_event(kind: str, data: Dict[str, Any] | None = None) -> None:
    """Append a global event to the CCCC_HOME event log (best-effort)."""
    try:
        ev = {
            "v": 1,
            "id": uuid.uuid4().hex,
            "ts": utc_now_iso(),
            "kind": str(kind or "").strip(),
            "data": data if isinstance(data, dict) else {},
        }
        if not ev["kind"]:
            return
        line = json.dumps(ev, ensure_ascii=False)
        path = global_events_path()
        path.parent.mkdir(parents=True, exist_ok=True)
        lk = acquire_lockfile(global_events_lock_path(), blocking=True)
        try:
            with path.open("a", encoding="utf-8") as f:
                f.write(line + "\n")
        finally:
            release_lockfile(lk)
    except Exception:
        # Never fail the caller; this is an auxiliary invalidation mechanism.
        return
