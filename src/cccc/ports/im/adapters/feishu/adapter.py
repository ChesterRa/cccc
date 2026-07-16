"""
Lark / Feishu adapter for CCCC IM Bridge.

Uses Feishu Open API with WebSocket long connection for real-time messaging.
Reference: https://open.feishu.cn/document/

Features:
- tenant_access_token auto-refresh (2h expiry)
- WebSocket event subscription (long connection)
- Rate limiting (5 msg/sec per chat)
- File upload/download support
"""

from __future__ import annotations

import threading
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

from ..base import IMAdapter
from .attachments import parse_download_attachment
from .chats import FeishuChatTitleResolver
from .client import FeishuClient
from .domain import FEISHU_DOMAIN, normalize_domain
from .files import prepare_upload_file
from .identity import parse_bot_identity_response
from .mapper import FeishuMessageMapper
from .mentions import FeishuBotIdentity
from .outbound import build_media_message_request, build_text_message_request
from .queue import FeishuMessageQueue
from .rate_limiter import FeishuRateLimiter
from .reactions import (
    build_add_reaction_request,
    build_remove_reaction_request,
    parse_add_reaction_response,
    reaction_succeeded,
)
from .text import compose_safe_text
from .webhook import normalize_webhook_event
from .ws import FeishuWsListener

# Feishu API limits
DEFAULT_MAX_CHARS = 30720
DEFAULT_MAX_LINES = 1024

class FeishuAdapter(IMAdapter):
    """
    Feishu adapter using WebSocket for inbound and REST API for outbound.
    """

    platform = "feishu"
    capabilities = {
        "text_in": "yes",
        "text_out": "yes",
        "files_in": "partial",
        "files_out": "yes",
        "threads": "yes",
        "reactions": "yes",
        "typing": "no",
        "streaming": "no",
        "voice_in": "no",
        "markdown": "partial",
    }
    capability_notes = {
        "threads": "root_id is preserved for replies when present",
        "reactions": "used for processing indicators where available",
    }

    def __init__(
        self,
        app_id: str,
        app_secret: str,
        domain: str = FEISHU_DOMAIN,
        bot_name: str = "cccc",
        log_path: Optional[Path] = None,
        max_chars: int = DEFAULT_MAX_CHARS,
        max_lines: int = DEFAULT_MAX_LINES,
    ):
        self.app_id = app_id
        self.app_secret = app_secret
        self.domain = normalize_domain(domain)
        self.api_base = f"{self.domain}/open-apis"
        self.log_path = log_path
        self.max_chars = max_chars
        self.max_lines = max_lines

        self._client = FeishuClient(
            app_id=self.app_id,
            app_secret=self.app_secret,
            api_base=self.api_base,
            log_fn=self._log,
        )

        self._messages = FeishuMessageQueue()

        # Rate limiter
        self._rate_limiter = FeishuRateLimiter(max_per_second=5.0)

        # Connection state
        self._connected = False
        self._ws_thread: Optional[threading.Thread] = None
        self._ws_running = False
        self._ws_connect_error: Optional[str] = None  # Error message if WS connection fails
        self._ws_started = threading.Event()  # Signals when WS connection attempt completes
        self._ws_listener: Optional[FeishuWsListener] = None

        self._chat_titles = FeishuChatTitleResolver(self._api)
        self._bot_open_id: str = ""
        self._bot_user_id: str = ""
        self._bot_name: str = str(bot_name or "").strip()

    def _bot_identity(self) -> FeishuBotIdentity:
        return FeishuBotIdentity.from_values(
            open_id=self._bot_open_id,
            user_id=self._bot_user_id,
            name=self._bot_name,
        )

    def _log(self, msg: str) -> None:
        """Append to log file if configured."""
        if self.log_path:
            try:
                self.log_path.parent.mkdir(parents=True, exist_ok=True)
                with self.log_path.open("a", encoding="utf-8") as f:
                    ts = time.strftime("%Y-%m-%d %H:%M:%S")
                    f.write(f"{ts} [feishu] {msg}\n")
            except Exception:
                pass

    def _get_token(self) -> str:
        """Get valid tenant_access_token, refreshing if needed."""
        return self._client.get_token()

    def _refresh_token(self) -> bool:
        """Refresh tenant_access_token."""
        return self._client.refresh_token()

    def _api(
        self,
        method: str,
        endpoint: str,
        body: Optional[Dict[str, Any]] = None,
        timeout: int = 15,
    ) -> Dict[str, Any]:
        """
        Call Feishu API.

        Args:
            method: HTTP method (GET, POST, etc.)
            endpoint: API endpoint (e.g., /im/v1/messages)
            body: Request body (for POST/PUT)
            timeout: Request timeout in seconds
        """
        return self._client.api(method, endpoint, body, timeout)

    def _load_bot_identity(self) -> bool:
        """Cache bot identity so group routing only accepts real bot mentions."""
        resp = self._api("GET", "/bot/v3/info")
        result = parse_bot_identity_response(resp, configured_name=self._bot_name)
        if not result.ok:
            self._log(f"[bot] Failed to load bot identity: {result.error}")
            return False

        self._bot_open_id = result.identity.open_id
        self._bot_user_id = result.identity.user_id
        self._bot_name = result.identity.name
        self._log("[bot] Loaded bot identity")
        return True

    def connect(self) -> bool:
        """
        Initialize connection to Feishu.

        1. Verify credentials by getting token
        2. Start WebSocket event listener
        """
        # Clear message queue on reconnect to avoid duplicate messages
        self._messages.clear()

        # Disable all proxies BEFORE importing lark SDK
        self._disable_proxies()

        # Inbound requires the official SDK (lark-oapi) for long connection messaging.
        try:
            import lark_oapi as lark  # type: ignore
            from lark_oapi.ws import Client as WsClient  # type: ignore
        except Exception:
            import sys
            self._log(f"[error] Missing dependency: lark-oapi. Install: {sys.executable} -m pip install lark-oapi")
            return False

        # Cache SDK handles for the background thread.
        self._lark = lark
        self._WsClient = WsClient

        # Get initial token
        if not self._refresh_token():
            self._log("[connect] Failed to get token")
            return False
        if not self._load_bot_identity():
            if self._bot_name:
                self._log(f"[connect] Bot identity unavailable; matching mentions by configured name @{self._bot_name}")
            else:
                self._log("[connect] Failed to load bot identity")
                return False

        # Start WebSocket listener for events
        self._ws_connect_error = None
        self._ws_started.clear()
        self._start_ws_listener()

        # Wait for WebSocket connection attempt to complete (with timeout)
        # This ensures we detect early connection failures (e.g., proxy issues)
        if not self._ws_started.wait(timeout=5.0):
            self._log("[connect] Timeout waiting for WebSocket to start")
            return False
        if self._ws_listener:
            self._ws_connect_error = self._ws_listener.connect_error

        # Check if connection failed
        if self._ws_connect_error:
            self._log(f"[connect] WebSocket connection failed: {self._ws_connect_error}")
            return False

        # Give SDK a moment to establish connection, then verify thread is still alive
        time.sleep(0.5)
        if self._ws_thread and not self._ws_thread.is_alive():
            self._log("[connect] WebSocket thread died unexpectedly")
            return False

        self._connected = True
        self._log(f"[connect] Connected (domain={self.domain}, app_id={self.app_id[:8]}...)")
        return True

    def _start_ws_listener(self) -> None:
        """
        Start WebSocket listener using Feishu official SDK.

        Uses lark_oapi.ws.Client for reliable long connection.
        """
        if self._ws_listener and self._ws_listener.is_alive():
            return

        self._ws_running = True
        lark = getattr(self, "_lark", None)
        WsClient = getattr(self, "_WsClient", None)
        if lark is None or WsClient is None:
            self._log("[ws] Missing SDK handles; connect() should have returned False.")
            self._ws_connect_error = "missing SDK handles"
            self._ws_started.set()
            return

        self._ws_listener = FeishuWsListener(
            app_id=self.app_id,
            app_secret=self.app_secret,
            domain=self.domain,
            lark=lark,
            ws_client_cls=WsClient,
            log_fn=self._log,
            enqueue_fn=self._enqueue_message,
        )
        self._ws_listener.start()
        self._ws_thread = self._ws_listener.thread
        self._ws_started = self._ws_listener.started
        self._ws_connect_error = self._ws_listener.connect_error

    def disconnect(self) -> None:
        """Disconnect from Feishu."""
        self._connected = False
        self._ws_running = False

        if self._ws_listener:
            self._ws_listener.stop()
            self._ws_thread = self._ws_listener.thread
        self._log("[disconnect] Disconnected")

    def poll(self) -> List[Dict[str, Any]]:
        """
        Poll for new messages.

        Returns queued messages from WebSocket listener.
        """
        if not self._connected:
            return []

        return self._messages.drain()

    def _enqueue_message(self, event: Dict[str, Any]) -> None:
        """
        Process incoming event and enqueue normalized message.

        Called by WebSocket listener when receiving im.message.receive_v1 event.
        """
        try:
            normalized = self._message_mapper().map_event(event)
            if normalized is None:
                return
            self._messages.append(normalized)

        except Exception as e:
            self._log(f"[enqueue] Error: {e}")

    def _message_mapper(self) -> FeishuMessageMapper:
        return FeishuMessageMapper(
            bot_identity=self._bot_identity(),
            chat_title_lookup=self._chat_titles.get,
        )

    def send_message(
        self,
        chat_id: str,
        text: str,
        thread_id: Optional[int] = None,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        """
        Send a text message to a chat.

        Args:
            chat_id: Feishu chat_id (oc_xxx)
            text: Message text
            thread_id: Optional root_id for threading
        """
        _ = mention_user_ids
        if not text:
            return True

        if not self._connected:
            return False

        # Ensure message fits limit
        safe_text = self._compose_safe(text)

        # Rate limit
        self._rate_limiter.wait_and_acquire(chat_id)

        endpoint, body = build_text_message_request(chat_id, safe_text, thread_id=thread_id)

        resp = self._api("POST", endpoint, body)

        if resp.get("code") == 0:
            return True

        self._log(f"[send] Failed to chat {chat_id}: {resp.get('msg', 'unknown')}")
        return False

    def _compose_safe(self, text: str) -> str:
        """Ensure message fits within Feishu limits."""
        return compose_safe_text(
            text,
            max_chars=self.max_chars,
            max_lines=self.max_lines,
            summarize_fn=self.summarize,
        )

    def get_chat_title(self, chat_id: str) -> str:
        """Get chat title via API."""
        return self._chat_titles.get(chat_id)

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        """Download an attachment from Feishu."""
        parsed = parse_download_attachment(attachment)
        return self._client.download_attachment(parsed.kind, parsed.key)

    def send_file(
        self,
        chat_id: str,
        *,
        file_path: Path,
        filename: str,
        caption: str = "",
        thread_id: Optional[int] = None,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        """Send a file to a chat."""
        _ = mention_user_ids
        if not self._connected:
            return False

        token = self._get_token()
        if not token:
            return False

        # Rate limit
        self._rate_limiter.wait_and_acquire(chat_id)

        try:
            raw = file_path.read_bytes()
        except Exception as e:
            self._log(f"[send_file] Read failed: {e}")
            return False

        upload_file = prepare_upload_file(file_path, filename)

        media = self._client.upload_media(
            raw,
            upload_file.filename,
            upload_file.mime_type,
            is_image=upload_file.is_image,
        )
        if not media:
            return False
        msg_type, media_key_name, media_key = media

        endpoint, msg_body = build_media_message_request(
            chat_id,
            msg_type=msg_type,
            media_key_name=media_key_name,
            media_key=media_key,
            thread_id=thread_id,
        )

        resp = self._api("POST", endpoint, msg_body)

        if resp.get("code") == 0:
            # Send caption as separate message if provided
            if caption:
                self.send_message(chat_id, caption, thread_id)
            return True

        self._log(f"[send_file] Send failed: {resp.get('msg', 'unknown')}")
        return False

    # ===== Typing Indicator (emoji reaction) =====

    def add_reaction(self, message_id: str, emoji_type: str = "") -> Optional[str]:
        """
        Add an emoji reaction to a message.

        Feishu does not have a native typing indicator, so we simulate one
        by reacting to the user's message with an emoji.

        Returns reaction_id on success (for later removal), None on failure.
        """
        if not message_id or not self._connected:
            return None

        endpoint, body, emoji = build_add_reaction_request(message_id, emoji_type)
        resp = self._api("POST", endpoint, body)
        reaction_id = parse_add_reaction_response(resp)
        if reaction_id:
            self._log(f"[reaction] Added {emoji} to {message_id} -> {reaction_id}")
            return reaction_id

        self._log(f"[reaction] Failed to add {emoji} to {message_id}: {resp.get('msg', 'unknown')}")
        return None

    def remove_reaction(self, message_id: str, reaction_id: str) -> bool:
        """
        Remove a previously added emoji reaction.

        Returns True on success.
        """
        if not message_id or not reaction_id or not self._connected:
            return False

        resp = self._api("DELETE", build_remove_reaction_request(message_id, reaction_id))
        if reaction_succeeded(resp):
            self._log(f"[reaction] Removed {reaction_id} from {message_id}")
            return True

        self._log(f"[reaction] Failed to remove {reaction_id} from {message_id}: {resp.get('msg', 'unknown')}")
        return False

    def format_outbound(self, by: str, to: List[str], text: str, is_system: bool = False) -> str:
        """Format message for Feishu display."""
        formatted = super().format_outbound(by, to, text, is_system)
        return self._compose_safe(formatted)

    # ===== WebSocket Event Handling (for webhook integration) =====

    def handle_webhook_event(self, event: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        """
        Handle incoming webhook event from Feishu.

        This method should be called by an external HTTP server
        when receiving events from Feishu webhook.

        Returns challenge response if needed, None otherwise.
        """
        result = normalize_webhook_event(event)
        if result.challenge_response is not None:
            return result.challenge_response
        if result.message_event is not None:
            self._enqueue_message(result.message_event)
        return None
