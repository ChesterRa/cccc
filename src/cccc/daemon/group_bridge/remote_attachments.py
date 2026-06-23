"""Attachment payload helpers for Group Bridge remote messages."""

from __future__ import annotations

import base64
import binascii
import hashlib
from typing import Any, Dict, Iterable, List

from ...kernel.blobs import resolve_blob_attachment_path, store_blob_bytes
from ...kernel.group import Group

REMOTE_ATTACHMENT_MAX_BYTES = 20 * 1024 * 1024
REMOTE_ATTACHMENT_MAX_TOTAL_BYTES = 50 * 1024 * 1024
REMOTE_ATTACHMENT_MAX_COUNT = 10
_VALID_KINDS = {"text", "image", "file"}


def build_remote_attachment_payloads(group: Group, attachments: Iterable[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """Build wire-safe attachment payloads from local blob metadata."""

    source_items = [dict(item) for item in (attachments or []) if isinstance(item, dict)]
    _check_count(source_items)
    out: List[Dict[str, Any]] = []
    total = 0
    for item in source_items:
        rel_path = str(item.get("path") or "").strip()
        if not rel_path:
            raise ValueError("attachment missing path")
        abs_path = resolve_blob_attachment_path(group, rel_path=rel_path)
        if not abs_path.exists() or not abs_path.is_file():
            raise ValueError(f"attachment not found: {rel_path}")
        raw = abs_path.read_bytes()
        size = len(raw)
        if size > REMOTE_ATTACHMENT_MAX_BYTES:
            raise ValueError("attachment too large")
        total += size
        if total > REMOTE_ATTACHMENT_MAX_TOTAL_BYTES:
            raise ValueError("attachments are too large")
        digest = hashlib.sha256(raw).hexdigest()
        expected_digest = str(item.get("sha256") or "").strip()
        if expected_digest and expected_digest != digest:
            raise ValueError("attachment hash mismatch")
        out.append(
            {
                "kind": _normalize_kind(item.get("kind")),
                "title": _attachment_title(item),
                "mime_type": str(item.get("mime_type") or ""),
                "bytes": size,
                "sha256": digest,
                "content_base64": base64.b64encode(raw).decode("ascii"),
            }
        )
    return out


def store_remote_attachment_payloads(group: Group, payloads: Iterable[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """Store received remote attachment payloads into the target group's blob store."""

    source_items = [dict(item) for item in (payloads or []) if isinstance(item, dict)]
    _check_count(source_items)
    out: List[Dict[str, Any]] = []
    total = 0
    for item in source_items:
        encoded = str(item.get("content_base64") or "").strip()
        if not encoded:
            raise ValueError("attachment missing content")
        try:
            raw = base64.b64decode(encoded.encode("ascii"), validate=True)
        except (binascii.Error, UnicodeEncodeError) as exc:
            raise ValueError("attachment content is not valid base64") from exc
        size = len(raw)
        if size > REMOTE_ATTACHMENT_MAX_BYTES:
            raise ValueError("attachment too large")
        total += size
        if total > REMOTE_ATTACHMENT_MAX_TOTAL_BYTES:
            raise ValueError("attachments are too large")
        expected_size = _safe_int(item.get("bytes"), default=size)
        if expected_size != size:
            raise ValueError("attachment size mismatch")
        digest = hashlib.sha256(raw).hexdigest()
        expected_digest = str(item.get("sha256") or "").strip()
        if expected_digest and expected_digest != digest:
            raise ValueError("attachment hash mismatch")
        out.append(
            store_blob_bytes(
                group,
                data=raw,
                filename=_attachment_title(item),
                mime_type=str(item.get("mime_type") or ""),
                kind=_normalize_kind(item.get("kind")),
            )
        )
    return out


def _check_count(items: List[Dict[str, Any]]) -> None:
    if len(items) > REMOTE_ATTACHMENT_MAX_COUNT:
        raise ValueError("too many attachments")


def _normalize_kind(value: Any) -> str:
    kind = str(value or "file").strip().lower() or "file"
    return kind if kind in _VALID_KINDS else "file"


def _attachment_title(item: Dict[str, Any]) -> str:
    title = str(item.get("title") or "").strip()
    if title:
        return title
    path = str(item.get("path") or "").replace("\\", "/").strip()
    if path:
        return path.split("/")[-1] or "file"
    return "file"


def _safe_int(value: Any, *, default: int) -> int:
    try:
        return int(value)
    except Exception:
        return default
