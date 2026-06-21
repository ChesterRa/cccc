"""DingTalk message formatting and normalization helpers."""

from __future__ import annotations

import threading
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

DINGTALK_MAX_MESSAGE_LENGTH = 4096
DEFAULT_MAX_CHARS = 4096
DEFAULT_MAX_LINES = 64


class RateLimiter:
    """
    Per-chat rate limiter for DingTalk outbound API calls.

    DingTalk has both global and per-chat limits. The bridge currently applies
    the conservative per-chat limit at the adapter boundary.
    """

    def __init__(self, max_per_second: float = 5.0):
        self.min_interval = 1.0 / max_per_second
        self.last_send: Dict[str, float] = {}
        self.lock = threading.Lock()

    def acquire(self, chat_id: str) -> float:
        """Return required wait time in seconds, or 0 when send can proceed."""
        with self.lock:
            now = time.time()
            last = self.last_send.get(chat_id, 0)
            elapsed = now - last

            if elapsed >= self.min_interval:
                self.last_send[chat_id] = now
                return 0.0
            return self.min_interval - elapsed

    def wait_and_acquire(self, chat_id: str) -> None:
        """Sleep until the chat can send, then mark the send slot acquired."""
        wait_time = self.acquire(chat_id)
        if wait_time > 0:
            time.sleep(wait_time)
            self.acquire(chat_id)


def clean_at_user_ids(at_user_ids: Optional[List[str]] = None) -> List[str]:
    """Normalize optional DingTalk mention user IDs."""
    return [str(x).strip() for x in (at_user_ids or []) if str(x).strip()]


def build_markdown_payload(text: str, at_user_ids: Optional[List[str]] = None) -> Dict[str, Any]:
    """Build a DingTalk markdown payload with optional real-mention metadata."""
    payload: Dict[str, Any] = {
        "title": text[:20] if len(text) > 20 else text,
        "text": text,
    }
    cleaned = clean_at_user_ids(at_user_ids)
    if cleaned:
        payload["at"] = {"atUserIds": cleaned}
    return payload


def normalize_text(text: str) -> str:
    """Normalize outbound text without truncating content."""
    if not text:
        return ""

    normalized = text.replace("\r\n", "\n").replace("\r", "\n").replace("\t", "  ")
    lines = [ln.rstrip() for ln in normalized.split("\n")]

    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()

    kept: List[str] = []
    empty_count = 0
    for ln in lines:
        if not ln.strip():
            empty_count += 1
            if empty_count <= 1:
                kept.append("")
        else:
            empty_count = 0
            kept.append(ln)

    return "\n".join(kept).strip()


def split_message_chunks(
    text: str,
    *,
    max_chars: int = DEFAULT_MAX_CHARS,
    max_lines: int = DEFAULT_MAX_LINES,
) -> List[str]:
    """Split long outbound text into DingTalk-sized chunks."""
    normalized = normalize_text(text)
    if not normalized:
        return []

    max_chars = max(1, min(int(max_chars or DINGTALK_MAX_MESSAGE_LENGTH), DINGTALK_MAX_MESSAGE_LENGTH))
    max_lines = max(1, int(max_lines or DEFAULT_MAX_LINES))
    chunks: List[str] = []
    current_lines: List[str] = []
    current_chars = 0

    def flush() -> None:
        nonlocal current_lines, current_chars
        if not current_lines:
            return
        chunk = "\n".join(current_lines).strip()
        if chunk:
            chunks.append(chunk)
        current_lines = []
        current_chars = 0

    def push_line(line: str) -> None:
        nonlocal current_chars
        sep = 1 if current_lines else 0
        current_lines.append(line)
        current_chars += len(line) + sep

    for raw_line in normalized.split("\n"):
        remaining = raw_line
        while True:
            if not current_lines and len(remaining) > max_chars:
                chunks.append(remaining[:max_chars])
                remaining = remaining[max_chars:]
                if not remaining:
                    break
                continue

            needs_new_chunk = False
            if current_lines and len(current_lines) >= max_lines:
                needs_new_chunk = True
            else:
                sep = 1 if current_lines else 0
                if current_chars + len(remaining) + sep > max_chars:
                    needs_new_chunk = True

            if needs_new_chunk:
                flush()
                continue

            push_line(remaining)
            break

    flush()
    return chunks


def parse_event_time(raw: Any) -> float:
    """Parse DingTalk createAt into epoch seconds, accepting seconds or millis."""
    try:
        ts = float(raw or 0.0)
    except Exception:
        return 0.0
    if ts <= 0:
        return 0.0
    if ts > 1e11:
        ts = ts / 1000.0
    return ts


def extract_rich_text(rich_text: List[Dict[str, Any]]) -> tuple[str, List[Dict[str, Any]]]:
    """Extract plain text and image attachments from DingTalk richText content."""
    texts: List[str] = []
    attachments: List[Dict[str, Any]] = []
    try:
        for item in rich_text:
            if item.get("text"):
                texts.append(item["text"])
            elif item.get("type") == "picture":
                download_code = item.get("downloadCode") or item.get("pictureDownloadCode", "")
                if download_code:
                    attachments.append({
                        "provider": "dingtalk",
                        "kind": "image",
                        "download_code": download_code,
                        "file_name": "image.png",
                    })
    except Exception:
        pass
    return " ".join(texts), attachments


def is_image_file(filename: str) -> bool:
    """Check if a file should be sent as a DingTalk image message."""
    image_extensions = {".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"}
    return Path(filename).suffix.lower() in image_extensions
