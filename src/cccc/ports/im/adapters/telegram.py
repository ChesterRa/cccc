"""
Telegram Bot API adapter for CCCC IM Bridge.

Reuses battle-tested code from v0.3.28 bridge_telegram.py:
- tg_api(): API call wrapper with JSON encoding, timeout, error handling
- tg_poll(): Long-poll getUpdates
- Rate limiting and message formatting
- Singleton lock to prevent duplicate instances
"""

from __future__ import annotations

import json
import os
import re
import threading
import time
import uuid
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from .base import IMAdapter

# Telegram API limits
TELEGRAM_MAX_MESSAGE_LENGTH = 4096
DEFAULT_MAX_CHARS = 4096
DEFAULT_MAX_LINES = 64


class RateLimiter:
    """
    Rate limiter for Telegram API.

    Telegram limits:
    - Same chat: ~1 msg/sec
    - Different chats: ~30 msg/sec
    """

    def __init__(self, max_per_second: float = 1.0):
        self.min_interval = 1.0 / max_per_second
        self.last_send: Dict[str, float] = {}  # chat_id -> timestamp
        self.lock = threading.Lock()

    def acquire(self, chat_id: str) -> float:
        """
        Check if we can send to this chat.
        Returns wait time in seconds (0 if can send immediately).
        """
        with self.lock:
            now = time.time()
            last = self.last_send.get(chat_id, 0)
            elapsed = now - last

            if elapsed >= self.min_interval:
                self.last_send[chat_id] = now
                return 0.0
            else:
                return self.min_interval - elapsed

    def wait_and_acquire(self, chat_id: str) -> None:
        """Wait if needed, then acquire."""
        wait_time = self.acquire(chat_id)
        if wait_time > 0:
            time.sleep(wait_time)
            self.acquire(chat_id)


class TelegramAdapter(IMAdapter):
    """
    Telegram Bot API adapter using long-poll getUpdates.
    """

    platform = "telegram"

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

        self._offset = 0
        self._rate_limiter = RateLimiter(max_per_second=1.0)
        self._connected = False
        self._bot_info: Optional[Dict[str, Any]] = None
        self._bot_username = ""

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

    def _api(
        self,
        method: str,
        params: Optional[Dict[str, Any]] = None,
        timeout: int = 35,
    ) -> Dict[str, Any]:
        """
        Call Telegram Bot API.

        Uses JSON body for consistent encoding (handles non-ASCII text).
        """
        url = f"https://api.telegram.org/bot{self.token}/{method}"
        data = json.dumps(params or {}, ensure_ascii=False).encode("utf-8")

        req = urllib.request.Request(url, data=data, method="POST")
        req.add_header("Content-Type", "application/json; charset=utf-8")
        req.add_header("Accept", "application/json")

        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                body = resp.read().decode("utf-8", errors="replace")
                return json.loads(body)
        except urllib.error.HTTPError as e:
            http_status = e.code
            err_text = ""
            try:
                err_text = e.read().decode("utf-8", "ignore")[:300]
            except Exception:
                pass
            self._log(f"[error] api {method}: HTTP {http_status} - {err_text}")
            return {"ok": False, "error": str(e), "http_status": http_status}
        except Exception as e:
            self._log(f"[error] api {method}: {e}")
            return {"ok": False, "error": str(e)}

    def connect(self) -> bool:
        """Verify token and get bot info."""
        resp = self._api("getMe", timeout=10)
        if resp.get("ok"):
            self._bot_info = resp.get("result", {})
            self._connected = True
            bot_username = self._bot_info.get("username", "unknown")
            self._bot_username = str(bot_username or "").strip()
            self._log(f"[connect] Connected as @{bot_username}")
            return True
        else:
            self._log(f"[connect] Failed: {resp.get('error', 'unknown error')}")
            return False

    def disconnect(self) -> None:
        """Disconnect (no-op for Telegram, just mark as disconnected)."""
        self._connected = False
        self._log("[disconnect] Disconnected")

    def poll(self) -> List[Dict[str, Any]]:
        """
        Long-poll for new messages using getUpdates.

        Returns list of normalized message dicts.
        """
        if not self._connected:
            return []

        resp = self._api(
            "getUpdates",
            {
                "offset": self._offset,
                "timeout": 25,
                # We intentionally ignore edited messages to avoid double-processing commands
                # and accidental duplicate deliveries when a user edits a message.
                "allowed_updates": ["message", "channel_post"],
            },
            timeout=35,
        )

        messages = []
        if resp.get("ok") and isinstance(resp.get("result"), list):
            for update in resp["result"]:
                try:
                    update_id = int(update.get("update_id", 0))
                    self._offset = max(self._offset, update_id + 1)

                    # Extract message from update (ignore edited_message to avoid double-processing).
                    msg = update.get("message") or update.get("channel_post")
                    if not msg:
                        continue

                    # Extract attachments (document/photo/etc.). Text/caption is still required for routing.
                    attachments: List[Dict[str, Any]] = []
                    try:
                        if isinstance(msg.get("document"), dict):
                            doc = msg["document"]
                            attachments.append({
                                "provider": "telegram",
                                "kind": "file",
                                "file_id": str(doc.get("file_id") or ""),
                                "file_unique_id": str(doc.get("file_unique_id") or ""),
                                "file_name": str(doc.get("file_name") or "file"),
                                "mime_type": str(doc.get("mime_type") or ""),
                                "bytes": int(doc.get("file_size") or 0),
                            })
                        elif isinstance(msg.get("photo"), list) and msg.get("photo"):
                            # Use largest size (last item).
                            photo = msg.get("photo")[-1]
                            if isinstance(photo, dict):
                                fid = str(photo.get("file_id") or "")
                                attachments.append({
                                    "provider": "telegram",
                                    "kind": "image",
                                    "file_id": fid,
                                    "file_unique_id": str(photo.get("file_unique_id") or ""),
                                    "file_name": f"photo_{fid}.jpg" if fid else "photo.jpg",
                                    "mime_type": "image/jpeg",
                                    "bytes": int(photo.get("file_size") or 0),
                                })
                    except Exception:
                        attachments = []

                    # Extract text
                    text = msg.get("text") or msg.get("caption") or ""
                    if not text:
                        continue

                    # Extract chat info
                    chat = msg.get("chat", {})
                    chat_id = int(chat.get("id", 0))
                    chat_title = chat.get("title") or chat.get("first_name") or str(chat_id)
                    chat_type = str(chat.get("type") or "").strip()
                    thread_id = 0
                    try:
                        thread_id = int(msg.get("message_thread_id") or 0)
                    except Exception:
                        thread_id = 0

                    # Extract sender info
                    from_user = msg.get("from", {})
                    username = from_user.get("username") or from_user.get("first_name") or "user"

                    # Use "chat_id:msg_id" format for message_id to support reactions
                    raw_msg_id = msg.get("message_id", 0)
                    composite_msg_id = f"{chat_id}:{raw_msg_id}"

                    # Detect bot mention in entities
                    mentioned = False
                    if self._bot_username:
                        raw_text = msg.get("text") or msg.get("caption") or ""
                        for entity in (msg.get("entities") or msg.get("caption_entities") or []):
                            if isinstance(entity, dict) and entity.get("type") == "mention":
                                offset = int(entity.get("offset", 0))
                                length = int(entity.get("length", 0))
                                entity_text = raw_text[offset:offset + length]
                                if entity_text.lower() == f"@{self._bot_username.lower()}":
                                    mentioned = True
                                    break

                    messages.append({
                        "chat_id": str(chat_id),
                        "chat_title": chat_title,
                        "chat_type": chat_type,
                        "routed": bool(chat_type == "private" or mentioned),
                        "thread_id": thread_id,
                        "text": text,
                        "attachments": attachments,
                        "from_user": username,
                        "message_id": composite_msg_id,
                        "update_id": update_id,
                    })
                except Exception as e:
                    self._log(f"[poll] Error parsing update: {e}")
                    continue

        return messages

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        file_id = str(attachment.get("file_id") or "").strip()
        if not file_id:
            raise ValueError("missing telegram file_id")

        meta = self._api("getFile", {"file_id": file_id}, timeout=15)
        if not meta.get("ok"):
            raise ValueError(f"getFile failed: {meta.get('error')}")
        result = meta.get("result") if isinstance(meta.get("result"), dict) else {}
        file_path = str(result.get("file_path") or "").strip()
        if not file_path:
            raise ValueError("missing telegram file_path")

        url = f"https://api.telegram.org/file/bot{self.token}/{file_path}"
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
        if not self._connected:
            return False

        # Telegram caption length is limited; we keep it short.
        safe_caption = self._compose_safe(caption) if caption else ""

        # Rate limit
        self._rate_limiter.wait_and_acquire(str(chat_id))

        boundary = "----cccc" + uuid.uuid4().hex
        url = f"https://api.telegram.org/bot{self.token}/sendDocument"

        try:
            raw = file_path.read_bytes()
        except Exception as e:
            self._log(f"[send_file] read failed: {e}")
            return False

        fields: List[Tuple[str, str]] = [("chat_id", str(chat_id))]
        if safe_caption:
            fields.append(("caption", safe_caption))
        if thread_id:
            try:
                tid = int(thread_id)
            except Exception:
                tid = 0
            if tid > 0:
                fields.append(("message_thread_id", str(tid)))

        body = b""
        for k, v in fields:
            body += (
                f"--{boundary}\r\n"
                f'Content-Disposition: form-data; name="{k}"\r\n\r\n'
                f"{v}\r\n"
            ).encode("utf-8")

        safe_fn = (filename or file_path.name or "file").replace("\\", "_").replace("/", "_")
        body += (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="document"; filename="{safe_fn}"\r\n'
            f"Content-Type: application/octet-stream\r\n\r\n"
        ).encode("utf-8")
        body += raw
        body += f"\r\n--{boundary}--\r\n".encode("utf-8")

        req = urllib.request.Request(url, data=body, method="POST")
        req.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")
        req.add_header("Accept", "application/json")

        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                data = resp.read().decode("utf-8", errors="replace")
                out = json.loads(data)
                return bool(out.get("ok"))
        except Exception as e:
            self._log(f"[send_file] failed: {e}")
            return False

    def send_message(self, chat_id: str, text: str, thread_id: Optional[int] = None) -> bool:
        """
        Send a message to a chat.

        Handles:
        - Rate limiting
        - Message length limits
        - Retry on failure
        """
        if not text:
            return True

        # Ensure message fits Telegram limit
        safe_text = self._compose_safe(text)

        # Rate limit
        self._rate_limiter.wait_and_acquire(str(chat_id))

        # Send with retry
        return self._send_with_retry(str(chat_id), safe_text, thread_id=thread_id)

    def _compose_safe(self, text: str) -> str:
        """Ensure message fits within Telegram limits."""
        # First summarize
        summarized = self.summarize(text, self.max_chars, self.max_lines)

        # Then ensure hard limit
        if len(summarized) > TELEGRAM_MAX_MESSAGE_LENGTH:
            summarized = summarized[: TELEGRAM_MAX_MESSAGE_LENGTH - 1] + "…"

        return summarized

    def _send_with_retry(self, chat_id: str, text: str, thread_id: Optional[int] = None, retries: int = 1) -> bool:
        """Send message with retry on failure."""
        params: Dict[str, Any] = {
            "chat_id": chat_id,
            "text": text,
            "disable_web_page_preview": True,
        }
        if thread_id:
            try:
                tid = int(thread_id)
            except Exception:
                tid = 0
            if tid > 0:
                params["message_thread_id"] = tid

        resp = self._api(
            "sendMessage",
            params,
            timeout=15,
        )

        if resp.get("ok"):
            return True

        # Retry once
        if retries > 0:
            time.sleep(1.0)
            return self._send_with_retry(chat_id, text, thread_id=thread_id, retries=retries - 1)

        self._log(f"[send] Failed to chat {chat_id}: {resp.get('error', 'unknown')}")
        return False

    def get_chat_title(self, chat_id: str) -> str:
        """Get chat title via API."""
        try:
            cid = int(chat_id)
        except Exception:
            cid = chat_id
        resp = self._api("getChat", {"chat_id": cid}, timeout=10)
        if resp.get("ok"):
            chat = resp.get("result", {})
            return chat.get("title") or chat.get("first_name") or str(chat_id)
        return str(chat_id)

    # ===== Typing Indicator (emoji reaction) =====

    TYPING_EMOJI = "👀"

    def _parse_message_id(self, message_id: str) -> Optional[Tuple[str, int]]:
        """
        Parse composite message_id "chat_id:msg_id" into (chat_id, msg_id).

        Returns None if the format is invalid.
        """
        parts = str(message_id).split(":", 1)
        if len(parts) != 2:
            self._log(f"[reaction] Invalid message_id format: {message_id}")
            return None
        try:
            return (parts[0], int(parts[1]))
        except (ValueError, TypeError):
            self._log(f"[reaction] Cannot parse msg_id from: {message_id}")
            return None

    def add_reaction(self, message_id: str, emoji_type: str = "") -> Optional[str]:
        """
        Add an emoji reaction to a message.

        Uses Telegram Bot API setMessageReaction.
        Returns a synthetic reaction_id "chat_id:msg_id:emoji" on success, None on failure.
        """
        if not message_id or not self._connected:
            return None

        parsed = self._parse_message_id(message_id)
        if not parsed:
            return None

        chat_id, msg_id = parsed
        emoji = emoji_type or self.TYPING_EMOJI

        resp = self._api("setMessageReaction", {
            "chat_id": chat_id,
            "message_id": msg_id,
            "reaction": [{"type": "emoji", "emoji": emoji}],
        }, timeout=10)

        if resp.get("ok"):
            reaction_id = f"{chat_id}:{msg_id}:{emoji}"
            self._log(f"[reaction] Added {emoji} to {message_id} -> {reaction_id}")
            return reaction_id

        self._log(f"[reaction] Failed to add {emoji} to {message_id}: {resp.get('error', 'unknown')}")
        return None

    def remove_reaction(self, message_id: str, reaction_id: str) -> bool:
        """
        Remove a previously added emoji reaction.

        Parses reaction_id "chat_id:msg_id:emoji" and calls setMessageReaction
        with an empty list to clear the bot's reaction.
        Returns True on success.
        """
        if not message_id or not reaction_id or not self._connected:
            return False

        # Parse reaction_id to extract chat_id and msg_id
        parts = str(reaction_id).split(":", 2)
        if len(parts) < 2:
            self._log(f"[reaction] Invalid reaction_id format: {reaction_id}")
            return False
        try:
            chat_id = parts[0]
            msg_id = int(parts[1])
        except (ValueError, TypeError):
            self._log(f"[reaction] Cannot parse reaction_id: {reaction_id}")
            return False

        resp = self._api("setMessageReaction", {
            "chat_id": chat_id,
            "message_id": msg_id,
            "reaction": [],
        }, timeout=10)

        if resp.get("ok"):
            self._log(f"[reaction] Removed {reaction_id} from {message_id}")
            return True

        self._log(f"[reaction] Failed to remove {reaction_id} from {message_id}: {resp.get('error', 'unknown')}")
        return False

    def send_chat_action(self, chat_id: str, action: str = "typing") -> bool:
        """Send a chat action indicator (e.g. 'typing') via Telegram API."""
        if not self._connected:
            return False
        resp = self._api("sendChatAction", {"chat_id": chat_id, "action": action}, timeout=10)
        return bool(resp.get("ok"))

    def format_outbound(self, by: str, to: List[str], text: str, is_system: bool = False) -> str:
        """Format message for Telegram display."""
        # Use base implementation but ensure it fits
        formatted = super().format_outbound(by, to, text, is_system)
        return self._compose_safe(formatted)
