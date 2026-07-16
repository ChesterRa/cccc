from __future__ import annotations

import json
import logging
import sys
from datetime import datetime, timezone
from typing import Any, Dict, Optional, TextIO


_CONFIGURED: Dict[str, bool] = {}
_LOGGER_LEVEL_OVERRIDES: Dict[str, int] = {}


def _utc_ts_iso(ts: float) -> str:
    try:
        return datetime.fromtimestamp(ts, tz=timezone.utc).isoformat().replace("+00:00", "Z")
    except Exception:
        return ""


class JsonlFormatter(logging.Formatter):
    """Minimal JSONL formatter for local-first debugging.

    Keep fields stable and small; extra fields can be added via `logger.*(..., extra={...})`.
    """

    def __init__(self, *, component: str):
        super().__init__()
        self._component = str(component or "").strip() or "cccc"

    def format(self, record: logging.LogRecord) -> str:
        payload: Dict[str, Any] = {
            "ts": _utc_ts_iso(getattr(record, "created", 0.0) or 0.0),
            "level": str(getattr(record, "levelname", "") or ""),
            "logger": str(getattr(record, "name", "") or ""),
            "component": self._component,
            "msg": record.getMessage(),
        }

        # Common correlation keys (optional).
        for k in ("trace_id", "op", "group_id", "scope_key", "actor_id", "event_id", "platform"):
            try:
                v = getattr(record, k, None)
            except Exception:
                v = None
            if v is None:
                continue
            sv = str(v).strip()
            if sv:
                payload[k] = sv

        if record.exc_info:
            try:
                payload["exc"] = self.formatException(record.exc_info)
            except Exception:
                payload["exc"] = "exception"

        try:
            return json.dumps(payload, ensure_ascii=False)
        except Exception:
            # Last resort: never crash logging.
            return '{"component":"%s","level":"%s","msg":"(log serialization failed)"}' % (
                self._component,
                payload.get("level", "INFO"),
            )


def _parse_level(level: str, default: int = logging.INFO) -> int:
    s = str(level or "").strip().upper()
    if not s:
        return default
    return int(getattr(logging, s, default))


def setup_root_json_logging(
    *,
    component: str,
    level: str = "INFO",
    stream: Optional[TextIO] = None,
    force: bool = False,
) -> None:
    """Configure root logging once per process.

    - Uses a single StreamHandler with JSONL formatter.
    - `force=True` clears existing handlers (useful for hot-switch).
    """
    key = f"root:{component}"
    if _CONFIGURED.get(key) and not force:
        return
    _CONFIGURED[key] = True

    root = logging.getLogger()
    root.setLevel(_parse_level(level))

    if force:
        for h in list(root.handlers):
            try:
                root.removeHandler(h)
            except Exception:
                pass

    handler = logging.StreamHandler(stream or sys.stderr)
    # Keep filtering on the logger side so per-logger DEBUG overrides still work
    # when the process default level stays at INFO.
    handler.setLevel(logging.NOTSET)
    handler.setFormatter(JsonlFormatter(component=component))

    # Avoid duplicate handlers on repeated calls (common in reload/dev).
    for h in list(root.handlers):
        if isinstance(h, logging.StreamHandler):
            # If it already looks like our JSONL handler, keep it.
            fmt = getattr(h, "formatter", None)
            if isinstance(fmt, JsonlFormatter):
                h.setLevel(logging.NOTSET)
                root.setLevel(_parse_level(level))
                return

    root.addHandler(handler)


def apply_logger_levels(logger_levels: Optional[Dict[str, str]]) -> None:
    """Apply explicit logger-level overrides and clear stale ones.

    Logger names omitted from the latest call are reset to NOTSET so they
    inherit from their nearest configured parent again.
    """
    desired: Dict[str, int] = {}
    if isinstance(logger_levels, dict):
        for name, level in logger_levels.items():
            logger_name = str(name or "").strip()
            if not logger_name:
                continue
            desired[logger_name] = _parse_level(str(level or "").strip().upper(), logging.NOTSET)

    stale = set(_LOGGER_LEVEL_OVERRIDES) - set(desired)
    for logger_name in stale:
        logging.getLogger(logger_name).setLevel(logging.NOTSET)
        _LOGGER_LEVEL_OVERRIDES.pop(logger_name, None)

    for logger_name, level in desired.items():
        logging.getLogger(logger_name).setLevel(level)
        _LOGGER_LEVEL_OVERRIDES[logger_name] = level
