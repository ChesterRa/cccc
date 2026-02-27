"""
Discord adapter for CCCC IM Bridge.

Uses discord.py library with Gateway connection for both inbound and outbound.
"""

from __future__ import annotations

import asyncio
import json
import re
import threading
import time
import urllib.request
from pathlib import Path
from typing import Any, Dict, List, Optional

from .base import IMAdapter

# Discord limits
DISCORD_MAX_MESSAGE_LENGTH = 2000
DEFAULT_MAX_CHARS = 2000
DEFAULT_MAX_LINES = 64


class DiscordAdapter(IMAdapter):
    """
    Discord adapter using discord.py Gateway.

    Runs the async event loop in a background thread.
    """

    platform = "discord"

    def __init__(
        self,
        token: str,
        log_path: Optional[Path] = None,
        max_chars: int = DEFAULT_MAX_CHARS,
        max_lines: int = DEFAULT_MAX_LINES,
    ):
        self.token = token
        self.log_path = log_path
        self.max_chars = max_chars
        self.max_lines = max_lines

        self._connected = False
        self._client: Any = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._thread: Optional[threading.Thread] = None
        self._message_queue: List[Dict[str, Any]] = []
        self._queue_lock = threading.Lock()
        self._ready_event = threading.Event()

    def _log(self, msg: str) -> None:
        """Append to log file if configured."""
        if self.log_path:
            try:
                self.log_path.parent.mkdir(parents=True, exist_ok=True)
                with self.log_path.open("a", encoding="utf-8") as f:
                    ts = time.strftime("%Y-%m-%d %H:%M:%S")
                    f.write(f"{ts} {msg}\n")
            except Exception:
                pass

    def connect(self) -> bool:
        """
        Initialize Discord client and start event loop in background thread.

        Requires discord.py package.
        """
        # Clear message queue on reconnect to avoid duplicate messages
        with self._queue_lock:
            self._message_queue.clear()

        try:
            import discord
        except ImportError:
            self._log("[error] discord.py not installed. Run: pip install discord.py")
            return False

        # Create client with message content intent
        intents = discord.Intents.default()
        intents.message_content = True
        self._client = discord.Client(intents=intents)

        # Register event handlers
        @self._client.event
        async def on_ready():
            self._log(f"[connect] Connected as {self._client.user}")
            self._ready_event.set()

        @self._client.event
        async def on_message(message):
            await self._handle_message(message)

        # Start event loop in background thread
        def run_loop():
            self._loop = asyncio.new_event_loop()
            asyncio.set_event_loop(self._loop)
            try:
                self._loop.run_until_complete(self._client.start(self.token))
            except Exception as e:
                self._log(f"[error] Discord client error: {e}")
            finally:
                self._loop.close()

        self._thread = threading.Thread(target=run_loop, daemon=True)
        self._thread.start()

        # Wait for ready with timeout
        if self._ready_event.wait(timeout=30):
            self._connected = True
            return True
        else:
            self._log("[error] Discord connection timeout")
            return False

    async def _handle_message(self, message: Any) -> None:
        """Handle incoming Discord message."""
        try:
            # Skip messages from self
            if message.author == self._client.user:
                return

            text = message.content or ""
            if not text:
                return

            attachments: List[Dict[str, Any]] = []
            try:
                for a in (getattr(message, "attachments", None) or []):
                    url = str(getattr(a, "url", "") or "").strip()
                    if not url:
                        continue
                    attachments.append({
                        "provider": "discord",
                        "kind": "file",
                        "url": url,
                        "file_name": str(getattr(a, "filename", "") or "file"),
                        "mime_type": str(getattr(a, "content_type", "") or ""),
                        "bytes": int(getattr(a, "size", 0) or 0),
                    })
            except Exception:
                attachments = []

            chat_type = "private" if getattr(message, "guild", None) is None else "channel"

            directed = False
            if chat_type == "private":
                directed = True
            elif self._client.user:
                try:
                    directed = any(
                        getattr(u, "id", None) == getattr(self._client.user, "id", None)
                        for u in (getattr(message, "mentions", None) or [])
                    )
                except Exception:
                    directed = False

            # In non-private channels, require an explicit bot mention to route messages.
            if not directed:
                return

            # Strip self-mention from beginning
            if self._client.user:
                text = re.sub(rf"^\s*(?:<@!?{self._client.user.id}>\s*)+", "", text)

            chat_id = str(message.channel.id)
            chat_title = getattr(message.channel, "name", None) or chat_id
            from_user = message.author.name or str(message.author.id)
            try:
                msg_ts = float(message.created_at.timestamp())  # type: ignore[union-attr]
            except Exception:
                msg_ts = 0.0

            # Queue the message
            with self._queue_lock:
                self._message_queue.append({
                    "chat_id": chat_id,
                    "chat_title": chat_title,
                    "chat_type": chat_type,
                    "routed": bool(directed),
                    "text": text.strip(),
                    "attachments": attachments,
                    "from_user": from_user,
                    "message_id": str(message.id),
                    "timestamp": msg_ts,
                })

            self._log(f"[inbound] channel={chat_id} user={from_user} text={text[:50]}...")

        except Exception as e:
            self._log(f"[error] handle_message: {e}")

    def disconnect(self) -> None:
        """Disconnect from Discord."""
        if self._client and self._loop:
            try:
                # Schedule close on the event loop
                future = asyncio.run_coroutine_threadsafe(
                    self._client.close(),
                    self._loop
                )
                future.result(timeout=5)
            except Exception:
                pass
        self._connected = False
        self._log("[disconnect] Disconnected")

    def poll(self) -> List[Dict[str, Any]]:
        """
        Return queued messages from Discord Gateway.

        Messages are queued by the on_message event handler.
        """
        if not self._connected:
            return []

        with self._queue_lock:
            messages = list(self._message_queue)
            self._message_queue.clear()

        return messages

    def send_message(self, chat_id: str, text: str, thread_id: Optional[int] = None) -> bool:
        """
        Send a message to a Discord channel.
        """
        _ = thread_id  # Discord threads are not wired yet (future work).
        if not self._connected or not self._client or not self._loop:
            return False

        if not text:
            return True

        # Ensure message fits Discord limit
        safe_text = self._compose_safe(text)

        try:
            # Get channel and send
            async def do_send():
                try:
                    cid = int(chat_id)
                except Exception:
                    cid = None
                channel = self._client.get_channel(cid) if cid is not None else None
                if channel:
                    await channel.send(safe_text)
                    return True
                else:
                    self._log(f"[warn] Channel {chat_id} not found")
                    return False

            future = asyncio.run_coroutine_threadsafe(do_send(), self._loop)
            return future.result(timeout=10)
        except Exception as e:
            self._log(f"[error] send_message to {chat_id}: {e}")
            return False

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        url = str(attachment.get("url") or "").strip()
        if not url:
            raise ValueError("missing discord attachment url")
        req = urllib.request.Request(url, method="GET")
        with urllib.request.urlopen(req, timeout=30) as resp:
            return resp.read()

    def send_file(
        self,
        chat_id: str,
        *,
        file_path: Path,
        filename: str,
        caption: str = "",
        thread_id: Optional[int] = None,
    ) -> bool:
        _ = thread_id  # Discord threads are not wired yet (future work).
        if not self._connected or not self._client or not self._loop:
            return False

        safe_text = self._compose_safe(caption) if caption else ""

        try:
            import discord
        except Exception:
            return False

        try:
            async def do_send():
                try:
                    cid = int(chat_id)
                except Exception:
                    cid = None
                channel = self._client.get_channel(cid) if cid is not None else None
                if not channel:
                    self._log(f"[warn] Channel {chat_id} not found")
                    return False
                try:
                    f = discord.File(fp=str(file_path), filename=str(filename or file_path.name or "file"))
                except Exception:
                    f = discord.File(fp=str(file_path))
                await channel.send(content=safe_text or None, file=f)
                return True

            future = asyncio.run_coroutine_threadsafe(do_send(), self._loop)
            return future.result(timeout=30)
        except Exception as e:
            self._log(f"[error] send_file to {chat_id}: {e}")
            return False

    def _compose_safe(self, text: str) -> str:
        """Ensure message fits within Discord limits."""
        summarized = self.summarize(text, self.max_chars, self.max_lines)

        if len(summarized) > DISCORD_MAX_MESSAGE_LENGTH:
            summarized = summarized[: DISCORD_MAX_MESSAGE_LENGTH - 1] + "…"

        return summarized

    def get_chat_title(self, chat_id: str) -> str:
        """Get channel name."""
        if not self._client:
            return str(chat_id)

        try:
            try:
                cid = int(chat_id)
            except Exception:
                cid = None
            channel = self._client.get_channel(cid) if cid is not None else None
            if channel:
                return getattr(channel, "name", str(chat_id))
        except Exception:
            pass
        return str(chat_id)

    def format_outbound(self, by: str, to: List[str], text: str, is_system: bool = False) -> str:
        """Format message for Discord display."""
        formatted = super().format_outbound(by, to, text, is_system)
        return self._compose_safe(formatted)
