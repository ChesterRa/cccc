"""Streaming migration helpers for legacy headless event logs."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Iterator

_MAX_EVENT_LINE_BYTES = 1024 * 1024
_DEDUPE_KEY_TOKEN = b'"dedupe_key"'


def iter_dedupe_payloads(path: Path) -> Iterator[dict[str, Any]]:
    with path.open("rb") as handle:
        while True:
            raw_line = _read_legacy_line(handle)
            if raw_line is None:
                break
            if not raw_line:
                continue
            try:
                payload = json.loads(raw_line)
            except (json.JSONDecodeError, UnicodeDecodeError):
                continue
            if isinstance(payload, dict) and payload.get("dedupe_key"):
                yield payload


def _read_legacy_line(handle: Any) -> bytes | None:
    raw_line = handle.readline(_MAX_EVENT_LINE_BYTES + 1)
    if not raw_line:
        return None
    if len(raw_line) <= _MAX_EVENT_LINE_BYTES:
        return raw_line

    mentions_dedupe_key = _DEDUPE_KEY_TOKEN in raw_line
    tail = raw_line[-(len(_DEDUPE_KEY_TOKEN) - 1) :]
    while not raw_line.endswith(b"\n"):
        raw_line = handle.readline(_MAX_EVENT_LINE_BYTES + 1)
        if not raw_line:
            break
        if not mentions_dedupe_key:
            mentions_dedupe_key = (
                _DEDUPE_KEY_TOKEN in tail + raw_line[: len(_DEDUPE_KEY_TOKEN) - 1]
                or _DEDUPE_KEY_TOKEN in raw_line
            )
            tail = (tail + raw_line)[-(len(_DEDUPE_KEY_TOKEN) - 1) :]
    if mentions_dedupe_key:
        raise OSError("oversized deepseek dedupe migration event has dedupe identity")
    return b""
