"""
DingTalk adapter for CCCC IM Bridge.

Uses DingTalk Open API with Stream mode for real-time messaging.
Reference: https://open.dingtalk.com/document/

Features:
- access_token auto-refresh (2h expiry)
- Stream mode event subscription (long connection)
- Rate limiting (20 msg/sec total)
- File upload/download support
"""

from __future__ import annotations

import hashlib
import hmac
import base64
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

# DingTalk API limits
DINGTALK_MAX_MESSAGE_LENGTH = 4096
DEFAULT_MAX_CHARS = 4096
DEFAULT_MAX_LINES = 64

# API base URLs
DINGTALK_API_OLD = "https://oapi.dingtalk.com"
DINGTALK_API_NEW = "https://api.dingtalk.com"


class RateLimiter:
    """
    Rate limiter for DingTalk API.

    DingTalk limits:
    - Total: ~20 msg/sec
    - Same chat: ~5 msg/sec
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


class DingTalkAdapter(IMAdapter):
    """
    DingTalk adapter using Stream mode for inbound and REST API for outbound.
    """

    platform = "dingtalk"

    def __init__(
        self,
        app_key: str,
        app_secret: str,
        robot_code: str = "",
        log_path: Optional[Path] = None,
        max_chars: int = DEFAULT_MAX_CHARS,
        max_lines: int = DEFAULT_MAX_LINES,
    ):
        self.app_key = app_key
        self.app_secret = app_secret
        self.robot_code = str(robot_code or "").strip()
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
        self._stream_thread: Optional[threading.Thread] = None
        self._stream_running = False

        # Cache for conversation info
        self._conversation_cache: Dict[str, str] = {}

        # Cache for session webhooks (conversation_id -> (webhook_url, expires_at))
        self._session_webhook_cache: Dict[str, tuple[str, float]] = {}

        # Cache for seen message IDs (survives reconnect to deduplicate SDK-resent messages)
        # Key: "{conversation_id}:{msg_id}", Value: timestamp
        self._seen_msg_ids: Dict[str, float] = {}

    def _log(self, msg: str) -> None:
        """Append to log file if configured."""
        if self.log_path:
            try:
                self.log_path.parent.mkdir(parents=True, exist_ok=True)
                with self.log_path.open("a", encoding="utf-8") as f:
                    ts = time.strftime("%Y-%m-%d %H:%M:%S")
                    f.write(f"{ts} [dingtalk] {msg}\n")
            except Exception:
                pass

    def _get_token(self) -> str:
        """Get valid access_token, refreshing if needed."""
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
        Refresh access_token.
        Token expires in 2 hours (7200 seconds).
        """
        url = f"{DINGTALK_API_OLD}/gettoken"
        params = {
            "appkey": self.app_key,
            "appsecret": self.app_secret,
        }
        query = urllib.parse.urlencode(params)
        full_url = f"{url}?{query}"

        req = urllib.request.Request(full_url, method="GET")
        req.add_header("Accept", "application/json")

        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                body = resp.read().decode("utf-8", errors="replace")
                result = json.loads(body)

                if result.get("errcode") == 0:
                    self._token = result.get("access_token", "")
                    expire = int(result.get("expires_in", 7200))
                    self._token_expires = time.time() + expire
                    self._log(f"[token] Refreshed, expires in {expire}s")
                    return True
                else:
                    self._log(f"[token] Failed: {result.get('errmsg', 'unknown')}")
                    return False
        except Exception as e:
            self._log(f"[token] Error: {e}")
            return False

    def _api_old(
        self,
        method: str,
        endpoint: str,
        body: Optional[Dict[str, Any]] = None,
        timeout: int = 15,
    ) -> Dict[str, Any]:
        """
        Call DingTalk old API (oapi.dingtalk.com).

        Used for: token, some legacy APIs
        """
        token = self._get_token()
        if not token and "gettoken" not in endpoint:
            return {"errcode": -1, "errmsg": "No valid token"}

        url = f"{DINGTALK_API_OLD}{endpoint}"

        if method == "GET":
            if body:
                query = urllib.parse.urlencode(body)
                url = f"{url}?{query}"
            if token and "access_token" not in url:
                sep = "&" if "?" in url else "?"
                url = f"{url}{sep}access_token={token}"
            data = None
        else:
            if token and "access_token" not in url:
                sep = "&" if "?" in url else "?"
                url = f"{url}{sep}access_token={token}"
            data = json.dumps(body or {}, ensure_ascii=False).encode("utf-8") if body else None

        req = urllib.request.Request(url, data=data, method=method)
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
            self._log(f"[api_old] {method} {endpoint}: HTTP {http_status} - {err_text}")
            return {"errcode": http_status, "errmsg": str(e), "error": err_text}
        except Exception as e:
            self._log(f"[api_old] {method} {endpoint}: {e}")
            return {"errcode": -1, "errmsg": str(e)}

    def _api_new(
        self,
        method: str,
        endpoint: str,
        body: Optional[Dict[str, Any]] = None,
        timeout: int = 15,
    ) -> Dict[str, Any]:
        """
        Call DingTalk new API (api.dingtalk.com).

        Used for: robot messages, conversations, files
        """
        token = self._get_token()
        if not token:
            return {"code": -1, "message": "No valid token"}

        url = f"{DINGTALK_API_NEW}{endpoint}"

        if method == "GET" and body:
            query = urllib.parse.urlencode(body)
            url = f"{url}?{query}"
            data = None
        else:
            data = json.dumps(body or {}, ensure_ascii=False).encode("utf-8") if body else None

        req = urllib.request.Request(url, data=data, method=method)
        req.add_header("x-acs-dingtalk-access-token", token)
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
            self._log(f"[api_new] {method} {endpoint}: HTTP {http_status} - {err_text}")
            return {"code": http_status, "message": str(e), "error": err_text}
        except Exception as e:
            self._log(f"[api_new] {method} {endpoint}: {e}")
            return {"code": -1, "message": str(e)}

    def connect(self) -> bool:
        """
        Initialize connection to DingTalk.

        1. Verify credentials by getting token
        2. Start Stream mode listener (if available)
        """
        # Clear message queue on reconnect to avoid duplicate messages
        with self._queue_lock:
            self._message_queue.clear()

        # Disable all proxies BEFORE importing dingtalk-stream SDK
        self._disable_proxies()

        # Inbound requires the official SDK (dingtalk-stream) for stream mode.
        try:
            import dingtalk_stream  # type: ignore
        except Exception:
            import sys
            self._log(f"[error] Missing dependency: dingtalk-stream. Install: {sys.executable} -m pip install dingtalk-stream")
            return False

        self._dingtalk_stream = dingtalk_stream

        # Get initial token
        if not self._refresh_token():
            self._log("[connect] Failed to get token")
            return False

        # Start Stream listener for events
        self._start_stream_listener()

        self._connected = True
        self._log(f"[connect] Connected with app_key={self.app_key[:8]}...")
        return True

    def _start_stream_listener(self) -> None:
        """
        Start Stream mode listener using DingTalk official SDK.

        Uses dingtalk-stream SDK for reliable long connection.
        """
        if self._stream_thread and self._stream_thread.is_alive():
            return

        self._stream_running = True

        def stream_loop():
            """Stream event loop using official DingTalk SDK."""
            self._log("[stream] Event listener starting...")

            dingtalk_stream = getattr(self, "_dingtalk_stream", None)
            if dingtalk_stream is None:
                self._log("[stream] Missing SDK handles; connect() should have returned False.")
                return

            AckMessage = getattr(dingtalk_stream, "AckMessage", None)
            if AckMessage is None:
                self._log("[stream] dingtalk_stream.AckMessage not found")
                return

            # Create handler class that references self
            adapter = self

            class CCCCChatbotHandler(dingtalk_stream.ChatbotHandler):
                """Handler for incoming chatbot messages."""

                async def process(self, callback: dingtalk_stream.CallbackMessage):
                    """Process incoming message from DingTalk."""
                    try:
                        adapter._log("[stream] Received message callback")

                        # callback.data is a dict, not an object
                        data = callback.data
                        try:
                            adapter._log(
                                "[stream] msg_id=%s conv_id=%s type=%s from=%s msgtype=%s"
                                % (
                                    str(data.get("msgId") or ""),
                                    str(data.get("conversationId") or ""),
                                    str(data.get("conversationType") or ""),
                                    str(data.get("senderNick") or data.get("senderStaffId") or data.get("senderId") or ""),
                                    str(data.get("msgtype") or ""),
                                )
                            )
                        except Exception:
                            pass

                        # Build event dict for _enqueue_message
                        # Parse content field: may be JSON string or dict,
                        # contains downloadCode/fileName for file/picture/video/audio
                        raw_content = data.get('content', {})
                        if isinstance(raw_content, str):
                            try:
                                parsed_content = json.loads(raw_content)
                            except (json.JSONDecodeError, ValueError):
                                parsed_content = {}
                        elif isinstance(raw_content, dict):
                            parsed_content = raw_content
                        else:
                            parsed_content = {}

                        event = {
                            "msgtype": data.get('msgtype', 'text'),
                            "robotCode": data.get('robotCode', ''),
                            "conversationId": data.get('conversationId', ''),
                            "conversationType": data.get('conversationType', ''),
                            "senderId": data.get('senderId', ''),
                            "senderStaffId": data.get('senderStaffId', ''),
                            "senderNick": data.get('senderNick', ''),
                            "msgId": data.get('msgId', ''),
                            "isAdmin": data.get('isAdmin', False),
                            "chatbotUserId": data.get('chatbotUserId', ''),
                            "conversationTitle": data.get('conversationTitle', ''),
                            "sessionWebhook": data.get('sessionWebhook', ''),
                            "sessionWebhookExpiredTime": data.get('sessionWebhookExpiredTime', 0),
                            "createAt": data.get('createAt', 0),
                            # richText content is inside content.richText
                            "richText": parsed_content.get('richText', []),
                            # picture/file/video/audio fields: check both top-level and content
                            "downloadCode": data.get('downloadCode', '') or parsed_content.get('downloadCode', ''),
                            "fileName": data.get('fileName', '') or parsed_content.get('fileName', ''),
                            # audio fields: check both top-level and content
                            "recognition": data.get('recognition', '') or parsed_content.get('recognition', ''),
                            "duration": data.get('duration', 0) or parsed_content.get('duration', 0),
                            # video fields
                            "videoType": data.get('videoType', '') or parsed_content.get('videoType', ''),
                            # file fields (DingTalk drive)
                            "spaceId": data.get('spaceId', '') or parsed_content.get('spaceId', ''),
                            "fileId": data.get('fileId', '') or parsed_content.get('fileId', ''),
                        }

                        # Diagnostic logging for non-text message types
                        msg_type_val = data.get('msgtype', '')
                        if msg_type_val and msg_type_val != 'text':
                            fn = event.get('fileName', '')
                            fn_safe = f"*{fn[fn.rfind('.'):]}({len(fn)})" if fn and '.' in fn else f"({len(fn)}ch)" if fn else "(none)"
                            adapter._log(
                                f"[stream] non-text message: msgtype={msg_type_val} "
                                f"content_keys={list(parsed_content.keys()) if parsed_content else '(empty)'} "
                                f"downloadCode={'yes' if event.get('downloadCode') else 'no'} "
                                f"fileName={fn_safe}"
                            )

                        # Extract text content (text is usually a dict like {"content": "..."})
                        text_data = data.get('text', {})
                        if text_data:
                            if isinstance(text_data, dict):
                                event["text"] = {"content": text_data.get('content', '')}
                            elif isinstance(text_data, str):
                                # Defensive: some DingTalk versions may return text as plain string
                                event["text"] = {"content": text_data}
                                adapter._log(f"[stream] text_data is str (not dict): {text_data[:100]!r}")
                            else:
                                adapter._log(f"[stream] text_data unexpected type: {type(text_data).__name__}")
                            adapter._log(f"[stream] text message received, msgtype={data.get('msgtype')}, convType={data.get('conversationType')}")

                        # Enqueue the message
                        if adapter._enqueue_message(event):
                            adapter._log("[stream] Message enqueued successfully")
                        else:
                            adapter._log("[stream] Message was not enqueued (filtered/duplicate/error)")

                        return AckMessage.STATUS_OK, 'OK'

                    except Exception as e:
                        adapter._log(f"[stream] Handler error: {e}")
                        import traceback
                        adapter._log(f"[stream] Traceback: {traceback.format_exc()}")
                        return AckMessage.STATUS_SYSTEM_EXCEPTION, str(e)

            try:
                # Create credential and client
                credential = dingtalk_stream.Credential(self.app_key, self.app_secret)
                self._stream_client = dingtalk_stream.DingTalkStreamClient(credential)

                # Register chatbot handler
                self._stream_client.register_callback_handler(
                    dingtalk_stream.chatbot.ChatbotMessage.TOPIC,
                    CCCCChatbotHandler()
                )

                self._log("[stream] Starting DingTalk Stream client...")

                # start_forever() is blocking
                self._stream_client.start_forever()

            except Exception as e:
                self._log(f"[stream] SDK error: {e}")
                import traceback
                self._log(f"[stream] Traceback: {traceback.format_exc()}")

            self._log("[stream] Event listener stopped")

        self._stream_thread = threading.Thread(target=stream_loop, daemon=True)
        self._stream_thread.start()

    def disconnect(self) -> None:
        """Disconnect from DingTalk."""
        self._connected = False
        self._stream_running = False

        # Stop SDK client if running
        if hasattr(self, '_stream_client') and self._stream_client:
            try:
                # DingTalk SDK doesn't have a clean stop method
                # The thread will exit when _stream_running is False
                pass
            except Exception:
                pass

        if self._stream_thread:
            self._stream_thread.join(timeout=2.0)
        self._log("[disconnect] Disconnected")

    def poll(self) -> List[Dict[str, Any]]:
        """
        Poll for new messages.

        Returns queued messages from Stream listener.
        """
        if not self._connected:
            return []

        with self._queue_lock:
            messages = self._message_queue.copy()
            self._message_queue.clear()

        return messages

    def _should_enqueue_message(self, conversation_id: str, msg_id: str) -> bool:
        """
        Check if message should be enqueued (deduplication).

        Returns True if message should be processed, False if it's a duplicate.
        """
        mid = str(msg_id or "").strip()
        if not mid:
            # No msgId means we can't deduplicate; allow processing
            return True

        now = time.time()
        key = f"{conversation_id}:{mid}"

        if key in self._seen_msg_ids:
            self._log(f"[dedup] Skipping duplicate message: {key}")
            return False

        self._seen_msg_ids[key] = now

        # Opportunistic pruning (keep memory bounded)
        if len(self._seen_msg_ids) > 2048:
            cutoff = now - 3600.0  # 1h
            self._seen_msg_ids = {k: ts for k, ts in self._seen_msg_ids.items() if ts >= cutoff}
            if len(self._seen_msg_ids) > 4096:
                # Extreme case: clear old half
                sorted_items = sorted(self._seen_msg_ids.items(), key=lambda x: x[1])
                self._seen_msg_ids = dict(sorted_items[len(sorted_items) // 2:])

        return True

    def _enqueue_message(self, event: Dict[str, Any]) -> bool:
        """
        Process incoming event and enqueue normalized message.

        Called by Stream listener or webhook handler.
        """
        try:
            # DingTalk event structure varies by type
            # Robot callback format
            msg_type = event.get("msgtype", "")
            conversation_id = event.get("conversationId", "")
            sender_id = event.get("senderStaffId", "") or event.get("senderId", "")
            sender_nick = event.get("senderNick", "user")
            msg_id = event.get("msgId", "")

            # Deduplicate: skip if we've already processed this message
            if not self._should_enqueue_message(conversation_id, msg_id):
                return False

            # Extract text based on message type
            # Track attachments extracted from richText (populated below if applicable)
            rich_text_attachments: List[Dict[str, Any]] = []

            if msg_type == "text":
                content = event.get("text", {})
                text = content.get("content", "")
            elif msg_type == "richText":
                raw_rich_text = event.get("richText", [])
                self._log(f"[enqueue] richText raw: {raw_rich_text}")
                text, rich_text_attachments = self._extract_rich_text(raw_rich_text)
                # If text is empty but we have images, use placeholder
                if not text.strip() and rich_text_attachments:
                    text = "[image]"
            elif msg_type == "picture":
                text = "[image]"
            elif msg_type == "file":
                text = f"[file: {event.get('fileName', 'unknown')}]"
            elif msg_type == "audio":
                # Audio: prefer speech recognition text if available
                recognition = str(event.get("recognition", "") or "").strip()
                text = recognition if recognition else "[audio]"
            elif msg_type == "video":
                text = "[video]"
            else:
                text = f"[{msg_type}]"

            if not text.strip():
                self._log(f"[enqueue] Discarding message with empty text: msg_type={msg_type}")
                return False

            # Build attachments list
            attachments: List[Dict[str, Any]] = []
            if msg_type == "picture":
                attachments.append({
                    "provider": "dingtalk",
                    "kind": "image",
                    "download_code": event.get("downloadCode", ""),
                    "file_name": "image.png",
                })
            elif msg_type == "file":
                attachment: Dict[str, Any] = {
                    "provider": "dingtalk",
                    "kind": "file",
                    "download_code": event.get("downloadCode", ""),
                    "file_name": event.get("fileName", "file"),
                }
                if event.get("spaceId"):
                    attachment["space_id"] = event["spaceId"]
                if event.get("fileId"):
                    attachment["file_id"] = event["fileId"]
                attachments.append(attachment)
            elif msg_type == "audio":
                attachments.append({
                    "provider": "dingtalk",
                    "kind": "audio",
                    "download_code": event.get("downloadCode", ""),
                    "file_name": "audio.amr",
                    "duration": event.get("duration", 0),
                })
            elif msg_type == "video":
                attachments.append({
                    "provider": "dingtalk",
                    "kind": "video",
                    "download_code": event.get("downloadCode", ""),
                    "file_name": "video.mp4",
                    "duration": event.get("duration", 0),
                    "video_type": event.get("videoType", ""),
                })
            elif msg_type == "richText" and rich_text_attachments:
                # Add attachments extracted from richText content
                attachments.extend(rich_text_attachments)

            # Determine chat type
            conversation_type = event.get("conversationType", "")
            if conversation_type == "1":
                chat_type = "p2p"
            elif conversation_type == "2":
                chat_type = "group"
            else:
                chat_type = "unknown"

            # Cache robotCode if present (needed for some outbound APIs).
            if not self.robot_code:
                rc = str(event.get("robotCode") or "").strip()
                if rc:
                    self.robot_code = rc
                    self._log("[stream] Learned robot_code from inbound event")

            # Get chat title (use from event if available, else API)
            chat_title = event.get("conversationTitle", "")
            if not chat_title:
                chat_title = self._get_conversation_title_cached(conversation_id)

            # Cache sessionWebhook for this conversation (for replying)
            session_webhook = event.get("sessionWebhook", "")
            session_expires = event.get("sessionWebhookExpiredTime", 0)
            if session_webhook and conversation_id:
                # Convert ms to seconds
                expires_at = session_expires / 1000.0 if session_expires > 1e10 else float(session_expires)
                self._session_webhook_cache[conversation_id] = (session_webhook, expires_at)
                self._log(f"[webhook] Cached: id={conversation_id}, expires_raw={session_expires}, expires_at={expires_at:.0f}")

            # Normalize message
            normalized = {
                "chat_id": conversation_id,
                "chat_title": chat_title,
                "chat_type": chat_type,
                "routed": True,  # ChatbotHandler only receives messages directed at the bot
                "thread_id": 0,  # DingTalk doesn't have threading like this
                "text": text,
                "attachments": attachments,
                "from_user": sender_nick or sender_id,
                "message_id": msg_id,
                "timestamp": self._parse_event_time(event.get("createAt")),
                # Keep sessionWebhook for potential reply use
                "_session_webhook": session_webhook,
            }

            with self._queue_lock:
                self._message_queue.append(normalized)
            return True

        except Exception as e:
            self._log(f"[enqueue] Error: {e}")
            return False

    def _parse_event_time(self, raw: Any) -> float:
        """Parse DingTalk createAt into epoch seconds (supports ms)."""
        try:
            ts = float(raw or 0.0)
        except Exception:
            return 0.0
        if ts <= 0:
            return 0.0
        if ts > 1e11:
            ts = ts / 1000.0
        return ts

    def _extract_rich_text(self, rich_text: List[Dict[str, Any]]) -> tuple[str, List[Dict[str, Any]]]:
        """Extract text and attachments from rich text content.

        DingTalk richText structure:
        [
            {"text": "some text"},
            {"type": "picture", "downloadCode": "xxx", "pictureDownloadCode": "xxx"}
        ]

        Returns:
            tuple of (text, attachments list)
        """
        texts: List[str] = []
        attachments: List[Dict[str, Any]] = []
        try:
            for item in rich_text:
                if item.get("text"):
                    texts.append(item["text"])
                elif item.get("type") == "picture":
                    # Extract picture attachment from richText element
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

    def _get_conversation_title_cached(self, conversation_id: str) -> str:
        """Get conversation title with caching."""
        if conversation_id in self._conversation_cache:
            return self._conversation_cache[conversation_id]

        title = self.get_chat_title(conversation_id)
        self._conversation_cache[conversation_id] = title
        return title

    def _send_via_webhook(self, webhook_url: str, text: str) -> bool:
        """Send message via sessionWebhook (most reliable for groups)."""
        body = {
            "msgtype": "text",
            "text": {
                "content": text
            }
        }
        data = json.dumps(body, ensure_ascii=False).encode('utf-8')

        req = urllib.request.Request(webhook_url, data=data, method="POST")
        req.add_header("Content-Type", "application/json; charset=utf-8")

        try:
            with urllib.request.urlopen(req, timeout=15) as resp:
                result = json.loads(resp.read().decode('utf-8', errors='replace'))
                if result.get('errcode') == 0:
                    self._log(f"[webhook] Sent successfully")
                    return True
                self._log(f"[webhook] Failed: {result}")
                return False
        except Exception as e:
            self._log(f"[webhook] Error: {e}")
            return False

    def send_message(self, chat_id: str, text: str, thread_id: Optional[int] = None) -> bool:
        """
        Send a text message to a conversation.

        Args:
            chat_id: DingTalk conversationId
            text: Message text
            thread_id: Unused (DingTalk doesn't support threading)
        """
        _ = thread_id  # DingTalk doesn't support message threading

        if not text:
            return True

        if not self._connected:
            return False

        # Ensure message fits limit
        safe_text = self._compose_safe(text)

        # Rate limit
        self._rate_limiter.wait_and_acquire(chat_id)

        # Try sessionWebhook first (most reliable for group messages)
        if chat_id in self._session_webhook_cache:
            webhook_url, expires_at = self._session_webhook_cache[chat_id]
            current_time = time.time()
            if current_time < expires_at:
                if self._send_via_webhook(webhook_url, safe_text):
                    return True
                self._log("[send] Webhook failed, falling back to API...")
            else:
                # Webhook expired, remove from cache
                del self._session_webhook_cache[chat_id]
        else:
            self._log("[send] No cached sessionWebhook; falling back to API.")

        if not self.robot_code:
            if chat_id.startswith("cid"):
                self._log("[send] Missing robot_code; cannot use new API fallback. Trying legacy API.")
                return self._send_message_legacy(chat_id, safe_text)
            self._log("[send] Missing robot_code; cannot send via API fallback. Configure DINGTALK_ROBOT_CODE.")
            return False

        # Use robot message API
        body: Dict[str, Any] = {
            "robotCode": self.robot_code,
            "msgKey": "sampleText",
            "msgParam": json.dumps({"content": safe_text}, ensure_ascii=False),
        }

        # Determine if group or 1:1
        if chat_id.startswith("cid"):
            # Group conversation
            body["openConversationId"] = chat_id
            endpoint = "/v1.0/robot/groupMessages/send"
        else:
            # 1:1 conversation - need user ID
            body["userIds"] = [chat_id]
            endpoint = "/v1.0/robot/oToMessages/batchSend"

        resp = self._api_new("POST", endpoint, body)

        if resp.get("processQueryKey") or resp.get("sendResults"):
            return True

        # Try alternative API for older bots
        if "code" in resp or "errcode" in resp:
            return self._send_message_legacy(chat_id, safe_text)

        self._log(f"[send] Failed to chat {chat_id}: {resp}")
        return False

    def _send_message_legacy(self, chat_id: str, text: str) -> bool:
        """Send message using legacy API (for older bot types)."""
        body = {
            "chatid": chat_id,
            "msg": {
                "msgtype": "text",
                "text": {
                    "content": text,
                },
            },
        }

        resp = self._api_old("POST", "/chat/send", body)

        if resp.get("errcode") == 0:
            return True

        self._log(f"[send_legacy] Failed: {resp.get('errmsg', 'unknown')}")
        return False

    def _compose_safe(self, text: str) -> str:
        """Ensure message fits within DingTalk limits."""
        summarized = self.summarize(text, self.max_chars, self.max_lines)

        if len(summarized) > DINGTALK_MAX_MESSAGE_LENGTH:
            summarized = summarized[: DINGTALK_MAX_MESSAGE_LENGTH - 1] + "..."

        return summarized

    def get_chat_title(self, chat_id: str) -> str:
        """Get conversation title via API."""
        # Try new API first
        resp = self._api_new("GET", f"/v1.0/im/conversations/{chat_id}")

        if resp.get("title"):
            return resp["title"]

        # Try legacy API
        resp = self._api_old("GET", "/chat/get", {"chatid": chat_id})

        if resp.get("errcode") == 0:
            return resp.get("name", chat_id)

        return chat_id

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        """Download an attachment from DingTalk."""
        download_code = attachment.get("download_code", "")
        if not download_code:
            raise ValueError("Missing download_code")

        if not self.robot_code:
            raise ValueError("Missing robot_code (configure DINGTALK_ROBOT_CODE to download attachments)")

        token = self._get_token()
        if not token:
            raise ValueError("No valid token")

        # Get download URL
        resp = self._api_new("POST", "/v1.0/robot/messageFiles/download", {
            "downloadCode": download_code,
            "robotCode": self.robot_code,
        })

        download_url = resp.get("downloadUrl", "")
        if not download_url:
            raise ValueError(f"Failed to get download URL: {resp}")

        # Download file
        req = urllib.request.Request(download_url, method="GET")

        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                return r.read()
        except Exception as e:
            raise ValueError(f"Download failed: {e}")

    def _is_image_file(self, filename: str) -> bool:
        """Check if file is an image based on extension."""
        image_extensions = {".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp"}
        ext = Path(filename).suffix.lower()
        return ext in image_extensions

    def _upload_media(self, raw: bytes, filename: str, media_type: str = "file") -> Optional[str]:
        """
        Upload file to DingTalk and return media_id.

        Args:
            raw: File content bytes
            filename: Original filename
            media_type: "file" or "image"

        Returns:
            media_id if successful, None otherwise
        """
        token = self._get_token()
        if not token:
            return None

        boundary = "----cccc" + uuid.uuid4().hex
        upload_url = f"{DINGTALK_API_OLD}/media/upload"
        upload_url = f"{upload_url}?access_token={token}&type={media_type}"

        safe_fn = (filename or "file").replace("\\", "_").replace("/", "_")

        # Determine content type based on media type
        if media_type == "image":
            ext = Path(filename).suffix.lower()
            content_type_map = {
                ".png": "image/png",
                ".jpg": "image/jpeg",
                ".jpeg": "image/jpeg",
                ".gif": "image/gif",
                ".bmp": "image/bmp",
                ".webp": "image/webp",
            }
            content_type = content_type_map.get(ext, "application/octet-stream")
        else:
            content_type = "application/octet-stream"

        # Build multipart form data
        body = b""
        body += (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="media"; filename="{safe_fn}"\r\n'
            f"Content-Type: {content_type}\r\n\r\n"
        ).encode("utf-8")
        body += raw
        body += f"\r\n--{boundary}--\r\n".encode("utf-8")

        req = urllib.request.Request(upload_url, data=body, method="POST")
        req.add_header("Content-Type", f"multipart/form-data; boundary={boundary}")

        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                result = json.loads(resp.read().decode("utf-8", errors="replace"))

            if result.get("errcode") != 0:
                self._log(f"[upload_media] Upload failed: {result.get('errmsg', 'unknown')}")
                return None

            media_id = result.get("media_id", "")
            if not media_id:
                self._log("[upload_media] No media_id in response")
                return None

            return media_id

        except Exception as e:
            self._log(f"[upload_media] Upload error: {e}")
            return None

    def _send_file_via_webhook(
        self,
        webhook_url: str,
        raw: bytes,
        filename: str,
        is_image: bool = False,
    ) -> bool:
        """
        Send file via sessionWebhook.

        For images, uploads to DingTalk media API and sends image message.
        For files, uploads and sends file message.

        Args:
            webhook_url: Session webhook URL
            raw: File content bytes
            filename: Original filename
            is_image: Whether to send as image type

        Returns:
            True if successful, False otherwise
        """
        # Upload media first
        media_type = "image" if is_image else "file"
        media_id = self._upload_media(raw, filename, media_type)
        if not media_id:
            return False

        # Build webhook message body
        if is_image:
            body = {
                "msgtype": "image",
                "image": {
                    "mediaId": media_id
                }
            }
        else:
            body = {
                "msgtype": "file",
                "file": {
                    "mediaId": media_id
                }
            }

        data = json.dumps(body, ensure_ascii=False).encode('utf-8')

        req = urllib.request.Request(webhook_url, data=data, method="POST")
        req.add_header("Content-Type", "application/json; charset=utf-8")

        try:
            with urllib.request.urlopen(req, timeout=15) as resp:
                result = json.loads(resp.read().decode('utf-8', errors='replace'))
                if result.get('errcode') == 0:
                    self._log(f"[send_file_webhook] Sent successfully via webhook")
                    return True
                self._log(f"[send_file_webhook] Failed: {result}")
                return False
        except Exception as e:
            self._log(f"[send_file_webhook] Error: {e}")
            return False

    def _send_file_via_api(
        self,
        chat_id: str,
        raw: bytes,
        filename: str,
        is_image: bool = False,
    ) -> bool:
        """
        Send file via new robot API (/v1.0/robot/groupMessages/send).

        Args:
            chat_id: DingTalk conversationId
            raw: File content bytes
            filename: Original filename
            is_image: Whether to send as image type

        Returns:
            True if successful, False otherwise
        """
        if not self.robot_code:
            self._log("[send_file_api] Missing robot_code; cannot use new API.")
            return False

        # Upload media first
        media_type = "image" if is_image else "file"
        media_id = self._upload_media(raw, filename, media_type)
        if not media_id:
            return False

        # Build API request body
        if is_image:
            msg_key = "sampleImageMsg"
            msg_param = json.dumps({"photoURL": f"@lADPD{media_id}"}, ensure_ascii=False)
        else:
            msg_key = "sampleFile"
            msg_param = json.dumps({"mediaId": media_id, "fileName": filename}, ensure_ascii=False)

        body: Dict[str, Any] = {
            "robotCode": self.robot_code,
            "msgKey": msg_key,
            "msgParam": msg_param,
        }

        # Determine endpoint based on chat type
        if chat_id.startswith("cid"):
            # Group conversation
            body["openConversationId"] = chat_id
            endpoint = "/v1.0/robot/groupMessages/send"
        else:
            # 1:1 conversation
            body["userIds"] = [chat_id]
            endpoint = "/v1.0/robot/oToMessages/batchSend"

        resp = self._api_new("POST", endpoint, body)

        if resp.get("processQueryKey") or resp.get("sendResults"):
            self._log(f"[send_file_api] Sent successfully via API")
            return True

        self._log(f"[send_file_api] Failed: {resp}")
        return False

    def send_file(
        self,
        chat_id: str,
        *,
        file_path: Path,
        filename: str,
        caption: str = "",
        thread_id: Optional[int] = None,
    ) -> bool:
        """
        Send a file to a conversation.

        Uses sessionWebhook first (most reliable for groups), falls back to
        new robot API (/v1.0/robot/groupMessages/send).
        """
        _ = thread_id  # DingTalk doesn't support threading

        if not self._connected:
            return False

        # Rate limit
        self._rate_limiter.wait_and_acquire(chat_id)

        try:
            raw = file_path.read_bytes()
        except Exception as e:
            self._log(f"[send_file] Read failed: {e}")
            return False

        safe_fn = (filename or file_path.name or "file").replace("\\", "_").replace("/", "_")
        is_image = self._is_image_file(safe_fn)

        # Try sessionWebhook first (most reliable for group messages)
        if chat_id in self._session_webhook_cache:
            webhook_url, expires_at = self._session_webhook_cache[chat_id]
            current_time = time.time()
            if current_time < expires_at:
                if self._send_file_via_webhook(webhook_url, raw, safe_fn, is_image):
                    if caption:
                        self.send_message(chat_id, caption)
                    return True
                self._log("[send_file] Webhook failed, falling back to API...")
            else:
                # Webhook expired, remove from cache
                del self._session_webhook_cache[chat_id]
        else:
            self._log("[send_file] No cached sessionWebhook; using API.")

        # Fallback to new robot API
        if self._send_file_via_api(chat_id, raw, safe_fn, is_image):
            if caption:
                self.send_message(chat_id, caption)
            return True

        self._log(f"[send_file] All methods failed for chat {chat_id}")
        return False

    def format_outbound(self, by: str, to: List[str], text: str, is_system: bool = False) -> str:
        """Format message for DingTalk display."""
        formatted = super().format_outbound(by, to, text, is_system)
        return self._compose_safe(formatted)

    # ===== Webhook Event Handling =====

    def handle_webhook_event(self, event: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        """
        Handle incoming webhook event from DingTalk.

        This method should be called by an external HTTP server
        when receiving events from DingTalk webhook/callback.
        """
        self._enqueue_message(event)
        return None

    def verify_callback_signature(
        self,
        timestamp: str,
        nonce: str,
        signature: str,
    ) -> bool:
        """
        Verify DingTalk callback signature.

        Used for webhook callback security validation.
        """
        try:
            # Sort and concatenate
            sign_str = f"{timestamp}\n{nonce}\n{self.app_secret}"

            # HMAC-SHA256
            hmac_code = hmac.new(
                self.app_secret.encode("utf-8"),
                sign_str.encode("utf-8"),
                hashlib.sha256,
            ).digest()

            # Base64 encode
            computed = base64.b64encode(hmac_code).decode("utf-8")

            return computed == signature
        except Exception:
            return False
