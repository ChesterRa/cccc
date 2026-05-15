"""
CCCC IM Bridge - Core logic.

Handles:
- Inbound: IM messages → daemon API → ledger
- Outbound: ledger events → filter → IM
- Command processing
- Ledger watching (cursor-based tail)
"""

from __future__ import annotations

import hashlib
import json
import mimetypes
import os
import re
import shlex
import signal
import sys
import time
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Tuple

from ...daemon.server import call_daemon
from ...kernel.actors import list_actors, resolve_recipient_tokens
from ...kernel.blobs import resolve_blob_attachment_path, store_blob_bytes
from ...kernel.group import Group, load_group
from ...kernel.messaging import disabled_recipient_actor_ids, get_default_send_to
from ...paths import ensure_home
from ...util.conv import coerce_bool
from .adapters.base import IMAdapter, OutboundStreamHandle
from .adapters.telegram import TelegramAdapter
from .adapters.slack import SlackAdapter
from .adapters.discord import DiscordAdapter
from .commands import (
    CommandType,
    ParsedCommand,
    format_context,
    format_help,
    format_status,
    parse_message,
)
from .config_schema import canonicalize_im_config
from .auth import KeyManager
from .subscribers import SubscriberManager

from ...util.file_lock import LockUnavailableError, acquire_lockfile


def _now() -> str:
    return time.strftime("%Y-%m-%d %H:%M:%S")


def _is_env_var_name(value: str) -> bool:
    return bool(re.fullmatch(r"[A-Z_][A-Z0-9_]*", (value or "").strip()))


_PRESERVED_RECIPIENT_TOKENS = frozenset({"user", "@user", "@all", "@peers", "@foreman"})
_GENERIC_ATTACHMENT_FILENAMES = frozenset({"", "file", "unknown", "attachment"})
_GENERIC_ATTACHMENT_BASENAME = "attachment"
_UNINFORMATIVE_MIME_TYPES = frozenset({"application/octet-stream", "binary/octet-stream"})


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


def _normalize_inbound_attachment_metadata(
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


def _normalize_inbound_attachment_message_text(msg_text: str, stored_attachments: List[Dict[str, Any]]) -> str:
    normalized_text = str(msg_text or "").strip()
    if len(stored_attachments) != 1:
        return normalized_text
    title = str(stored_attachments[0].get("title") or "").strip() or "file"
    if not normalized_text:
        return f"[file] {title}"
    if re.fullmatch(r"\[file(?::\s*(unknown|file))?\]", normalized_text, flags=re.IGNORECASE):
        return f"[file] {title}"
    return normalized_text


def _acquire_singleton_lock(lock_path: Path) -> Optional[Any]:
    """
    Acquire singleton lock to prevent multiple bridge instances.
    Returns file handle on success, None on failure.
    """
    try:
        f = acquire_lockfile(lock_path, blocking=False)
    except LockUnavailableError:
        return None
    except Exception:
        return None

    try:
        f.seek(0)
        f.write((str(os.getpid()) + "\n").encode("utf-8", errors="replace"))
        f.flush()
    except Exception:
        # Best-effort: the lock is the important part.
        pass
    return f


class LedgerWatcher:
    """
    Watch ledger.jsonl for new events using cursor-based tail.

    Reuses exactly-once semantics from v0.3.28 outbox_consumer.py:
    - Cursor file tracks (dev, inode, offset)
    - Handles file rotation/truncation
    - Resumes from last position on restart
    """

    def __init__(
        self,
        group: Group,
        cursor_name: str = "im_bridge",
        log_fn: Optional[Callable[[str], None]] = None,
    ):
        self.group = group
        self.ledger_path = group.ledger_path
        self.cursor_path = group.path / "state" / f"{cursor_name}_cursor.json"
        self._log_fn = log_fn

        self._offset = 0
        self._dev: Optional[int] = None
        self._ino: Optional[int] = None
        self._buf = ""

        self._load_cursor()

    def _log(self, msg: str) -> None:
        """Log message if log function is configured."""
        if self._log_fn:
            self._log_fn(msg)

    def _load_cursor(self) -> None:
        """Load cursor from disk."""
        try:
            if self.cursor_path.exists():
                data = json.loads(self.cursor_path.read_text(encoding="utf-8"))
                self._dev = data.get("dev")
                self._ino = data.get("ino")
                self._offset = int(data.get("offset", 0))
        except Exception:
            self._dev = None
            self._ino = None
            self._offset = 0

    def _save_cursor(self, dev: int, ino: int, offset: int) -> bool:
        """Save cursor to disk. Returns True on success."""
        try:
            self.cursor_path.parent.mkdir(parents=True, exist_ok=True)
            tmp = self.cursor_path.with_suffix(".tmp")
            tmp.write_text(
                json.dumps({"dev": dev, "ino": ino, "offset": offset}, indent=2),
                encoding="utf-8",
            )
            tmp.replace(self.cursor_path)
            return True
        except Exception as e:
            self._log(f"[watcher] Failed to save cursor: {e}")
            return False

    def seek_to_end(self) -> bool:
        """
        Move cursor to the end of ledger file.

        Use this on startup to skip pending messages (skip-on-restart behavior).
        Returns True on success.
        """
        if not self.ledger_path.exists():
            return True

        try:
            st = os.stat(self.ledger_path)
            dev, ino, size = st.st_dev, st.st_ino, st.st_size

            self._dev = dev
            self._ino = ino
            self._offset = size
            self._buf = ""

            if self._save_cursor(dev, ino, size):
                self._log(f"[watcher] Cursor moved to end (offset={size})")
                return True
            return False
        except Exception as e:
            self._log(f"[watcher] Failed to seek to end: {e}")
            return False

    def poll(self) -> List[Dict[str, Any]]:
        """
        Poll for new events.

        Returns list of parsed event dicts.
        """
        events = []

        if not self.ledger_path.exists():
            return events

        try:
            st = os.stat(self.ledger_path)
            dev, ino, size = st.st_dev, st.st_ino, st.st_size

            # Check for rotation/truncation
            rotated = (
                self._dev is None
                or self._dev != dev
                or self._ino != ino
                or self._offset > size
            )

            if rotated:
                # Start from current position (don't replay history on first run)
                if self._dev is None:
                    # First time: check if file is fresh
                    is_fresh = (time.time() - st.st_mtime) <= 5.0
                    self._offset = 0 if is_fresh else size
                else:
                    # Rotation: start from end
                    self._offset = size

                self._dev = dev
                self._ino = ino
                self._save_cursor(dev, ino, self._offset)

            # Read new data
            if size > self._offset:
                with open(self.ledger_path, "r", encoding="utf-8", errors="replace") as f:
                    f.seek(self._offset)
                    chunk = f.read()

                if chunk:
                    data = self._buf + chunk
                    lines = data.splitlines(keepends=True)

                    # Keep incomplete line in buffer
                    if lines and not lines[-1].endswith("\n"):
                        self._buf = lines.pop()
                    else:
                        self._buf = ""

                    # Parse complete lines
                    for line in lines:
                        text = line.rstrip("\r\n")
                        if not text:
                            continue

                        self._offset += len(line)

                        try:
                            event = json.loads(text)
                            events.append(event)
                        except json.JSONDecodeError:
                            continue

                    # Save cursor
                    self._save_cursor(dev, ino, self._offset)

        except Exception:
            pass

        return events


class IMBridge:
    """
    Main IM Bridge class.

    Coordinates:
    - Adapter (platform-specific communication)
    - Subscriber manager
    - Ledger watcher
    - Command processing
    """

    def __init__(
        self,
        group: Group,
        adapter: IMAdapter,
        log_path: Optional[Path] = None,
        skip_pending_on_start: bool = False,
    ):
        self.group = group
        self.adapter = adapter
        self.log_path = log_path
        self.skip_pending_on_start = skip_pending_on_start

        self.subscribers = SubscriberManager(group.path / "state")
        self.key_manager = KeyManager(group.path / "state")
        self.watcher = LedgerWatcher(group, log_fn=self._log)

        self._running = False
        self._last_outbound_check = 0.0
        # Inbound history filtering baseline. Messages older than this moment
        # (with a small grace window) are treated as pre-start backlog.
        self._connected_at = 0.0
        self._history_grace_seconds = 2.0
        # Best-effort inbound dedupe to prevent double-processing when platforms
        # retry delivery or emit multiple events for the same message (e.g., edits).
        self._seen_inbound: Dict[str, float] = {}

        # Typing indicator state: chat_id -> (message_id, reaction_id)
        # Used to show a "processing" emoji on the user's message while agents work.
        self._typing_indicators: Dict[str, Tuple[str, str]] = {}
        # Telegram sendChatAction throttle: chat_id -> last_sent_timestamp
        self._typing_action_ts: Dict[str, float] = {}
        # Active outbound streams: stream_id -> {target_key -> OutboundStreamHandle}
        # Two-level cache so each subscriber chat gets its own handle.
        self._active_streams: Dict[str, Dict[str, OutboundStreamHandle]] = {}
        # Targets that successfully completed a stream and may skip the final
        # plain-text fallback for the same stream_id.
        self._completed_stream_targets: Dict[str, set[str]] = {}
        # Inbound health monitoring: periodic log every 5 minutes
        self._last_health_log: float = 0.0
        self._inbound_count: int = 0  # messages processed since last health log
        # Per chat/thread mention targets for outbound replies.
        # The bridge computes this from inbound metadata and passes it explicitly
        # to adapters instead of letting adapters guess from their own caches.
        self._mention_targets: Dict[str, List[str]] = {}

    def _should_process_inbound(self, *, chat_id: str, thread_id: int, message_id: str) -> bool:
        """
        Return True if this inbound message should be processed.

        Uses a small in-memory cache keyed by (chat_id, thread_id, message_id).
        """
        mid = str(message_id or "").strip()
        if not mid:
            return True

        now = time.time()
        key = f"{chat_id}:{int(thread_id or 0)}:{mid}"

        if key in self._seen_inbound:
            self._log(f"[inbound] Dedup skip: {key}")
            return False

        self._seen_inbound[key] = now

        # Opportunistic pruning (keep memory bounded without extra deps).
        if len(self._seen_inbound) > 2048:
            cutoff = now - 3600.0  # 1h
            self._seen_inbound = {k: ts for k, ts in self._seen_inbound.items() if ts >= cutoff}
            if len(self._seen_inbound) > 4096:
                self._seen_inbound.clear()

        return True

    def _log(self, msg: str) -> None:
        """Log message."""
        if self.log_path:
            try:
                self.log_path.parent.mkdir(parents=True, exist_ok=True)
                with self.log_path.open("a", encoding="utf-8") as f:
                    f.write(f"{_now()} {msg}\n")
            except Exception:
                pass

    def _daemon(self, req: Dict[str, Any]) -> Dict[str, Any]:
        """Call daemon API."""
        try:
            return call_daemon(req)
        except Exception as e:
            return {"ok": False, "error": {"code": "daemon_error", "message": str(e)}}

    def start(self) -> bool:
        """Start the bridge."""
        if not self.adapter.connect():
            self._log("[start] Failed to connect adapter")
            return False
        self._connected_at = time.time()

        # Skip pending messages if configured (move cursor to end of ledger)
        if self.skip_pending_on_start:
            if self.watcher.seek_to_end():
                self._log("[start] Skipped pending messages (cursor moved to end)")
            else:
                self._log("[start] Warning: failed to skip pending messages")

        self._running = True
        self._log(f"[start] Bridge started for group {self.group.group_id}")
        return True

    def stop(self) -> None:
        """Stop the bridge."""
        self._running = False
        self.adapter.disconnect()
        self._log("[stop] Bridge stopped")

    def run_once(self) -> None:
        """Run one iteration of the bridge loop."""
        # Process inbound messages
        self._process_inbound()

        # Refresh Telegram "typing" action for active indicators
        self._refresh_typing_actions()

        # Process outbound events (throttled)
        now = time.time()
        if now - self._last_outbound_check >= 1.0:
            self._process_outbound()
            self._last_outbound_check = now

        # Periodic inbound health log (every 5 minutes)
        if now - self._last_health_log >= 300.0:
            last_enqueue = getattr(self.adapter, "_last_enqueue_ts", 0.0)
            gap = f"{now - last_enqueue:.0f}s ago" if last_enqueue > 0 else "never"
            self._log(
                f"[health] inbound={self._inbound_count} since last check, "
                f"last_enqueue={gap}, seen_cache={len(self._seen_inbound)}"
            )
            self._inbound_count = 0
            self._last_health_log = now

    def run_forever(self, poll_interval: float = 0.5) -> None:
        """Run the bridge loop forever."""
        while self._running:
            try:
                self.run_once()
            except Exception as e:
                self._log(f"[error] Loop error: {e}")

            time.sleep(poll_interval)

    def _process_inbound(self) -> None:
        """Process incoming IM messages."""
        # Reload authorized-chat state from disk so that binds performed by the
        # daemon (a separate process) are picked up without restarting the bridge.
        self.key_manager._load()

        messages = self.adapter.poll()
        if messages:
            self._log(f"[inbound] Polled {len(messages)} messages")

        for msg in messages:
            chat_id = str(msg.get("chat_id") or "").strip()
            text = str(msg.get("text") or "")
            chat_title = str(msg.get("chat_title") or "")
            from_user = str(msg.get("from_user") or "user")
            from_user_id = str(msg.get("from_user_id") or "")
            chat_type = str(msg.get("chat_type") or "").strip().lower()
            try:
                thread_id = int(msg.get("thread_id") or 0)
            except Exception:
                thread_id = 0
            message_id = str(msg.get("message_id") or "").strip()

            if not text:
                self._log(f"[inbound] Empty text, raw msg keys: {list(msg.keys())}, text field: {repr(msg.get('text'))}")
                continue
            if self._is_historical_inbound(msg):
                self._log(
                    f"[inbound] Dropped historical message chat={chat_id} thread={thread_id} message_id={message_id}"
                )
                continue
            if not self._should_process_inbound(chat_id=chat_id, thread_id=thread_id, message_id=message_id):
                continue
            self._inbound_count += 1
            self._remember_mention_targets(chat_id, thread_id, msg)

            # Authorization check: unauthorized chats may only /subscribe.
            self._log(f"[inbound] Checking auth for chat_id={chat_id} thread={thread_id}")
            if not self.key_manager.is_authorized(chat_id, thread_id):
                parsed_pre = parse_message(text)
                if parsed_pre.type == CommandType.SUBSCRIBE:
                    self._handle_subscribe(chat_id, chat_title, thread_id=thread_id)
                else:
                    self._log(f"[auth] Dropped message from unauthorized chat={chat_id} thread={thread_id}")
                continue

            # Parse command
            parsed = parse_message(text)

            # Handle commands
            if parsed.type == CommandType.SUBSCRIBE:
                self._handle_subscribe(chat_id, chat_title, thread_id=thread_id)
            elif parsed.type == CommandType.UNSUBSCRIBE:
                self._handle_unsubscribe(chat_id, thread_id=thread_id)
            elif parsed.type == CommandType.VERBOSE:
                self._handle_verbose(chat_id, thread_id=thread_id)
            elif parsed.type == CommandType.STATUS:
                self._handle_status(chat_id, thread_id=thread_id)
            elif parsed.type == CommandType.CONTEXT:
                self._handle_context(chat_id, thread_id=thread_id)
            elif parsed.type == CommandType.PAUSE:
                self._handle_pause(chat_id, thread_id=thread_id)
            elif parsed.type == CommandType.RESUME:
                self._handle_resume(chat_id, thread_id=thread_id)
            elif parsed.type == CommandType.LAUNCH:
                self._handle_launch(chat_id, thread_id=thread_id)
            elif parsed.type == CommandType.QUIT:
                self._handle_quit(chat_id, thread_id=thread_id)
            elif parsed.type == CommandType.HELP:
                self._handle_help(chat_id, thread_id=thread_id)
            elif parsed.type == CommandType.SEND:
                attachments = msg.get("attachments") if isinstance(msg.get("attachments"), list) else []
                mention_user_ids = msg.get("mention_user_ids") if isinstance(msg.get("mention_user_ids"), list) else []
                self._handle_message(
                    chat_id,
                    parsed,
                    from_user,
                    attachments=attachments,
                    mention_user_ids=mention_user_ids,
                    thread_id=thread_id,
                    message_id=message_id,
                    from_user_id=from_user_id,
                )
            elif parsed.type == CommandType.MESSAGE:
                routed = coerce_bool(msg.get("routed"), default=False)

                # Unknown slash commands should not be forwarded as chat content.
                # (Telegram groups may contain unrelated /commands; we only reply when routed/private.)
                if text.lstrip().startswith("/"):
                    if routed or chat_type in ("private",):
                        self.adapter.send_message(chat_id, "❓ Unknown command. Use /help.", thread_id=thread_id)
                    continue

                # When routed (@bot or DM), treat plain text as implicit /send.
                if routed or chat_type in ("private",):
                    attachments = msg.get("attachments") if isinstance(msg.get("attachments"), list) else []
                    mention_user_ids = msg.get("mention_user_ids") if isinstance(msg.get("mention_user_ids"), list) else []
                    # Build a SEND-like ParsedCommand so _handle_message can extract @targets from args.
                    implicit_args = parsed.text.split() if parsed.text else []
                    implicit_send = ParsedCommand(
                        type=CommandType.SEND,
                        text=parsed.text,
                        mentions=parsed.mentions,
                        args=implicit_args,
                    )
                    self._handle_message(
                        chat_id,
                        implicit_send,
                        from_user,
                        attachments=attachments,
                        mention_user_ids=mention_user_ids,
                        thread_id=thread_id,
                        message_id=message_id,
                        from_user_id=from_user_id,
                    )
                    continue

                # Non-routed messages are ignored.
                continue

    def _parse_message_timestamp(self, raw: Any) -> Optional[float]:
        """
        Parse message timestamp into epoch seconds.

        Accepts either seconds or milliseconds as int/float/string.
        """
        if raw is None:
            return None
        try:
            ts = float(raw)
        except Exception:
            return None
        if ts <= 0:
            return None
        # Heuristic: values in milliseconds are much larger than epoch seconds.
        if ts > 1e11:
            ts = ts / 1000.0
        return ts

    def _is_historical_inbound(self, msg: Dict[str, Any]) -> bool:
        """
        Return True if an inbound message is older than bridge start time.

        Messages without parseable timestamps are treated as fresh to avoid
        false drops on adapters that do not provide event time.
        """
        connected_at = float(self._connected_at or 0.0)
        if connected_at <= 0:
            return False

        ts = self._parse_message_timestamp(msg.get("timestamp"))
        if ts is None:
            return False

        cutoff = connected_at - float(self._history_grace_seconds or 0.0)
        return ts < cutoff

    def _process_outbound(self) -> None:
        """Process outbound events from ledger."""
        # Reload subscriber state from disk so that subscriptions created by the
        # daemon (e.g. auto-subscribe on bind) are picked up without restarting.
        self.subscribers._load()
        # Reload authorized-chat state as revoke/bind is performed by daemon.
        self.key_manager._load()

        events = self.watcher.poll()
        actor_labels = self._actor_display_map()

        for event in events:
            self._forward_event(event, actor_labels=actor_labels)

    def _actor_display_map(self) -> Dict[str, str]:
        """Build actor_id -> display label map (title first, id fallback)."""
        group = load_group(self.group.group_id) or self.group
        labels: Dict[str, str] = {}
        for actor in list_actors(group):
            if not isinstance(actor, dict):
                continue
            actor_id = str(actor.get("id") or "").strip()
            if not actor_id:
                continue
            title = str(actor.get("title") or "").strip()
            labels[actor_id] = title if title else actor_id
        return labels

    def _display_actor_token(self, token: str, actor_labels: Dict[str, str]) -> str:
        """Render actor/selector token for outbound IM display."""
        raw = str(token or "").strip()
        if not raw:
            return raw
        if raw in _PRESERVED_RECIPIENT_TOKENS:
            return raw
        if raw in actor_labels:
            return actor_labels[raw]
        if raw.startswith("@"):
            stripped = raw[1:].strip()
            if stripped in actor_labels:
                return actor_labels[stripped]
        return raw

    @staticmethod
    def _stream_target_key(chat_id: str, thread_id: int) -> str:
        return f"{chat_id}:{int(thread_id or 0)}"

    def _remember_mention_targets(self, chat_id: str, thread_id: int, msg: Dict[str, Any]) -> None:
        """Cache the latest explicit mention target for this chat/thread."""
        target_key = self._stream_target_key(chat_id, thread_id)
        raw_ids = msg.get("mention_user_ids")
        mention_user_ids = (
            [str(x).strip() for x in raw_ids if str(x).strip()]
            if isinstance(raw_ids, list)
            else []
        )
        if mention_user_ids:
            self._mention_targets[target_key] = mention_user_ids
        else:
            self._mention_targets.pop(target_key, None)

        # Keep the cache bounded without introducing another dependency.
        while len(self._mention_targets) > 256:
            oldest_key = next(iter(self._mention_targets))
            self._mention_targets.pop(oldest_key, None)

    def _resolve_outbound_mention_targets(
        self,
        *,
        event: Dict[str, Any],
        sub: Any,
        is_user_facing: bool,
    ) -> Optional[List[str]]:
        """Resolve explicit mention targets for an outbound IM message."""
        if not is_user_facing:
            return None

        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower()
        if platform != "dingtalk":
            return None

        data = event.get("data", {})
        if isinstance(data, dict):
            raw_ids = data.get("mention_user_ids")
            if isinstance(raw_ids, list):
                cleaned = [str(x).strip() for x in raw_ids if str(x).strip()]
                return cleaned

        target_key = self._stream_target_key(sub.chat_id, sub.thread_id)
        cached = self._mention_targets.get(target_key)
        if cached is None:
            return None
        return list(cached)

    def _forward_stream_event(self, event: Dict[str, Any]) -> None:
        """Forward a chat.stream event to subscribed chats via adapter streaming methods.

        Two-level cache: _active_streams[stream_id][target_key] = handle,
        so each subscriber chat gets its own platform handle (E1 fix).

        Graceful degradation:
        - begin_stream failure (None or exception) → no handle cached for that target;
          subsequent update/end are silently ignored for that target and the final
          chat.message will deliver as plain text (E4 fix).
        - update_stream exception → logged, frame dropped, stream continues.
        - only targets whose end_stream succeeds may suppress the final plain-text
          fallback; failures degrade back to chat.message delivery.
        """
        data = event.get("data", {})
        if not isinstance(data, dict):
            return
        op = str(data.get("op") or "").strip()
        stream_id = str(data.get("stream_id") or "").strip()
        text = str(data.get("text") or "")
        seq = int(data.get("seq") or 0)
        to = data.get("to", [])
        if not stream_id or not op:
            return

        # E3 fix: filter by to field — reuse _should_forward-compatible logic.
        is_user_facing = not to or "user" in to

        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower()
        subscribed = self.subscribers.get_subscribed_targets(platform=platform)

        for sub in subscribed:
            if not self.key_manager.is_authorized(sub.chat_id, sub.thread_id):
                continue

            # E3: non-verbose subscribers only get user-facing streams.
            if not bool(sub.verbose) and not is_user_facing:
                continue

            target_key = self._stream_target_key(sub.chat_id, sub.thread_id)

            if op == "start":
                try:
                    handle = self.adapter.begin_stream(sub.chat_id, stream_id, text=text, thread_id=sub.thread_id)
                except Exception:
                    self._log(f"[stream] begin_stream exception for stream={stream_id} target={target_key}, degrading to plain text")
                    handle = None
                if handle is not None:
                    targets = self._active_streams.setdefault(stream_id, {})
                    targets[target_key] = handle
            elif op == "update":
                targets = self._active_streams.get(stream_id)
                if targets is not None:
                    handle = targets.get(target_key)
                    if handle is not None:
                        try:
                            self.adapter.update_stream(handle, text=text, seq=seq)
                        except Exception:
                            self._log(f"[stream] update_stream exception for stream={stream_id} target={target_key} seq={seq}, frame dropped")
            elif op == "end":
                targets = self._active_streams.get(stream_id)
                if targets is not None:
                    handle = targets.get(target_key)
                    if handle is not None:
                        end_ok = False
                        try:
                            end_ok = bool(self.adapter.end_stream(handle, text=text))
                        except Exception:
                            self._log(f"[stream] end_stream exception for stream={stream_id} target={target_key}")
                        if end_ok and text:
                            completed = self._completed_stream_targets.setdefault(stream_id, set())
                            completed.add(target_key)
                        targets.pop(target_key, None)
                        if not targets:
                            self._active_streams.pop(stream_id, None)

    def _forward_event(self, event: Dict[str, Any], *, actor_labels: Optional[Dict[str, str]] = None) -> None:
        """Forward a ledger event to subscribed chats."""
        kind = event.get("kind", "")
        by = event.get("by", "")

        # Skip user messages (they come from IM, Web, or CLI - avoid echo)
        # We only forward agent messages and system notifications
        if by == "user":
            return

        # Route streaming events to dedicated handler
        if kind == "chat.stream":
            self._forward_stream_event(event)
            return

        # Determine if we should forward
        is_system = kind == "system.notify"
        is_chat = kind == "chat.message"

        if not is_system and not is_chat:
            return

        # Get message details
        data = event.get("data", {})
        text = data.get("text", "")
        to = data.get("to", [])
        attachments = data.get("attachments", [])

        # E4: per-target dedup for streamed messages.
        # If chat.message carries stream_id, only targets that successfully
        # completed end_stream skip the final plain-text fallback. Attachments are
        # still delivered because the stream never carried them.
        _streamed_msg_id = str(data.get("stream_id") or "").strip() if is_chat else ""
        _streamed_targets: set[str] = set()
        if _streamed_msg_id:
            self._active_streams.pop(_streamed_msg_id, None)
            completed = self._completed_stream_targets.pop(_streamed_msg_id, None)
            if completed:
                _streamed_targets = set(completed)

        if not text and not attachments:
            return

        # Forward to subscribed chats (filtered by platform to avoid cross-platform sends)
        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower()
        subscribed = self.subscribers.get_subscribed_targets(platform=platform)

        # Determine if this event is user-facing (to:user or broadcast).
        # Agent-to-agent messages should NOT cancel typing indicators.
        is_user_facing = not to or "user" in to
        display_labels = actor_labels or self._actor_display_map()

        for sub in subscribed:
            # Safety filter: only authorized chats are allowed to receive bridge
            # traffic even if stale subscription state exists on disk.
            if not self.key_manager.is_authorized(sub.chat_id, sub.thread_id):
                continue

            target_key = self._stream_target_key(sub.chat_id, sub.thread_id)
            skip_text_due_to_stream = bool(_streamed_targets and target_key in _streamed_targets)

            verbose = bool(sub.verbose)

            # Filter based on verbose setting
            if not self._should_forward(event, verbose):
                continue

            # Format message (may be empty for file-only events)
            display_by = self._display_actor_token(by, display_labels)
            display_to = [
                self._display_actor_token(str(t), display_labels)
                for t in (to if isinstance(to, list) else [])
                if str(t or "").strip()
            ]
            formatted = self.adapter.format_outbound(display_by, display_to, text, is_system) if text else ""
            mention_user_ids = self._resolve_outbound_mention_targets(
                event=event,
                sub=sub,
                is_user_facing=is_user_facing,
            )

            # Try file delivery first (if any attachments)
            sent_any_file = False
            delivered_user_facing = False
            file_cfg = (self.group.doc.get("im") or {}) if isinstance(self.group.doc.get("im"), dict) else {}
            files_cfg = file_cfg.get("files") if isinstance(file_cfg.get("files"), dict) else {}
            files_enabled = coerce_bool(files_cfg.get("enabled"), default=True)
            platform = str(getattr(self.adapter, "platform", "") or "").strip().lower()
            default_max_mb = 20 if platform in ("telegram", "slack") else 10
            try:
                max_mb = int(files_cfg.get("max_mb") or default_max_mb)
            except Exception:
                max_mb = default_max_mb
            max_bytes = max(0, max_mb) * 1024 * 1024

            if files_enabled and isinstance(attachments, list):
                for i, a in enumerate(attachments):
                    if not isinstance(a, dict):
                        continue
                    rel_path = str(a.get("path") or "").strip()
                    if not rel_path:
                        continue
                    # Only handle blobs (state/blobs/*).
                    try:
                        abs_path = resolve_blob_attachment_path(self.group, rel_path=rel_path)
                    except Exception:
                        continue
                    try:
                        size = int(abs_path.stat().st_size)
                    except Exception:
                        size = 0
                    if max_bytes and size > max_bytes:
                        # Skip oversized files (no fallback).
                        continue
                    title = str(a.get("title") or abs_path.name or "file")
                    cap = formatted if (i == 0 and formatted and not skip_text_due_to_stream) else ""
                    ok = False
                    try:
                        ok = bool(
                            self.adapter.send_file(
                                sub.chat_id,
                                file_path=abs_path,
                                filename=title,
                                caption=cap,
                                thread_id=sub.thread_id,
                                mention_user_ids=mention_user_ids,
                            )
                        )
                    except Exception:
                        ok = False
                    if ok:
                        sent_any_file = True
                        if is_user_facing:
                            delivered_user_facing = True

            # If we didn't send any files, or if there's text with no files, send message.
            if formatted and not sent_any_file and not skip_text_due_to_stream:
                if mention_user_ids is None:
                    sent_msg = bool(self.adapter.send_message(sub.chat_id, formatted, thread_id=sub.thread_id))
                else:
                    sent_msg = bool(
                        self.adapter.send_message(
                            sub.chat_id,
                            formatted,
                            thread_id=sub.thread_id,
                            mention_user_ids=mention_user_ids,
                        )
                    )
                if sent_msg and is_user_facing:
                    delivered_user_facing = True

            # Remove typing indicator only after outbound delivery for this event
            # is actually completed for this chat.
            if delivered_user_facing:
                self._remove_typing_indicator(sub.chat_id)

    def _should_forward(self, event: Dict[str, Any], verbose: bool) -> bool:
        """Determine if event should be forwarded based on verbose setting."""
        kind = event.get("kind", "")

        # System notifications always forwarded
        if kind == "system.notify":
            return True

        # Chat messages
        if kind == "chat.message":
            data = event.get("data", {})
            to = data.get("to", [])
            by = event.get("by", "")

            # Verbose: forward all
            if verbose:
                return True

            # Non-verbose: only forward to:user messages
            if "user" in to or not to:
                return True

            return False

        return False

    # =========================================================================
    # Command Handlers
    # =========================================================================

    def _handle_subscribe(self, chat_id: str, chat_title: str, thread_id: int = 0) -> None:
        """Handle /subscribe command."""
        # Reload auth state on-demand as subscribe semantics depend on current
        # authorization truth (bind/revoke can happen in daemon/web concurrently).
        self.key_manager._load()
        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower()

        # If the chat is not yet authorized, generate a binding key.
        if not self.key_manager.is_authorized(chat_id, thread_id):
            key = self.key_manager.generate_key(chat_id, thread_id, platform)
            self.adapter.send_message(
                chat_id,
                f"🔑 Authorization required.\n\n"
                f"Open CCCC Web → Settings → IM Bridge, then approve this request in Pending Requests "
                f"(or paste this key in Bind):\n"
                f"`{key}`\n\n"
                f"If foreman is online, send this message to foreman:\n"
                f"`Please help bind my IM key: {key}`\n\n"
                f"Or run in terminal:\n"
                f"`cccc im bind --key {key}`\n\n"
                f"Key expires in 10 minutes.",
                thread_id=thread_id,
            )
            self._log(f"[subscribe] Pending auth key generated for chat={chat_id} thread={thread_id}")
            return

        was_subscribed = self.subscribers.is_subscribed(chat_id, thread_id=thread_id)
        sub = self.subscribers.subscribe(chat_id, chat_title, thread_id=thread_id, platform=platform)
        verbose_str = "on" if sub.verbose else "off"
        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower() or "telegram"
        if platform == "telegram":
            tip = "Tip: in groups @mention the bot, and in DM plain text is sent to foreman by default."
        elif platform in ("slack", "discord"):
            tip = "Channel tip: @mention the bot to route plain text. Use /send for explicit recipients."
        else:
            tip = "Tip: plain text routes to foreman by default; use /send for explicit recipients."
        target_label = self.group.doc.get("title", self.group.group_id)
        group_id = self.group.group_id
        if was_subscribed:
            headline = f"✅ Already authorized for this chat ({target_label} [{group_id}])"
        else:
            headline = f"✅ Subscribed to {target_label} [{group_id}]"
        self.adapter.send_message(
            chat_id,
            f"{headline}\n"
            f"Verbose mode: {verbose_str}\n"
            f"{tip}\n"
            f"Use /help for commands.",
            thread_id=thread_id,
        )
        self._log(f"[subscribe] chat={chat_id} thread={thread_id} title={chat_title}")

    def _handle_unsubscribe(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /unsubscribe command — also revokes authorization so re-subscribe requires key."""
        # Reload auth state — authorization may have been granted by the daemon
        # process (im_bind_chat), so in-memory _authorized can be stale.
        self.key_manager._load()
        was_subscribed = self.subscribers.unsubscribe(chat_id, thread_id=thread_id)
        self.key_manager.revoke(chat_id, thread_id)
        if was_subscribed:
            self.adapter.send_message(chat_id, "👋 Unsubscribed and authorization revoked. Use /subscribe to re-authenticate.", thread_id=thread_id)
        else:
            self.adapter.send_message(chat_id, "ℹ️ You were not subscribed. Authorization revoked.", thread_id=thread_id)
        self._log(f"[unsubscribe] chat={chat_id} thread={thread_id} (auth revoked)")

    def _handle_verbose(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /verbose command (toggle)."""
        new_value = self.subscribers.toggle_verbose(chat_id, thread_id=thread_id)
        if new_value is None:
            self.adapter.send_message(chat_id, "ℹ️ Please /subscribe first.", thread_id=thread_id)
        else:
            status = "ON - showing all messages" if new_value else "OFF - showing only messages to you"
            self.adapter.send_message(chat_id, f"👁 Verbose mode: {status}", thread_id=thread_id)
        self._log(f"[verbose] chat={chat_id} thread={thread_id} new_value={new_value}")

    def _handle_status(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /status command."""
        # Get group info
        resp = self._daemon({"op": "group_show", "args": {"group_id": self.group.group_id}})
        if not resp.get("ok"):
            self.adapter.send_message(chat_id, "❌ Failed to get status", thread_id=thread_id)
            return

        group_data = resp.get("result", {}).get("group", {})
        group_title = group_data.get("title", self.group.group_id)
        group_state = group_data.get("state", "active")
        running = resp.get("result", {}).get("running", False)

        # Get actors
        actors_resp = self._daemon({"op": "actor_list", "args": {"group_id": self.group.group_id}})
        actors = []
        if actors_resp.get("ok"):
            actors = actors_resp.get("result", {}).get("actors", [])

        status_text = format_status(group_title, group_state, running, actors)
        self.adapter.send_message(chat_id, status_text, thread_id=thread_id)

    def _handle_context(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /context command."""
        resp = self._daemon({"op": "context_get", "args": {"group_id": self.group.group_id}})
        if not resp.get("ok"):
            self.adapter.send_message(chat_id, "❌ Failed to get context", thread_id=thread_id)
            return

        context = resp.get("result", {})
        context_text = format_context(context)
        self.adapter.send_message(chat_id, context_text, thread_id=thread_id)

    def _handle_pause(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /pause command."""
        resp = self._daemon({
            "op": "group_set_state",
            "args": {"group_id": self.group.group_id, "state": "paused", "by": "user"},
        })
        if resp.get("ok"):
            self.adapter.send_message(chat_id, "⏸ Group paused. Message delivery stopped.", thread_id=thread_id)
        else:
            error = resp.get("error", {}).get("message", "unknown error")
            self.adapter.send_message(chat_id, f"❌ Failed to pause: {error}", thread_id=thread_id)
        self._log(f"[pause] chat={chat_id} thread={thread_id} ok={resp.get('ok')}")

    def _handle_resume(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /resume command."""
        resp = self._daemon({
            "op": "group_set_state",
            "args": {"group_id": self.group.group_id, "state": "active", "by": "user"},
        })
        if resp.get("ok"):
            self.adapter.send_message(chat_id, "▶️ Group resumed. Message delivery active.", thread_id=thread_id)
        else:
            error = resp.get("error", {}).get("message", "unknown error")
            self.adapter.send_message(chat_id, f"❌ Failed to resume: {error}", thread_id=thread_id)
        self._log(f"[resume] chat={chat_id} thread={thread_id} ok={resp.get('ok')}")

    def _handle_launch(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /launch command."""
        resp = self._daemon({
            "op": "group_start",
            "args": {"group_id": self.group.group_id, "by": "user"},
        })
        if resp.get("ok"):
            self.adapter.send_message(chat_id, "🚀 Launching all agents...", thread_id=thread_id)
        else:
            error = resp.get("error", {}).get("message", "unknown error")
            self.adapter.send_message(chat_id, f"❌ Failed to launch: {error}", thread_id=thread_id)
        self._log(f"[launch] chat={chat_id} thread={thread_id} ok={resp.get('ok')}")

    def _handle_quit(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /quit command."""
        resp = self._daemon({
            "op": "group_stop",
            "args": {"group_id": self.group.group_id, "by": "user"},
        })
        if resp.get("ok"):
            self.adapter.send_message(chat_id, "🛑 Stopping all agents...", thread_id=thread_id)
        else:
            error = resp.get("error", {}).get("message", "unknown error")
            self.adapter.send_message(chat_id, f"❌ Failed to quit: {error}", thread_id=thread_id)
        self._log(f"[quit] chat={chat_id} thread={thread_id} ok={resp.get('ok')}")

    def _handle_help(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /help command."""
        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower() or "telegram"
        self.adapter.send_message(chat_id, format_help(platform=platform), thread_id=thread_id)

    def _add_typing_indicator(self, chat_id: str, message_id: str) -> None:
        """Add a typing indicator (emoji reaction) to the user's message."""
        if not message_id:
            return
        reaction_id = self.adapter.add_reaction(message_id)
        if reaction_id:
            self._typing_indicators[chat_id] = (message_id, reaction_id)

    def _send_typing_action(self, chat_id: str) -> None:
        """Send a Telegram 'typing' chat action, throttled to once per 4 seconds."""
        now = time.time()
        last = self._typing_action_ts.get(chat_id, 0.0)
        if now - last < 4.0:
            return
        if self.adapter.send_chat_action(chat_id):
            self._typing_action_ts[chat_id] = now

    def _refresh_typing_actions(self) -> None:
        """Re-send 'typing' action for all active typing indicators."""
        for chat_id in list(self._typing_indicators):
            self._send_typing_action(chat_id)

    def _remove_typing_indicator(self, chat_id: str) -> None:
        """Remove the typing indicator for a chat, if any."""
        indicator = self._typing_indicators.pop(chat_id, None)
        self._typing_action_ts.pop(chat_id, None)
        if indicator:
            message_id, reaction_id = indicator
            self.adapter.remove_reaction(message_id, reaction_id)

    def _handle_message(
        self,
        chat_id: str,
        parsed: ParsedCommand,
        from_user: str,
        *,
        attachments: List[Dict[str, Any]],
        mention_user_ids: Optional[List[str]] = None,
        thread_id: int = 0,
        message_id: str = "",
        from_user_id: str = "",
    ) -> None:
        """Handle /send message (explicit routing)."""

        # Reload group from disk to reflect latest enabled/actor state.
        group = load_group(self.group.group_id)
        if group is None:
            self.adapter.send_message(chat_id, "❌ Failed to send: group not found (bridge stopped).", thread_id=thread_id)
            self._log(f"[message] chat={chat_id} thread={thread_id} error=group_not_found (stopping bridge)")
            self.stop()
            return

        # Parse recipients from leading args (supports multiple @targets, comma-separated).
        to: List[str] = []
        args = list(parsed.args or [])
        # Known-recipient set for validating @targets in args.
        _actor_ids = {str(a.get("id") or "").strip() for a in list_actors(group) if isinstance(a, dict)}
        _valid_selectors = {"@all", "@peers", "@foreman", "user"}
        while args:
            head = str(args[0] or "").strip()
            if not head:
                args.pop(0)
                continue
            if head.startswith("@") or head in ("user",):
                # Only consume tokens that are known CCCC actors/selectors.
                # Unknown @tokens (e.g. IM bot mentions like @BotName) stop consumption
                # so they fall through to message text instead of failing as invalid recipients.
                tokens = [t.strip() for t in head.split(",") if t.strip()]
                if all(t in _valid_selectors or t.lstrip("@") in _actor_ids for t in tokens):
                    args.pop(0)
                    to.extend(tokens)
                    continue
                break
            break
        msg_text = " ".join([str(x) for x in args]).strip()

        if not msg_text and not attachments:
            # Explicit /send but empty.
            return

        # If no explicit recipients were provided, try @mentions in the message text first.
        if not to and msg_text:
            mention_tokens: List[str] = []
            for m in re.findall(r"@(\w[\w-]*)", msg_text):
                if not m:
                    continue
                if m in ("all", "peers", "foreman"):
                    mention_tokens.append(f"@{m}")
                    continue
                if m in _actor_ids:
                    mention_tokens.append(m)
            if mention_tokens:
                to = mention_tokens

        # If still empty, apply the group policy for empty-recipient sends.
        if not to and get_default_send_to(group.doc) == "foreman":
            to = ["@foreman"]

        # Fail fast if the recipient set matches no enabled agents.
        # IMPORTANT: do this before downloading/storing attachments to avoid leaving orphan blobs.
        try:
            canonical_to = resolve_recipient_tokens(group, to)
        except Exception as e:
            self.adapter.send_message(chat_id, f"❌ Invalid recipient: {e}", thread_id=thread_id)
            return
        if to and not canonical_to:
            self.adapter.send_message(
                chat_id,
                "❌ Invalid recipient. Use /send @<agent> <message>, /send @all <message>, or /send @peers <message>.",
                thread_id=thread_id,
            )
            return

        enabled_actor_ids: List[str] = []
        for a in list_actors(group):
            if not isinstance(a, dict):
                continue
            if not coerce_bool(a.get("enabled"), default=True):
                continue
            aid = str(a.get("id") or "").strip()
            if aid:
                enabled_actor_ids.append(aid)
        enabled_set = set(enabled_actor_ids)
        foreman_id = enabled_actor_ids[0] if enabled_actor_ids else ""

        matched: set[str] = set()
        to_set = set(canonical_to)
        if not to_set:
            # Empty recipients = broadcast semantics.
            matched.update(enabled_actor_ids)
        if "@all" in to_set:
            matched.update(enabled_actor_ids)
        if "@foreman" in to_set and foreman_id:
            matched.add(foreman_id)
        if "@peers" in to_set:
            for aid in enabled_actor_ids:
                if aid != foreman_id:
                    matched.add(aid)
        for tok in canonical_to:
            if not tok or tok.startswith("@") or tok == "user":
                continue
            if tok in enabled_set:
                matched.add(tok)

        if not matched:
            # Check if disabled actors match — daemon will auto-wake them.
            disabled_matches = disabled_recipient_actor_ids(group, canonical_to)
            if not disabled_matches:
                wanted = " ".join(canonical_to) if canonical_to else "@all"
                self.adapter.send_message(
                    chat_id,
                    f"⚠️ No agents match the recipient(s): {wanted}. Run /status to check available agents.",
                    thread_id=thread_id,
                )
                self._log(f"[message] chat={chat_id} thread={thread_id} skipped (no targets) to={canonical_to}")
                return
            self._log(f"[message] chat={chat_id} thread={thread_id} auto-wake candidates: {disabled_matches}")

        # File settings (only after recipients are validated)
        im_cfg = group.doc.get("im") if isinstance(group.doc.get("im"), dict) else {}
        files_cfg = im_cfg.get("files") if isinstance(im_cfg.get("files"), dict) else {}
        files_enabled = coerce_bool(files_cfg.get("enabled"), default=True)
        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower()
        default_max_mb = 20 if platform == "telegram" else 10 if platform == "discord" else 20
        try:
            max_mb = int(files_cfg.get("max_mb") or default_max_mb)
        except Exception:
            max_mb = default_max_mb
        max_bytes = max(0, max_mb) * 1024 * 1024

        stored_attachments: List[Dict[str, Any]] = []
        if files_enabled and attachments:
            for a in attachments:
                if not isinstance(a, dict):
                    continue
                try:
                    size = int(a.get("bytes") or 0)
                except Exception:
                    size = 0
                if max_bytes and size and size > max_bytes:
                    self.adapter.send_message(chat_id, f"⚠️ Ignored: file too large (> {max_mb}MB).", thread_id=thread_id)
                    continue
                try:
                    raw = self.adapter.download_attachment(a)
                except Exception as e:
                    self.adapter.send_message(chat_id, f"❌ Failed to download attachment: {e}", thread_id=thread_id)
                    continue
                if max_bytes and len(raw) > max_bytes:
                    self.adapter.send_message(chat_id, f"⚠️ Ignored: file too large (> {max_mb}MB).", thread_id=thread_id)
                    continue
                normalized_filename, normalized_mime_type, normalized_kind = _normalize_inbound_attachment_metadata(
                    raw=raw,
                    filename=str(a.get("file_name") or a.get("filename") or "file"),
                    mime_type=str(a.get("mime_type") or a.get("content_type") or ""),
                    kind=str(a.get("kind") or "file"),
                )
                stored_attachments.append(
                    store_blob_bytes(
                        group,
                        data=raw,
                        filename=normalized_filename,
                        mime_type=normalized_mime_type,
                        kind=normalized_kind,
                    )
                )

        msg_text = _normalize_inbound_attachment_message_text(msg_text, stored_attachments)

        if not msg_text and stored_attachments:
            if len(stored_attachments) == 1:
                msg_text = f"[file] {stored_attachments[0].get('title') or 'file'}"
            else:
                msg_text = f"[files] {len(stored_attachments)} attachments"

        if not msg_text and not stored_attachments:
            # Nothing left to send (all attachments were ignored / failed).
            return

        cleaned_mention_user_ids = [str(item).strip() for item in (mention_user_ids or []) if str(item).strip()]

        resp = self._daemon({
            "op": "send",
            "args": {
                "group_id": self.group.group_id,
                "text": msg_text,
                "by": "user",
                "to": canonical_to,
                "path": "",
                "attachments": stored_attachments,
                "source_platform": str(getattr(self.adapter, "platform", "") or "").strip() or None,
                "source_user_name": from_user or None,
                "source_user_id": from_user_id or None,
                "mention_user_ids": cleaned_mention_user_ids or None,
            },
        })

        if not resp.get("ok"):
            err = resp.get("error") if isinstance(resp.get("error"), dict) else {}
            code = str(err.get("code") or "").strip()
            message = str(err.get("message") or "unknown error")
            suffix = " (bridge stopped)" if code == "group_not_found" else ""
            self.adapter.send_message(chat_id, f"❌ Failed to send: {message}{suffix}", thread_id=thread_id)
            self._log(f"[message] chat={chat_id} thread={thread_id} error={code}:{message}")
            if code == "group_not_found":
                # Fatal misconfig: prevent spamming and competing bot pollers.
                self.stop()
        else:
            # Add typing indicator to show that the message is being processed.
            self._add_typing_indicator(chat_id, message_id)
            self._send_typing_action(chat_id)
            self._log(
                f"[message] chat={chat_id} thread={thread_id} from={from_user}({from_user_id}) to={canonical_to} len={len(msg_text)} files={len(stored_attachments)}"
            )


def start_bridge(group_id: str, platform: str = "telegram") -> None:
    """
    Start IM bridge for a group.

    This is the main entry point called by CLI.
    """
    # Load group
    group = load_group(group_id)
    if group is None:
        print(f"[error] Group not found: {group_id}")
        sys.exit(1)

    # Get IM config from group
    im_config = canonicalize_im_config(group.doc.get("im", {}))
    if not im_config:
        print(f"[error] No IM configuration for group {group_id}")
        print("Run: cccc im set telegram --group " + group_id)
        sys.exit(1)

    configured_platform = im_config.get("platform", "").lower()
    if configured_platform != platform.lower():
        print(f"[error] Group configured for {configured_platform}, not {platform}")
        sys.exit(1)

    # Resolve credentials based on platform.
    # Note: some platforms support either "store the value in group.yaml" or "store an env var name".
    bot_token: Optional[str] = None
    app_token: Optional[str] = None  # Slack only (for Socket Mode inbound)
    feishu_app_id: str = ""
    feishu_app_secret: str = ""
    dingtalk_app_key: str = ""
    dingtalk_app_secret: str = ""
    dingtalk_robot_code: str = ""
    weixin_account_id: str = ""

    def _resolve_secret(*, value_key: str, env_key: str, default_env: str) -> str:
        raw = str(im_config.get(value_key) or "").strip()
        if raw:
            return raw
        env_name_raw = str(im_config.get(env_key) or "").strip()
        env_name = env_name_raw if _is_env_var_name(env_name_raw) else ""
        if env_name:
            return str(os.environ.get(env_name, "") or "").strip()
        # Common misconfig: raw secret pasted into *_env field.
        if env_name_raw and not env_name:
            return env_name_raw
        return str(os.environ.get(default_env, "") or "").strip()

    if platform.lower() == "slack":
        # Slack requires bot_token (xoxb-) for outbound, app_token (xapp-) for inbound
        bot_token_env_raw = str(im_config.get("bot_token_env") or "").strip()
        bot_token_env = bot_token_env_raw if _is_env_var_name(bot_token_env_raw) else ""
        if bot_token_env:
            bot_token = os.environ.get(bot_token_env, "").strip()
        if not bot_token:
            bot_token = str(im_config.get("bot_token") or "").strip()
        if not bot_token and bot_token_env_raw and not bot_token_env:
            # Common misconfig: raw token pasted into *_env field.
            bot_token = bot_token_env_raw

        app_token_env_raw = str(im_config.get("app_token_env") or "").strip()
        app_token_env = app_token_env_raw if _is_env_var_name(app_token_env_raw) else ""
        if app_token_env:
            app_token = os.environ.get(app_token_env, "").strip()
        if not app_token:
            app_token = str(im_config.get("app_token") or "").strip()
        if not app_token and app_token_env_raw and not app_token_env:
            app_token = app_token_env_raw

        if not bot_token:
            print(f"[error] No bot token configured for Slack")
            if bot_token_env:
                print(f"Set environment variable: {bot_token_env}")
            sys.exit(1)

        # app_token is optional (inbound disabled without it)
        if not app_token:
            print("[warn] No app token configured - inbound messages disabled")
    elif platform.lower() == "feishu":
        # Feishu/Lark uses app_id + app_secret.
        # Resolve early so the singleton lock can be keyed by credentials.
        feishu_app_id = _resolve_secret(
            value_key="feishu_app_id",
            env_key="feishu_app_id_env",
            default_env="FEISHU_APP_ID",
        )
        feishu_app_secret = _resolve_secret(
            value_key="feishu_app_secret",
            env_key="feishu_app_secret_env",
            default_env="FEISHU_APP_SECRET",
        )
        if not feishu_app_id or not feishu_app_secret:
            print(
                "[error] Feishu/Lark requires app_id + app_secret (set via group IM config or FEISHU_APP_ID/FEISHU_APP_SECRET)."
            )
            sys.exit(1)

        # Optional domain override to support Lark (Global).
        feishu_domain_raw = (
            str(im_config.get("feishu_domain") or os.environ.get("FEISHU_DOMAIN") or "")
            .strip()
            .lower()
        )
        feishu_domain_raw = feishu_domain_raw.rstrip("/")
        if feishu_domain_raw.endswith("/open-apis"):
            feishu_domain_raw = feishu_domain_raw[: -len("/open-apis")].rstrip("/")
        if feishu_domain_raw in (
            "lark",
            "global",
            "intl",
            "international",
            "https://open.larkoffice.com",
            "open.larkoffice.com",
            # Historical alias used in some SDKs/docs.
            "https://open.larksuite.com",
            "open.larksuite.com",
        ):
            im_config["feishu_domain"] = "https://open.larkoffice.com"
        elif feishu_domain_raw in (
            "feishu",
            "cn",
            "china",
            "https://open.feishu.cn",
            "open.feishu.cn",
            "",
        ):
            im_config["feishu_domain"] = "https://open.feishu.cn"
        else:
            # Keep a safe default (do not allow arbitrary domains from env here).
            im_config["feishu_domain"] = "https://open.feishu.cn"
    elif platform.lower() == "dingtalk":
        # DingTalk uses app_key + app_secret (+ optional robot_code).
        # Resolve early so the singleton lock can be keyed by credentials.
        dingtalk_app_key = _resolve_secret(
            value_key="dingtalk_app_key",
            env_key="dingtalk_app_key_env",
            default_env="DINGTALK_APP_KEY",
        )
        dingtalk_app_secret = _resolve_secret(
            value_key="dingtalk_app_secret",
            env_key="dingtalk_app_secret_env",
            default_env="DINGTALK_APP_SECRET",
        )
        dingtalk_robot_code = _resolve_secret(
            value_key="dingtalk_robot_code",
            env_key="dingtalk_robot_code_env",
            default_env="DINGTALK_ROBOT_CODE",
        )
        if not dingtalk_app_key or not dingtalk_app_secret:
            print(
                "[error] DingTalk requires app_key + app_secret (set via group IM config or DINGTALK_APP_KEY/DINGTALK_APP_SECRET)."
            )
            sys.exit(1)
    elif platform.lower() == "wecom":
        wecom_bot_id = _resolve_secret(
            value_key="wecom_bot_id",
            env_key="wecom_bot_id_env",
            default_env="WECOM_BOT_ID",
        )
        wecom_secret = _resolve_secret(
            value_key="wecom_secret",
            env_key="wecom_secret_env",
            default_env="WECOM_SECRET",
        )
        if not wecom_bot_id or not wecom_secret:
            print(
                "[error] WeCom requires bot_id + secret (set via group IM config or WECOM_BOT_ID/WECOM_SECRET)."
            )
            sys.exit(1)
    elif platform.lower() == "weixin":
        weixin_account_id = str(
            im_config.get("weixin_account_id")
            or os.environ.get("CCCC_IM_WEIXIN_ACCOUNT_ID")
            or ""
        ).strip()
    else:
        # Telegram/Discord: single token
        token_env_raw = str(im_config.get("token_env") or im_config.get("bot_token_env") or "").strip()
        token_env = token_env_raw if _is_env_var_name(token_env_raw) else ""
        if token_env:
            bot_token = os.environ.get(token_env, "").strip()
        if not bot_token:
            bot_token = str(im_config.get("token") or im_config.get("bot_token") or "").strip()
        if not bot_token and token_env_raw and not token_env:
            # Common misconfig: raw token pasted into *_env field.
            bot_token = token_env_raw

        if not bot_token:
            print(f"[error] No token configured for {platform}")
            if token_env:
                print(f"Set environment variable: {token_env}")
            sys.exit(1)

    # Paths
    state_dir = group.path / "state"
    log_path = state_dir / "im_bridge.log"
    lock_path = state_dir / "im_bridge.lock"
    pid_path = state_dir / "im_bridge.pid"

    # Acquire global singleton lock per credential set to avoid multiple groups (or
    # multiple processes) consuming the same inbound stream (Telegram getUpdates, Slack Socket Mode, etc.).
    lock_identity = ""
    if platform.lower() == "slack":
        lock_identity = f"slack|bot={bot_token or ''}|app={app_token or ''}"
    elif platform.lower() == "feishu":
        lock_identity = f"feishu|domain={str(im_config.get('feishu_domain') or '')}|app_id={feishu_app_id}"
    elif platform.lower() == "dingtalk":
        lock_identity = f"dingtalk|app_key={dingtalk_app_key}"
    elif platform.lower() == "wecom":
        lock_identity = f"wecom|bot_id={wecom_bot_id}"
    elif platform.lower() == "weixin":
        lock_identity = f"weixin|cred_path={state_dir / 'im_weixin_credentials.json'}"
    else:
        lock_identity = f"{platform.lower()}|token={bot_token or ''}"
    token_material = lock_identity
    token_fingerprint = hashlib.sha256(token_material.encode("utf-8")).hexdigest()[:12]
    token_lock_path = ensure_home() / "locks" / f"im_bridge_{platform.lower()}_{token_fingerprint}.lock"
    token_lock_file = _acquire_singleton_lock(token_lock_path)
    if token_lock_file is None:
        other_pid = ""
        try:
            other_pid = token_lock_path.read_text(encoding="utf-8").strip().splitlines()[0]
        except Exception:
            other_pid = ""
        pid_hint = f" (pid={other_pid})" if other_pid else ""
        print(f"[error] Another {platform} bridge is already running for this credential set{pid_hint}")
        print("Stop it before starting a new bridge, or use different credentials.")
        sys.exit(1)

    # Acquire singleton lock
    group_lock_file = _acquire_singleton_lock(lock_path)
    if group_lock_file is None:
        print("[error] Another bridge instance is already running")
        try:
            token_lock_file.close()
        except Exception:
            pass
        sys.exit(1)

    # Write PID file
    pid_path.parent.mkdir(parents=True, exist_ok=True)
    pid_path.write_text(str(os.getpid()), encoding="utf-8")

    # Create adapter
    if platform.lower() == "telegram":
        adapter = TelegramAdapter(token=bot_token, log_path=log_path)
    elif platform.lower() == "slack":
        adapter = SlackAdapter(bot_token=bot_token, app_token=app_token, log_path=log_path)
    elif platform.lower() == "discord":
        adapter = DiscordAdapter(token=bot_token, log_path=log_path)
    elif platform.lower() == "feishu":
        from .adapters.feishu import FeishuAdapter
        adapter = FeishuAdapter(
            app_id=feishu_app_id,
            app_secret=feishu_app_secret,
            domain=str(im_config.get("feishu_domain") or "https://open.feishu.cn"),
            log_path=log_path,
        )
    elif platform.lower() == "dingtalk":
        from .adapters.dingtalk import DingTalkAdapter
        adapter = DingTalkAdapter(
            app_key=dingtalk_app_key,
            app_secret=dingtalk_app_secret,
            robot_code=dingtalk_robot_code,
            log_path=log_path,
        )
    elif platform.lower() == "wecom":
        from .adapters.wecom import WecomAdapter
        adapter = WecomAdapter(
            bot_id=wecom_bot_id,
            secret=wecom_secret,
            log_path=log_path,
            ws_url=str(im_config.get("wecom_ws_url") or ""),
        )
    elif platform.lower() == "weixin":
        from .adapters.weixin import WeixinAdapter
        adapter = WeixinAdapter(
            account_id=weixin_account_id,
            log_path=log_path,
            cred_path=state_dir / "im_weixin_credentials.json",
            context_cache_path=state_dir / "im_weixin_context_tokens.json",
        )
    else:
        print(f"[error] Unsupported platform: {platform}")
        sys.exit(1)

    # Read skip_pending_on_start option from config
    # When True, bridge will skip messages that accumulated during downtime
    skip_pending = coerce_bool(im_config.get("skip_pending_on_start"), default=True)

    # Create and start bridge
    bridge = IMBridge(
        group=group,
        adapter=adapter,
        log_path=log_path,
        skip_pending_on_start=skip_pending,
    )

    # Setup signal handlers
    def handle_signal(signum: int, frame: Any) -> None:
        print(f"\n[signal] Received signal {signum}, stopping...")
        bridge.stop()

    signal.signal(signal.SIGTERM, handle_signal)
    signal.signal(signal.SIGINT, handle_signal)

    # Start
    if not bridge.start():
        print("[error] Failed to start bridge")
        sys.exit(1)

    print(f"[info] IM Bridge started for group {group_id}")
    print(f"[info] Platform: {platform}")
    print(f"[info] Log: {log_path}")
    print("[info] Press Ctrl+C to stop")

    # Run
    try:
        bridge.run_forever()
    finally:
        # Cleanup
        try:
            pid_path.unlink(missing_ok=True)
        except Exception:
            pass
        try:
            group_lock_file.close()
        except Exception:
            pass
        try:
            token_lock_file.close()
        except Exception:
            pass

    print("[info] Bridge stopped")


# Entry point for running as module: python -m cccc.ports.im.bridge <group_id> [platform]
if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python -m cccc.ports.im.bridge <group_id> [platform]")
        print("  platform: telegram (default), slack, discord, feishu (Feishu/Lark), dingtalk, wecom, weixin")
        print("")
        print("Environment variables:")
        print("  Telegram: TELEGRAM_BOT_TOKEN")
        print("  Slack:    SLACK_BOT_TOKEN, SLACK_APP_TOKEN (optional)")
        print("  Discord:  DISCORD_BOT_TOKEN")
        print("  Feishu/Lark: FEISHU_APP_ID, FEISHU_APP_SECRET, FEISHU_DOMAIN (optional: feishu|lark|https://...)")
        print("  DingTalk: DINGTALK_APP_KEY, DINGTALK_APP_SECRET, DINGTALK_ROBOT_CODE (optional)")
        print("  WeCom:    WECOM_BOT_ID, WECOM_SECRET")
        print("  Weixin:   CCCC_IM_WEIXIN_ACCOUNT_ID (optional)")
        sys.exit(1)

    _group_id = sys.argv[1]
    _platform = sys.argv[2] if len(sys.argv) > 2 else "telegram"
    start_bridge(_group_id, _platform)
