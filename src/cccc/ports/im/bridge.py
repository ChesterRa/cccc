"""
CCCC IM Bridge - Core logic.

Handles:
- Inbound: IM messages → daemon API → ledger
- Outbound: ledger events → filter → IM
- Command processing
- Ledger watching (cursor-based tail)
"""

from __future__ import annotations

import json
import hashlib
import os
import re
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
from .adapters.base import IMAdapter
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
from .subscribers import SubscriberManager
from ...util.file_lock import LockUnavailableError, acquire_lockfile


def _now() -> str:
    return time.strftime("%Y-%m-%d %H:%M:%S")


def _is_env_var_name(value: str) -> bool:
    return bool(re.fullmatch(r"[A-Z_][A-Z0-9_]*", (value or "").strip()))


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
        self.watcher = LedgerWatcher(group, log_fn=self._log)

        self._running = False
        self._last_outbound_check = 0.0
        # Best-effort inbound dedupe to prevent double-processing when platforms
        # retry delivery or emit multiple events for the same message (e.g., edits).
        self._seen_inbound: Dict[str, float] = {}

        # Typing indicator state: chat_id -> (message_id, reaction_id)
        # Used to show a "processing" emoji on the user's message while agents work.
        self._typing_indicators: Dict[str, Tuple[str, str]] = {}

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

        # Process outbound events (throttled)
        now = time.time()
        if now - self._last_outbound_check >= 1.0:
            self._process_outbound()
            self._last_outbound_check = now

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
        messages = self.adapter.poll()

        for msg in messages:
            chat_id = str(msg.get("chat_id") or "").strip()
            text = str(msg.get("text") or "")
            chat_title = str(msg.get("chat_title") or "")
            from_user = str(msg.get("from_user") or "user")
            chat_type = str(msg.get("chat_type") or "").strip().lower()
            try:
                thread_id = int(msg.get("thread_id") or 0)
            except Exception:
                thread_id = 0
            message_id = str(msg.get("message_id") or "").strip()

            if not text:
                continue
            if not self._should_process_inbound(chat_id=chat_id, thread_id=thread_id, message_id=message_id):
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
                self._handle_message(chat_id, parsed, from_user, attachments=attachments, thread_id=thread_id, message_id=message_id)
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
                    # Build a SEND-like ParsedCommand so _handle_message can extract @targets from args.
                    implicit_args = parsed.text.split() if parsed.text else []
                    implicit_send = ParsedCommand(
                        type=CommandType.SEND,
                        text=parsed.text,
                        mentions=parsed.mentions,
                        args=implicit_args,
                    )
                    self._handle_message(chat_id, implicit_send, from_user, attachments=attachments, thread_id=thread_id, message_id=message_id)
                    continue

                # Non-routed messages are ignored.
                continue

    def _process_outbound(self) -> None:
        """Process outbound events from ledger."""
        events = self.watcher.poll()

        for event in events:
            self._forward_event(event)

    def _forward_event(self, event: Dict[str, Any]) -> None:
        """Forward a ledger event to subscribed chats."""
        kind = event.get("kind", "")
        by = event.get("by", "")

        # Skip user messages (they come from IM, Web, or CLI - avoid echo)
        # We only forward agent messages and system notifications
        if by == "user":
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

        if not text and not attachments:
            return

        # Forward to subscribed chats (filtered by platform to avoid cross-platform sends)
        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower()
        subscribed = self.subscribers.get_subscribed_targets(platform=platform)

        # Remove typing indicators for chats that are about to receive a reply.
        for sub in subscribed:
            self._remove_typing_indicator(sub.chat_id)

        for sub in subscribed:
            verbose = bool(sub.verbose)

            # Filter based on verbose setting
            if not self._should_forward(event, verbose):
                continue

            # Format message (may be empty for file-only events)
            formatted = self.adapter.format_outbound(by, to, text, is_system) if text else ""

            # Try file delivery first (if any attachments)
            sent_any_file = False
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
                    cap = formatted if (i == 0 and formatted) else ""
                    ok = False
                    try:
                        ok = bool(self.adapter.send_file(sub.chat_id, file_path=abs_path, filename=title, caption=cap, thread_id=sub.thread_id))
                    except Exception:
                        ok = False
                    if ok:
                        sent_any_file = True

            # If we didn't send any files, or if there's text with no files, send message.
            if formatted and not sent_any_file:
                self.adapter.send_message(sub.chat_id, formatted, thread_id=sub.thread_id)

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
        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower()
        sub = self.subscribers.subscribe(chat_id, chat_title, thread_id=thread_id, platform=platform)
        verbose_str = "on" if sub.verbose else "off"
        platform = str(getattr(self.adapter, "platform", "") or "").strip().lower() or "telegram"
        if platform == "telegram":
            tip = "Tip: in groups @mention the bot, and in DM plain text is sent to foreman by default."
        elif platform in ("slack", "discord"):
            tip = "Channel tip: @mention the bot to route plain text. Use /send for explicit recipients."
        else:
            tip = "Tip: plain text routes to foreman by default; use /send for explicit recipients."
        self.adapter.send_message(
            chat_id,
            f"✅ Subscribed to {self.group.doc.get('title', self.group.group_id)}\n"
            f"Verbose mode: {verbose_str}\n"
            f"{tip}\n"
            f"Use /help for commands.",
            thread_id=thread_id,
        )
        self._log(f"[subscribe] chat={chat_id} thread={thread_id} title={chat_title}")

    def _handle_unsubscribe(self, chat_id: str, thread_id: int = 0) -> None:
        """Handle /unsubscribe command."""
        was_subscribed = self.subscribers.unsubscribe(chat_id, thread_id=thread_id)
        if was_subscribed:
            self.adapter.send_message(chat_id, "👋 Unsubscribed. You will no longer receive messages.", thread_id=thread_id)
        else:
            self.adapter.send_message(chat_id, "ℹ️ You were not subscribed.", thread_id=thread_id)
        self._log(f"[unsubscribe] chat={chat_id} thread={thread_id}")

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

    def _remove_typing_indicator(self, chat_id: str) -> None:
        """Remove the typing indicator for a chat, if any."""
        indicator = self._typing_indicators.pop(chat_id, None)
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
        thread_id: int = 0,
        message_id: str = "",
    ) -> None:
        """Handle /send message (explicit routing)."""
        _ = from_user

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
                stored_attachments.append(
                    store_blob_bytes(
                        group,
                        data=raw,
                        filename=str(a.get("file_name") or a.get("filename") or "file"),
                        mime_type=str(a.get("mime_type") or a.get("content_type") or ""),
                        kind=str(a.get("kind") or "file"),
                    )
                )

        if not msg_text and stored_attachments:
            if len(stored_attachments) == 1:
                msg_text = f"[file] {stored_attachments[0].get('title') or 'file'}"
            else:
                msg_text = f"[files] {len(stored_attachments)} attachments"

        if not msg_text and not stored_attachments:
            # Nothing left to send (all attachments were ignored / failed).
            return

        resp = self._daemon({
            "op": "send",
            "args": {
                "group_id": self.group.group_id,
                "text": msg_text,
                "by": "user",
                "to": canonical_to,
                "path": "",
                "attachments": stored_attachments,
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
            self._log(
                f"[message] chat={chat_id} thread={thread_id} to={canonical_to} len={len(msg_text)} files={len(stored_attachments)}"
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
    im_config = group.doc.get("im", {})
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
    else:
        # Telegram/Discord: single token
        token_env_raw = str(im_config.get("token_env") or im_config.get("bot_token_env") or "").strip()
        token_env = token_env_raw if _is_env_var_name(token_env_raw) else ""
        if token_env:
            bot_token = os.environ.get(token_env, "").strip()
        if not bot_token:
            bot_token = str(im_config.get("token") or "").strip()
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
        print("  platform: telegram (default), slack, discord, feishu (Feishu/Lark), dingtalk")
        print("")
        print("Environment variables:")
        print("  Telegram: TELEGRAM_BOT_TOKEN")
        print("  Slack:    SLACK_BOT_TOKEN, SLACK_APP_TOKEN (optional)")
        print("  Discord:  DISCORD_BOT_TOKEN")
        print("  Feishu/Lark: FEISHU_APP_ID, FEISHU_APP_SECRET, FEISHU_DOMAIN (optional: feishu|lark|https://...)")
        print("  DingTalk: DINGTALK_APP_KEY, DINGTALK_APP_SECRET, DINGTALK_ROBOT_CODE (optional)")
        sys.exit(1)

    _group_id = sys.argv[1]
    _platform = sys.argv[2] if len(sys.argv) > 2 else "telegram"
    start_bridge(_group_id, _platform)
