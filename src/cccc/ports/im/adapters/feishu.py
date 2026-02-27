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

import json
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path
from typing import Any, Dict, List, Optional

from .base import IMAdapter

# Feishu API limits
FEISHU_MAX_MESSAGE_LENGTH = 30720  # 30 KB (Safe limit for Posts, well within 150 KB for Text)
DEFAULT_MAX_CHARS = 30720
DEFAULT_MAX_LINES = 1024

# API domains:
# - Feishu (CN): https://open.feishu.cn
# - Lark (Global): https://open.larkoffice.com
FEISHU_DOMAIN = "https://open.feishu.cn"
LARK_DOMAIN = "https://open.larkoffice.com"


def _normalize_domain(domain: str) -> str:
    d = str(domain or "").strip()
    if not d:
        return FEISHU_DOMAIN
    d = d.rstrip("/")
    if d.endswith("/open-apis"):
        d = d[: -len("/open-apis")]
        d = d.rstrip("/")
    if not (d.startswith("http://") or d.startswith("https://")):
        d = "https://" + d
    return d


class RateLimiter:
    """
    Rate limiter for Feishu API.

    Feishu limits:
    - Same chat: ~5 msg/sec
    - Total: ~100 msg/sec
    """

    def __init__(self, max_per_second: float = 5.0):
        self.min_interval = 1.0 / max_per_second
        self.last_send: Dict[str, float] = {}
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


class FeishuAdapter(IMAdapter):
    """
    Feishu adapter using WebSocket for inbound and REST API for outbound.
    """

    platform = "feishu"

    def __init__(
        self,
        app_id: str,
        app_secret: str,
        domain: str = FEISHU_DOMAIN,
        log_path: Optional[Path] = None,
        max_chars: int = DEFAULT_MAX_CHARS,
        max_lines: int = DEFAULT_MAX_LINES,
    ):
        self.app_id = app_id
        self.app_secret = app_secret
        self.domain = _normalize_domain(domain)
        self.api_base = f"{self.domain}/open-apis"
        self.log_path = log_path
        self.max_chars = max_chars
        self.max_lines = max_lines

        # Token management
        self._token: str = ""
        self._token_expires: float = 0
        self._token_lock = threading.Lock()

        # Message queue (thread-safe)
        self._message_queue: List[Dict[str, Any]] = []
        self._queue_lock = threading.Lock()

        # Rate limiter
        self._rate_limiter = RateLimiter(max_per_second=5.0)

        # Connection state
        self._connected = False
        self._ws_thread: Optional[threading.Thread] = None
        self._ws_running = False
        self._ws_connect_error: Optional[str] = None  # Error message if WS connection fails
        self._ws_started = threading.Event()  # Signals when WS connection attempt completes

        # Cache for chat titles
        self._chat_title_cache: Dict[str, str] = {}

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
        with self._token_lock:
            now = time.time()
            # Refresh 5 minutes before expiry
            if self._token and now < self._token_expires - 300:
                return self._token

            # Refresh token
            if self._refresh_token():
                return self._token
            return ""

    def _refresh_token(self) -> bool:
        """
        Refresh tenant_access_token.
        Token expires in 2 hours.
        """
        url = f"{self.api_base}/auth/v3/tenant_access_token/internal"
        data = json.dumps({
            "app_id": self.app_id,
            "app_secret": self.app_secret,
        }, ensure_ascii=False).encode("utf-8")

        req = urllib.request.Request(url, data=data, method="POST")
        req.add_header("Content-Type", "application/json; charset=utf-8")

        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                body = resp.read().decode("utf-8", errors="replace")
                result = json.loads(body)

                if result.get("code") == 0:
                    self._token = result.get("tenant_access_token", "")
                    expire = int(result.get("expire", 7200))
                    self._token_expires = time.time() + expire
                    self._log(f"[token] Refreshed, expires in {expire}s")
                    return True
                else:
                    self._log(f"[token] Failed: {result.get('msg', 'unknown')}")
                    return False
        except Exception as e:
            self._log(f"[token] Error: {e}")
            return False

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
        token = self._get_token()
        if not token:
            return {"code": -1, "msg": "No valid token"}

        url = f"{self.api_base}{endpoint}"

        if method == "GET" and body:
            # Convert body to query params for GET
            query = urllib.parse.urlencode(body)
            url = f"{url}?{query}"
            data = None
        else:
            data = json.dumps(body or {}, ensure_ascii=False).encode("utf-8") if body else None

        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("Authorization", f"Bearer {token}")
        req.add_header("Content-Type", "application/json; charset=utf-8")
        req.add_header("Accept", "application/json")

        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                result_body = resp.read().decode("utf-8", errors="replace")
                return json.loads(result_body)
        except urllib.error.HTTPError as e:
            http_status = e.code
            err_text = ""
            try:
                err_text = e.read().decode("utf-8", "ignore")[:300]
            except Exception:
                pass
            self._log(f"[api] {method} {endpoint}: HTTP {http_status} - {err_text}")
            return {"code": http_status, "msg": str(e), "error": err_text}
        except Exception as e:
            self._log(f"[api] {method} {endpoint}: {e}")
            return {"code": -1, "msg": str(e)}

    def connect(self) -> bool:
        """
        Initialize connection to Feishu.

        1. Verify credentials by getting token
        2. Start WebSocket event listener
        """
        # Clear message queue on reconnect to avoid duplicate messages
        with self._queue_lock:
            self._message_queue.clear()

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

        # Start WebSocket listener for events
        self._ws_connect_error = None
        self._ws_started.clear()
        self._start_ws_listener()

        # Wait for WebSocket connection attempt to complete (with timeout)
        # This ensures we detect early connection failures (e.g., proxy issues)
        if not self._ws_started.wait(timeout=5.0):
            self._log("[connect] Timeout waiting for WebSocket to start")
            return False

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
        if self._ws_thread and self._ws_thread.is_alive():
            return

        self._ws_running = True

        def ws_loop():
            """WebSocket event loop using official Feishu SDK."""
            self._log("[ws] Event listener starting...")

            # SDK is imported and cached in connect().
            lark = getattr(self, "_lark", None)
            WsClient = getattr(self, "_WsClient", None)
            if lark is None or WsClient is None:
                self._log("[ws] Missing SDK handles; connect() should have returned False.")
                return

            # Event handler function - SDK passes single data argument
            def on_p2_im_message_receive_v1(data):
                """Handle incoming im.message.receive_v1 event from SDK."""
                try:
                    # Note: keep logs minimal; message payload may contain sensitive content.
                    self._log(f"[ws] Received event: {type(data).__name__}")

                    # SDK provides structured event object
                    event_data = {
                        "message": {},
                        "sender": {"sender_id": {}}
                    }

                    # Extract message info from SDK event object
                    if hasattr(data, 'event') and data.event:
                        event_obj = data.event

                        if hasattr(event_obj, 'message') and event_obj.message:
                            msg = event_obj.message
                            # Serialize SDK MentionEvent objects to dicts
                            mentions_raw = getattr(msg, 'mentions', None)
                            mentions_list = None
                            if mentions_raw:
                                mentions_list = []
                                for m in mentions_raw:
                                    md = {
                                        "key": getattr(m, 'key', ''),
                                        "name": getattr(m, 'name', ''),
                                    }
                                    if hasattr(m, 'id') and m.id:
                                        md["id"] = {
                                            "open_id": getattr(m.id, 'open_id', ''),
                                            "user_id": getattr(m.id, 'user_id', ''),
                                        }
                                    mentions_list.append(md)
                            event_data["message"] = {
                                "message_id": getattr(msg, 'message_id', '') or '',
                                "chat_id": getattr(msg, 'chat_id', '') or '',
                                "chat_type": getattr(msg, 'chat_type', '') or '',
                                "message_type": getattr(msg, 'message_type', '') or '',
                                "content": getattr(msg, 'content', '{}') or '{}',
                                "root_id": getattr(msg, 'root_id', '') or '',
                                "create_time": getattr(msg, 'create_time', '') or '',
                                "mentions": mentions_list,
                            }
                            self._log(f"[ws] Chat: {event_data['message']['chat_id']} type={event_data['message']['message_type']}")

                        if hasattr(event_obj, 'sender') and event_obj.sender:
                            sender = event_obj.sender
                            event_data["sender"]["sender_type"] = getattr(sender, 'sender_type', '') or ''

                            if hasattr(sender, 'sender_id') and sender.sender_id:
                                sid = sender.sender_id
                                event_data["sender"]["sender_id"] = {
                                    "open_id": getattr(sid, 'open_id', '') or '',
                                    "user_id": getattr(sid, 'user_id', '') or '',
                                }
                            # Sender IDs may be sensitive; avoid logging raw IDs.

                    # Wrap in expected format for _enqueue_message
                    full_event = {
                        "header": {"event_type": "im.message.receive_v1"},
                        "event": event_data
                    }
                    self._enqueue_message(full_event)
                    self._log("[ws] Message enqueued")

                except Exception as e:
                    self._log(f"[ws] Event handler error: {e}")
                    import traceback
                    self._log(f"[ws] Traceback: {traceback.format_exc()}")

            try:
                # Create WebSocket client with event handler
                event_handler = lark.EventDispatcherHandler.builder("", "") \
                    .register_p2_im_message_receive_v1(on_p2_im_message_receive_v1) \
                    .build()

                self._ws_client = WsClient(
                    app_id=self.app_id,
                    app_secret=self.app_secret,
                    event_handler=event_handler,
                    log_level=lark.LogLevel.INFO,
                    domain=self.domain,
                )

                self._log("[ws] Starting Feishu SDK WebSocket client...")

                # Signal that we're about to start (no error yet)
                self._ws_connect_error = None
                self._ws_started.set()

                # start() is blocking, runs until stopped
                self._ws_client.start()

            except Exception as e:
                error_msg = str(e)
                self._log(f"[ws] SDK error: {error_msg}")
                import traceback
                self._log(f"[ws] Traceback: {traceback.format_exc()}")
                # Record the error so connect() can detect it
                self._ws_connect_error = error_msg
                self._ws_started.set()  # Signal completion (with error)

            self._log("[ws] Event listener stopped")

        self._ws_thread = threading.Thread(target=ws_loop, daemon=True)
        self._ws_thread.start()

    def disconnect(self) -> None:
        """Disconnect from Feishu."""
        self._connected = False
        self._ws_running = False

        # Stop SDK client if running
        if hasattr(self, '_ws_client') and self._ws_client:
            try:
                self._ws_client.stop()
            except Exception:
                pass

        if self._ws_thread:
            self._ws_thread.join(timeout=2.0)
        self._log("[disconnect] Disconnected")

    def poll(self) -> List[Dict[str, Any]]:
        """
        Poll for new messages.

        Returns queued messages from WebSocket listener.
        """
        if not self._connected:
            return []

        with self._queue_lock:
            messages = self._message_queue.copy()
            self._message_queue.clear()

        return messages

    def _enqueue_message(self, event: Dict[str, Any]) -> None:
        """
        Process incoming event and enqueue normalized message.

        Called by WebSocket listener when receiving im.message.receive_v1 event.
        """
        try:
            header = event.get("header", {})
            event_type = header.get("event_type", "")

            if event_type != "im.message.receive_v1":
                return

            payload = event.get("event", {})
            message = payload.get("message", {})
            sender = payload.get("sender", {})

            # Extract message content
            msg_type = message.get("message_type", "")
            content_str = message.get("content", "{}")

            try:
                content = json.loads(content_str)
            except Exception:
                content = {}

            # Extract text based on message type
            if msg_type == "text":
                text = content.get("text", "")
            elif msg_type == "post":
                # Rich text - extract plain text
                text = self._extract_post_text(content)
            elif msg_type == "image":
                text = "[image]"
            elif msg_type == "file":
                text = f"[file: {content.get('file_name', 'unknown')}]"
            else:
                text = f"[{msg_type}]"

            # Replace Feishu mention placeholders (@_user_1 etc.) with display names.
            msg_mentions = message.get("mentions") or []
            if isinstance(msg_mentions, list):
                for m in msg_mentions:
                    key = m.get("key", "")    # e.g. "@_user_1"
                    name = m.get("name", "")  # e.g. "张三"
                    if key and name:
                        text = text.replace(key, f"@{name}")

            if not text.strip():
                return

            # Build attachments list
            attachments: List[Dict[str, Any]] = []
            if msg_type == "image":
                attachments.append({
                    "provider": "feishu",
                    "kind": "image",
                    "image_key": content.get("image_key", ""),
                    "file_name": "image.png",
                })
            elif msg_type == "file":
                attachments.append({
                    "provider": "feishu",
                    "kind": "file",
                    "file_key": content.get("file_key", ""),
                    "file_name": content.get("file_name", "file"),
                })

            # Get sender info
            sender_id = sender.get("sender_id", {})
            open_id = sender_id.get("open_id", "")
            sender_type = sender.get("sender_type", "")

            # Skip bot's own messages
            if sender_type == "app":
                return

            # Normalize message
            chat_id = message.get("chat_id", "")
            chat_type = message.get("chat_type", "")  # p2p, group

            # Detect bot mention: in standard Feishu event subscription,
            # group messages are only delivered when the bot is @mentioned.
            mentions = message.get("mentions") or []
            is_at_bot = isinstance(mentions, list) and len(mentions) > 0

            normalized = {
                "chat_id": chat_id,
                "chat_title": self._get_chat_title_cached(chat_id),
                "chat_type": chat_type,
                "routed": bool(chat_type == "p2p" or is_at_bot),
                "thread_id": message.get("root_id", 0) or 0,
                "text": text,
                "attachments": attachments,
                "from_user": open_id,
                "message_id": message.get("message_id", ""),
                "timestamp": self._parse_message_time(message.get("create_time")),
            }

            with self._queue_lock:
                self._message_queue.append(normalized)

        except Exception as e:
            self._log(f"[enqueue] Error: {e}")

    def _extract_post_text(self, content: Dict[str, Any]) -> str:
        """Extract plain text from rich text (post) content."""
        texts = []
        try:
            # Post content structure: {"title": "...", "content": [[{tag, ...}]]}
            title = content.get("title", "")
            if title:
                texts.append(title)

            for line in content.get("content", []):
                for elem in line:
                    tag = elem.get("tag", "")
                    if tag == "text":
                        texts.append(elem.get("text", ""))
                    elif tag == "a":
                        texts.append(elem.get("text", elem.get("href", "")))
                    elif tag == "at":
                        texts.append(f"@{elem.get('user_name', 'user')}")
        except Exception:
            pass
        return " ".join(texts)

    def _parse_message_time(self, raw: Any) -> float:
        """Parse Feishu create_time into epoch seconds (supports ms)."""
        try:
            ts = float(raw or 0.0)
        except Exception:
            return 0.0
        if ts <= 0:
            return 0.0
        if ts > 1e11:
            ts = ts / 1000.0
        return ts

    def _get_chat_title_cached(self, chat_id: str) -> str:
        """Get chat title with caching."""
        if chat_id in self._chat_title_cache:
            return self._chat_title_cache[chat_id]

        title = self.get_chat_title(chat_id)
        self._chat_title_cache[chat_id] = title
        return title

    def send_message(self, chat_id: str, text: str, thread_id: Optional[int] = None) -> bool:
        """
        Send a text message to a chat.

        Args:
            chat_id: Feishu chat_id (oc_xxx)
            text: Message text
            thread_id: Optional root_id for threading
        """
        if not text:
            return True

        if not self._connected:
            return False

        # Ensure message fits limit
        safe_text = self._compose_safe(text)

        # Rate limit
        self._rate_limiter.wait_and_acquire(chat_id)

        # Build message content
        content = json.dumps({"text": safe_text}, ensure_ascii=False)

        params: Dict[str, Any] = {
            "receive_id_type": "chat_id",
        }

        body: Dict[str, Any] = {
            "receive_id": chat_id,
            "msg_type": "text",
            "content": content,
        }

        # Thread support (reply to root message)
        if thread_id:
            body["root_id"] = str(thread_id)

        # Build URL with query params
        query = urllib.parse.urlencode(params)
        endpoint = f"/im/v1/messages?{query}"

        resp = self._api("POST", endpoint, body)

        if resp.get("code") == 0:
            return True

        self._log(f"[send] Failed to chat {chat_id}: {resp.get('msg', 'unknown')}")
        return False

    def _compose_safe(self, text: str) -> str:
        """Ensure message fits within Feishu limits."""
        summarized = self.summarize(text, self.max_chars, self.max_lines)

        if len(summarized) > FEISHU_MAX_MESSAGE_LENGTH:
            summarized = summarized[: FEISHU_MAX_MESSAGE_LENGTH - 1] + "..."

        return summarized

    def get_chat_title(self, chat_id: str) -> str:
        """Get chat title via API."""
        resp = self._api("GET", f"/im/v1/chats/{chat_id}")

        if resp.get("code") == 0:
            data = resp.get("data", {})
            return data.get("name", "") or data.get("chat_id", chat_id)

        return chat_id

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        """Download an attachment from Feishu."""
        kind = attachment.get("kind", "")
        token = self._get_token()

        if not token:
            raise ValueError("No valid token")

        if kind == "image":
            image_key = attachment.get("image_key", "")
            if not image_key:
                raise ValueError("Missing image_key")

            url = f"{self.api_base}/im/v1/images/{image_key}"
        elif kind == "file":
            file_key = attachment.get("file_key", "")
            if not file_key:
                raise ValueError("Missing file_key")

            url = f"{self.api_base}/im/v1/files/{file_key}"
        else:
            raise ValueError(f"Unknown attachment kind: {kind}")

        req = urllib.request.Request(url, method="GET")
        req.add_header("Authorization", f"Bearer {token}")

        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                return resp.read()
        except Exception as e:
            raise ValueError(f"Download failed: {e}")

    def send_file(
        self,
        chat_id: str,
        *,
        file_path: Path,
        filename: str,
        caption: str = "",
        thread_id: Optional[int] = None,
    ) -> bool:
        """Send a file to a chat."""
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

        # Step 1: Upload file to get file_key
        boundary = "----cccc" + uuid.uuid4().hex
        upload_url = f"{self.api_base}/im/v1/files"

        safe_fn = (filename or file_path.name or "file").replace("\\", "_").replace("/", "_")

        # Build multipart form data
        body = b""

        # file_type field (opus, mp4, pdf, doc, xls, ppt, stream)
        body += (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="file_type"\r\n\r\n'
            f"stream\r\n"
        ).encode("utf-8")

        # file_name field
        body += (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="file_name"\r\n\r\n'
            f"{safe_fn}\r\n"
        ).encode("utf-8")

        # file field
        body += (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="file"; filename="{safe_fn}"\r\n'
            f"Content-Type: application/octet-stream\r\n\r\n"
        ).encode("utf-8")
        body += raw
        body += f"\r\n--{boundary}--\r\n".encode("utf-8")

        req = urllib.request.Request(upload_url, data=body, method="POST")
        req.add_header("Authorization", f"Bearer {token}")
        req.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")

        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                result = json.loads(resp.read().decode("utf-8", errors="replace"))

            if result.get("code") != 0:
                self._log(f"[send_file] Upload failed: {result.get('msg', 'unknown')}")
                return False

            file_key = result.get("data", {}).get("file_key", "")
            if not file_key:
                self._log("[send_file] No file_key in response")
                return False

        except Exception as e:
            self._log(f"[send_file] Upload error: {e}")
            return False

        # Step 2: Send file message
        content = json.dumps({"file_key": file_key}, ensure_ascii=False)

        params: Dict[str, Any] = {
            "receive_id_type": "chat_id",
        }

        msg_body: Dict[str, Any] = {
            "receive_id": chat_id,
            "msg_type": "file",
            "content": content,
        }

        if thread_id:
            msg_body["root_id"] = str(thread_id)

        query = urllib.parse.urlencode(params)
        endpoint = f"/im/v1/messages?{query}"

        resp = self._api("POST", endpoint, msg_body)

        if resp.get("code") == 0:
            # Send caption as separate message if provided
            if caption:
                self.send_message(chat_id, caption, thread_id)
            return True

        self._log(f"[send_file] Send failed: {resp.get('msg', 'unknown')}")
        return False

    # ===== Typing Indicator (emoji reaction) =====

    TYPING_EMOJI = "OnIt"

    def add_reaction(self, message_id: str, emoji_type: str = "") -> Optional[str]:
        """
        Add an emoji reaction to a message.

        Feishu does not have a native typing indicator, so we simulate one
        by reacting to the user's message with an emoji.

        Returns reaction_id on success (for later removal), None on failure.
        """
        if not message_id or not self._connected:
            return None

        emoji = emoji_type or self.TYPING_EMOJI
        resp = self._api("POST", f"/im/v1/messages/{message_id}/reactions", {
            "reaction_type": {"emoji_type": emoji},
        })

        if resp.get("code") == 0:
            reaction_id = (resp.get("data") or {}).get("reaction_id", "")
            self._log(f"[reaction] Added {emoji} to {message_id} -> {reaction_id}")
            return reaction_id or None

        self._log(f"[reaction] Failed to add {emoji} to {message_id}: {resp.get('msg', 'unknown')}")
        return None

    def remove_reaction(self, message_id: str, reaction_id: str) -> bool:
        """
        Remove a previously added emoji reaction.

        Returns True on success.
        """
        if not message_id or not reaction_id or not self._connected:
            return False

        resp = self._api("DELETE", f"/im/v1/messages/{message_id}/reactions/{reaction_id}")

        if resp.get("code") == 0:
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
        # Handle URL verification challenge
        if "challenge" in event:
            return {"challenge": event["challenge"]}

        # Handle regular events
        schema = event.get("schema", "")

        if schema == "2.0":
            # Event v2 format
            self._enqueue_message(event)
        else:
            # Event v1 format (legacy)
            wrapped = {
                "header": {
                    "event_type": event.get("event", {}).get("type", ""),
                },
                "event": event.get("event", {}),
            }
            self._enqueue_message(wrapped)

        return None
