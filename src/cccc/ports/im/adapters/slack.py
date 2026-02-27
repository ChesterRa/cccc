"""
Slack adapter for CCCC IM Bridge.

Uses:
- Bot Token (xoxb-): Web API for sending messages
- App Token (xapp-): Socket Mode for receiving messages (optional)

If only bot_token is provided, the adapter can send but not receive.
"""

from __future__ import annotations

import json
import os
import re
import threading
import time
import urllib.request
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from .base import IMAdapter

# Slack limits
SLACK_MAX_MESSAGE_LENGTH = 4000  # Slack blocks limit, text limit is higher but 4000 is safe
DEFAULT_MAX_CHARS = 4000
DEFAULT_MAX_LINES = 64


class SlackAdapter(IMAdapter):
    """
    Slack adapter using Socket Mode for inbound and Web API for outbound.
    """

    platform = "slack"

    def __init__(
        self,
        bot_token: str,
        app_token: Optional[str] = None,
        log_path: Optional[Path] = None,
        max_chars: int = DEFAULT_MAX_CHARS,
        max_lines: int = DEFAULT_MAX_LINES,
    ):
        self.bot_token = bot_token
        self.app_token = app_token
        self.log_path = log_path
        self.max_chars = max_chars
        self.max_lines = max_lines

        self._connected = False
        self._bot_user_id: Optional[str] = None
        self._web_client: Any = None
        self._socket_client: Any = None
        self._message_queue: List[Dict[str, Any]] = []
        self._queue_lock = threading.Lock()

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
        Initialize Slack clients.

        Requires slack_sdk package.
        """
        # Clear message queue on reconnect to avoid duplicate messages
        with self._queue_lock:
            self._message_queue.clear()

        try:
            from slack_sdk import WebClient
        except ImportError:
            self._log("[error] slack_sdk not installed. Run: pip install slack_sdk")
            return False

        # Initialize Web client (required for sending)
        self._web_client = WebClient(token=self.bot_token)

        # Verify token and get bot user ID
        try:
            auth = self._web_client.auth_test()
            self._bot_user_id = auth.get("user_id", "")
            bot_name = auth.get("user", "unknown")
            self._log(f"[connect] Connected as @{bot_name} (user_id={self._bot_user_id})")
        except Exception as e:
            self._log(f"[error] auth_test failed: {e}")
            return False

        # Initialize Socket Mode client if app_token provided
        if self.app_token:
            try:
                from slack_sdk.socket_mode import SocketModeClient
                from slack_sdk.socket_mode.response import SocketModeResponse
                from slack_sdk.socket_mode.request import SocketModeRequest

                self._socket_client = SocketModeClient(
                    app_token=self.app_token,
                    web_client=self._web_client,
                )

                # Register event handler
                def handle_event(client: SocketModeClient, req: SocketModeRequest):
                    self._handle_socket_event(client, req)

                self._socket_client.socket_mode_request_listeners.append(handle_event)

                # Start socket mode in background thread
                self._socket_client.connect()
                self._log("[connect] Socket Mode connected (inbound enabled)")
            except ImportError:
                self._log("[warn] Socket Mode requires slack_sdk[socket-mode]. Inbound disabled.")
            except Exception as e:
                self._log(f"[warn] Socket Mode connection failed: {e}. Inbound disabled.")
        else:
            self._log("[info] No app_token provided. Inbound disabled (send-only mode).")

        self._connected = True
        return True

    def _handle_socket_event(self, client: Any, req: Any) -> None:
        """Handle incoming Socket Mode events."""
        try:
            from slack_sdk.socket_mode.response import SocketModeResponse

            # Acknowledge immediately to avoid timeout
            client.send_socket_mode_response(SocketModeResponse(envelope_id=req.envelope_id))

            if req.type != "events_api":
                return

            event = req.payload.get("event", {})
            event_type = event.get("type", "")

            # Only handle message events
            if event_type != "message":
                return

            # Skip subtypes (edits, bot messages, etc.) except file_share
            subtype = event.get("subtype", "")
            if subtype and subtype != "file_share":
                return

            # Skip messages from self
            user = event.get("user", "")
            if user == self._bot_user_id:
                return

            text = event.get("text", "")
            channel = event.get("channel", "")
            channel_type = str(event.get("channel_type") or "").strip().lower()

            if not text or not channel:
                return

            attachments: List[Dict[str, Any]] = []
            try:
                files = event.get("files")
                if isinstance(files, list):
                    for f in files:
                        if not isinstance(f, dict):
                            continue
                        attachments.append({
                            "provider": "slack",
                            "kind": "file",
                            "id": str(f.get("id") or ""),
                            "file_name": str(f.get("name") or "file"),
                            "mime_type": str(f.get("mimetype") or ""),
                            "bytes": int(f.get("size") or 0),
                            "url": str(f.get("url_private_download") or f.get("url_private") or ""),
                        })
            except Exception:
                attachments = []

            chat_type = "private" if channel_type in ("im", "mpim") else "channel"
            if not channel_type:
                # Fallback: Slack channel IDs are prefixed by type: D=im, C=public channel, G=private channel/mpim.
                chat_type = "private" if str(channel).startswith("D") else "channel"

            mentioned = False
            if self._bot_user_id:
                mentioned = f"<@{self._bot_user_id}>" in text

            # In non-private channels, require an explicit bot mention to route messages.
            if chat_type != "private" and not mentioned:
                return

            # Strip self-mention from beginning (keeps in-text mentions intact)
            if self._bot_user_id:
                text = re.sub(rf"^\s*(?:<@{re.escape(self._bot_user_id)}>\s*)+", "", text)

            # Queue the message
            try:
                event_ts = float(event.get("ts") or 0.0)
            except Exception:
                event_ts = 0.0
            with self._queue_lock:
                self._message_queue.append({
                    "chat_id": channel,
                    "chat_title": channel,  # Will be resolved later if needed
                    "chat_type": chat_type,
                    "routed": bool(chat_type == "private" or mentioned),
                    "text": text.strip(),
                    "attachments": attachments,
                    "from_user": user,
                    "message_id": event.get("ts", ""),
                    "timestamp": event_ts,
                })

            self._log(f"[inbound] channel={channel} user={user} text={text[:50]}...")

        except Exception as e:
            self._log(f"[error] handle_socket_event: {e}")

    def disconnect(self) -> None:
        """Disconnect from Slack."""
        if self._socket_client:
            try:
                self._socket_client.disconnect()
            except Exception:
                pass
        self._connected = False
        self._log("[disconnect] Disconnected")

    def poll(self) -> List[Dict[str, Any]]:
        """
        Return queued messages from Socket Mode.

        Messages are queued by the socket event handler.
        """
        if not self._connected:
            return []

        with self._queue_lock:
            messages = list(self._message_queue)
            self._message_queue.clear()

        return messages

    def send_message(self, chat_id: str, text: str, thread_id: Optional[int] = None) -> bool:
        """
        Send a message to a Slack channel.

        chat_id is actually a channel ID string in Slack.
        """
        _ = thread_id  # Slack threads not wired yet (future work).
        if not self._connected or not self._web_client:
            return False

        if not text:
            return True

        # Ensure message fits Slack limit
        safe_text = self._compose_safe(text)

        try:
            self._web_client.chat_postMessage(
                channel=str(chat_id),
                text=safe_text,
            )
            return True
        except Exception as e:
            self._log(f"[error] send_message to {chat_id}: {e}")
            return False

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        url = str(attachment.get("url") or "").strip()
        if not url:
            raise ValueError("missing slack attachment url")
        req = urllib.request.Request(url, method="GET")
        req.add_header("Authorization", f"Bearer {self.bot_token}")
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
        _ = thread_id  # Slack threads not wired yet (future work).
        if not self._connected or not self._web_client:
            return False

        try:
            with file_path.open("rb") as f:
                if hasattr(self._web_client, "files_upload_v2"):
                    self._web_client.files_upload_v2(
                        channel=str(chat_id),
                        file=f,
                        filename=str(filename or file_path.name or "file"),
                        initial_comment=str(caption or ""),
                    )
                else:
                    # Backward-compat for older slack_sdk.
                    self._web_client.files_upload(
                        channels=str(chat_id),
                        file=f,
                        filename=str(filename or file_path.name or "file"),
                        initial_comment=str(caption or ""),
                    )
            return True
        except Exception as e:
            self._log(f"[error] send_file to {chat_id}: {e}")
            return False

    def _compose_safe(self, text: str) -> str:
        """Ensure message fits within Slack limits."""
        summarized = self.summarize(text, self.max_chars, self.max_lines)

        if len(summarized) > SLACK_MAX_MESSAGE_LENGTH:
            summarized = summarized[: SLACK_MAX_MESSAGE_LENGTH - 1] + "…"

        return summarized

    def get_chat_title(self, chat_id: str) -> str:
        """Get channel name via API."""
        if not self._web_client:
            return str(chat_id)

        try:
            resp = self._web_client.conversations_info(channel=str(chat_id))
            channel = resp.get("channel", {})
            return channel.get("name", str(chat_id))
        except Exception:
            return str(chat_id)

    def format_outbound(self, by: str, to: List[str], text: str, is_system: bool = False) -> str:
        """Format message for Slack display."""
        formatted = super().format_outbound(by, to, text, is_system)
        return self._compose_safe(formatted)
