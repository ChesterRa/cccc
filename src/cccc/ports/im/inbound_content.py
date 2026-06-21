"""Inbound IM content normalization and attachment storage."""

from __future__ import annotations

import mimetypes
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable, Dict, List, Tuple

from ...kernel.blobs import store_blob_bytes
from ...util.conv import coerce_bool


_GENERIC_ATTACHMENT_FILENAMES = frozenset({"", "file", "unknown", "attachment"})
_GENERIC_ATTACHMENT_BASENAME = "attachment"
_UNINFORMATIVE_MIME_TYPES = frozenset({"application/octet-stream", "binary/octet-stream"})


@dataclass(frozen=True)
class PreparedInboundContent:
    text: str
    attachments: List[Dict[str, Any]]


def _is_generic_attachment_filename(value: str) -> bool:
    raw = str(value or "").strip().lower()
    if not raw:
        return True
    stem = Path(raw).stem.strip().lower()
    return raw in _GENERIC_ATTACHMENT_FILENAMES or stem in _GENERIC_ATTACHMENT_FILENAMES


def _guess_extension_from_mime_type(mime_type: str) -> str:
    normalized = str(mime_type or "").strip().lower()
    if not normalized:
        return ""
    if normalized == "image/jpeg":
        return ".jpg"
    guessed = mimetypes.guess_extension(normalized) or ""
    return guessed if guessed.startswith(".") else ""


def _sniff_attachment_content_type(raw: bytes) -> Tuple[str, str]:
    head = raw[:64]
    if head.startswith(b"\x89PNG\r\n\x1a\n"):
        return ("image/png", ".png")
    if head.startswith(b"\xff\xd8\xff"):
        return ("image/jpeg", ".jpg")
    if head.startswith((b"GIF87a", b"GIF89a")):
        return ("image/gif", ".gif")
    if head.startswith(b"BM"):
        return ("image/bmp", ".bmp")
    if len(head) >= 12 and head[:4] == b"RIFF" and head[8:12] == b"WEBP":
        return ("image/webp", ".webp")
    if head.startswith(b"%PDF-"):
        return ("application/pdf", ".pdf")
    if head.lstrip().startswith(b"{\\rtf"):
        return ("application/rtf", ".rtf")
    if head.startswith((b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08")):
        sample = raw[:8192].lower()
        if b"word/" in sample:
            return (
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                ".docx",
            )
        if b"xl/" in sample:
            return (
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                ".xlsx",
            )
        if b"ppt/" in sample:
            return (
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                ".pptx",
            )
        return ("application/zip", ".zip")
    if head.startswith(b"7z\xbc\xaf\x27\x1c"):
        return ("application/x-7z-compressed", ".7z")
    if head.startswith(b"Rar!\x1a\x07"):
        return ("application/vnd.rar", ".rar")
    if head.startswith(b"\x1f\x8b"):
        return ("application/gzip", ".gz")
    if head.startswith(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"):
        sample = raw[:8192].lower()
        if b"worddocument" in sample:
            return ("application/msword", ".doc")
        if b"workbook" in sample:
            return ("application/vnd.ms-excel", ".xls")
        if b"powerpoint document" in sample:
            return ("application/vnd.ms-powerpoint", ".ppt")
        return ("application/vnd.ms-office", ".doc")

    try:
        text_head = raw[:512].decode("utf-8", errors="ignore").lstrip().lower()
    except Exception:
        text_head = ""
    if text_head.startswith("<svg") or text_head.startswith("<?xml") and "<svg" in text_head:
        return ("image/svg+xml", ".svg")
    if text_head:
        lines = [line.strip() for line in text_head.splitlines() if line.strip()]
        if lines:
            has_ini_header = any(line.startswith("[") and "]" in line for line in lines[:3])
            has_key_value = any("=" in line for line in lines[:8])
            if has_ini_header and has_key_value:
                return ("text/plain", ".ini")
            return ("text/plain", ".txt")
    return ("", "")


def normalize_inbound_attachment_metadata(
    *,
    raw: bytes,
    filename: str,
    mime_type: str,
    kind: str,
) -> Tuple[str, str, str]:
    raw_filename = str(filename or "").strip()
    normalized_filename = raw_filename or "file"
    normalized_mime_type = str(mime_type or "").strip().lower()
    normalized_kind = str(kind or "").strip().lower() or "file"

    sniffed_mime_type, sniffed_ext = _sniff_attachment_content_type(raw)

    effective_mime_type = normalized_mime_type
    if not effective_mime_type or effective_mime_type in _UNINFORMATIVE_MIME_TYPES:
        effective_mime_type = sniffed_mime_type or effective_mime_type
    effective_kind = normalized_kind
    if effective_mime_type.startswith("image/"):
        effective_kind = "image"
    elif sniffed_mime_type.startswith("image/"):
        effective_kind = "image"

    if _is_generic_attachment_filename(normalized_filename):
        inferred_ext = sniffed_ext or _guess_extension_from_mime_type(effective_mime_type)
        normalized_filename = f"{_GENERIC_ATTACHMENT_BASENAME}{inferred_ext}" if inferred_ext else _GENERIC_ATTACHMENT_BASENAME

    has_suffix = bool(Path(normalized_filename).suffix)
    if effective_kind == "image" and not has_suffix and sniffed_ext:
        normalized_filename = f"{normalized_filename}{sniffed_ext}"

    return (normalized_filename, effective_mime_type, effective_kind)


def normalize_inbound_attachment_message_text(msg_text: str, stored_attachments: List[Dict[str, Any]]) -> str:
    normalized_text = str(msg_text or "").strip()
    if len(stored_attachments) != 1:
        return normalized_text
    title = str(stored_attachments[0].get("title") or "").strip() or "file"
    if not normalized_text:
        return f"[file] {title}"
    if re.fullmatch(r"\[file(?::\s*(unknown|file))?\]", normalized_text, flags=re.IGNORECASE):
        return f"[file] {title}"
    return normalized_text


def prepare_inbound_content(
    *,
    group: Any,
    adapter: Any,
    text: str,
    attachments: List[Dict[str, Any]],
    send_warning: Callable[[str], None],
) -> PreparedInboundContent:
    im_cfg = group.doc.get("im") if isinstance(group.doc.get("im"), dict) else {}
    files_cfg = im_cfg.get("files") if isinstance(im_cfg.get("files"), dict) else {}
    files_enabled = coerce_bool(files_cfg.get("enabled"), default=True)
    platform = str(getattr(adapter, "platform", "") or "").strip().lower()
    default_max_mb = 20 if platform == "telegram" else 10 if platform == "discord" else 20
    try:
        max_mb = int(files_cfg.get("max_mb") or default_max_mb)
    except Exception:
        max_mb = default_max_mb
    max_bytes = max(0, max_mb) * 1024 * 1024

    stored_attachments: List[Dict[str, Any]] = []
    if files_enabled and attachments:
        for item in attachments:
            if not isinstance(item, dict):
                continue
            try:
                size = int(item.get("bytes") or 0)
            except Exception:
                size = 0
            if max_bytes and size and size > max_bytes:
                send_warning(f"⚠️ Ignored: file too large (> {max_mb}MB).")
                continue
            try:
                raw = adapter.download_attachment(item)
            except Exception as exc:
                send_warning(f"❌ Failed to download attachment: {exc}")
                continue
            if max_bytes and len(raw) > max_bytes:
                send_warning(f"⚠️ Ignored: file too large (> {max_mb}MB).")
                continue
            filename, mime_type, kind = normalize_inbound_attachment_metadata(
                raw=raw,
                filename=str(item.get("file_name") or item.get("filename") or "file"),
                mime_type=str(item.get("mime_type") or item.get("content_type") or ""),
                kind=str(item.get("kind") or "file"),
            )
            stored_attachments.append(
                store_blob_bytes(
                    group,
                    data=raw,
                    filename=filename,
                    mime_type=mime_type,
                    kind=kind,
                )
            )

    normalized_text = normalize_inbound_attachment_message_text(text, stored_attachments)
    if not normalized_text and stored_attachments:
        if len(stored_attachments) == 1:
            normalized_text = f"[file] {stored_attachments[0].get('title') or 'file'}"
        else:
            normalized_text = f"[files] {len(stored_attachments)} attachments"

    return PreparedInboundContent(text=normalized_text, attachments=stored_attachments)
