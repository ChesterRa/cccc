from __future__ import annotations

import copy
import json
import os
import tempfile
from pathlib import Path
from typing import Any, Dict


class ConcurrentDocumentWriteError(RuntimeError):
    """Raised when two writers change the same shared-document field."""


_MISSING = object()


def _same_document_value(left: Any, right: Any) -> bool:
    if left is _MISSING or right is _MISSING:
        return left is right
    return bool(left == right)


def _merge_document_value(
    baseline: Any,
    desired: Any,
    current: Any,
    *,
    document: str,
    path: tuple[str, ...],
) -> Any:
    if _same_document_value(desired, baseline):
        return _MISSING if current is _MISSING else copy.deepcopy(current)
    if _same_document_value(current, baseline) or _same_document_value(current, desired):
        return _MISSING if desired is _MISSING else copy.deepcopy(desired)

    baseline_map = {} if baseline is _MISSING else baseline
    if isinstance(baseline_map, dict) and isinstance(desired, dict) and isinstance(current, dict):
        merged = copy.deepcopy(current)
        for raw_key in set(baseline_map) | set(desired) | set(current):
            key = str(raw_key)
            baseline_value = baseline_map.get(raw_key, _MISSING)
            desired_value = desired.get(raw_key, _MISSING)
            if _same_document_value(desired_value, baseline_value):
                continue
            current_value = current.get(raw_key, _MISSING)
            value = _merge_document_value(
                baseline_value,
                desired_value,
                current_value,
                document=document,
                path=(*path, key),
            )
            if value is _MISSING:
                merged.pop(raw_key, None)
            else:
                merged[raw_key] = value
        return merged

    field = ".".join(path) or "<root>"
    raise ConcurrentDocumentWriteError(f"concurrent write conflict in {document} at {field}")


def merge_concurrent_document_changes(
    baseline: Dict[str, Any],
    desired: Dict[str, Any],
    current: Dict[str, Any],
    *,
    document: str,
) -> Dict[str, Any]:
    """Three-way merge one writer's changes without losing concurrent updates.

    Dict changes on different paths merge. Lists and scalar values remain
    atomic: competing edits to the same path fail instead of silently choosing
    a winner.
    """

    merged = _merge_document_value(
        baseline,
        desired,
        current,
        document=document,
        path=(),
    )
    if not isinstance(merged, dict):
        raise ConcurrentDocumentWriteError(f"concurrent write conflict in {document} at <root>")
    return merged


def atomic_write_text(path: Path, text: str, *, encoding: str = "utf-8") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=path.name + ".", dir=str(path.parent))
    try:
        with os.fdopen(fd, "w", encoding=encoding) as f:
            f.write(text)
        os.replace(tmp, path)
    finally:
        try:
            if os.path.exists(tmp):
                os.unlink(tmp)
        except Exception:
            pass


def atomic_write_json(path: Path, obj: Dict[str, Any], *, indent: int = 2) -> None:
    atomic_write_text(path, json.dumps(obj, ensure_ascii=False, indent=indent) + "\n")

def atomic_write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(prefix=path.name + ".", dir=str(path.parent))
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(data)
        os.replace(tmp, path)
    finally:
        try:
            if os.path.exists(tmp):
                os.unlink(tmp)
        except Exception:
            pass


def read_json(path: Path) -> Dict[str, Any]:
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}
