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
import urllib.request
from pathlib import Path
from typing import Any, Dict, List, Optional

from .base import IMAdapter, IMProcessingContext, IMProcessingOutcome, OutboundStreamHandle
from .dingtalk_api import DingTalkApiClient
from .dingtalk_media import DingTalkMediaService
from .dingtalk_messages import (
    DEFAULT_MAX_CHARS,
    DEFAULT_MAX_LINES,
    DINGTALK_MAX_MESSAGE_LENGTH,
    RateLimiter,
    build_markdown_payload,
    extract_rich_text,
    normalize_text,
    parse_event_time,
    split_message_chunks,
)
from .dingtalk_reactions import DingTalkReactionService
from .dingtalk_sender import DingTalkSender
from .dingtalk_session import DingTalkConversationStore


class DingTalkAdapter(IMAdapter):
    """
    DingTalk adapter using Stream mode for inbound and REST API for outbound.
    """

    platform = "dingtalk"
    capabilities = {
        "text_in": "yes",
        "text_out": "yes",
        "files_in": "yes",
        "files_out": "yes",
        "threads": "no",
        "reactions": "yes",
        "typing": "partial",
        "streaming": "partial",
        "voice_in": "partial",
        "markdown": "yes",
    }
    capability_notes = {
        "streaming": "AI Card streaming is available when the card client is configured",
        "reactions": "Robot emoji reactions are used for processing indicators via DingTalk OpenAPI",
        "typing": "Processing feedback uses DingTalk emoji reactions, with temporary AI Card fallback when robot_code is configured",
        "voice_in": "audio attachments/recognition may arrive from DingTalk; local ASR is not automatic",
    }
    _LAST_SENDER_MAX = 256

    def __init__(
        self,
        app_key: str,
        app_secret: str,
        robot_code: str = "",
        log_path: Optional[Path] = None,
        session_state_path: Optional[Path] = None,
        max_chars: int = DEFAULT_MAX_CHARS,
        max_lines: int = DEFAULT_MAX_LINES,
    ):
        self.app_key = app_key
        self.app_secret = app_secret
        self.robot_code = str(robot_code or "").strip()
        self.log_path = log_path
        self.max_chars = max_chars
        self.max_lines = max_lines
        self._api = DingTalkApiClient(app_key=app_key, app_secret=app_secret, log=self._log)

        # Message queue (thread-safe)
        self._message_queue: List[Dict[str, Any]] = []
        self._queue_lock = threading.Lock()

        # Rate limiter
        self._rate_limiter = RateLimiter(max_per_second=5.0)
        self._sender = DingTalkSender(
            api_old=lambda method, endpoint, body, timeout=15: self._api_old(method, endpoint, body, timeout),
            api_new=lambda method, endpoint, body, timeout=15: self._api_new(method, endpoint, body, timeout),
            send_webhook=lambda url, text, at_user_ids=None: self._send_via_webhook(
                url,
                text,
                at_user_ids=at_user_ids,
            ),
            rate_limiter=self._rate_limiter,
            robot_code_getter=lambda: self.robot_code,
            is_group=self._is_group_conversation,
            user_id_for_chat=self._conversation_user_id,
            webhook_entry=self._session_webhook_entry,
            last_sender_for_chat=self._last_sender_staff_id,
            log=self._log,
            max_chars_getter=lambda: self.max_chars,
            max_lines_getter=lambda: self.max_lines,
        )
        self._media = DingTalkMediaService(
            get_token=self._get_token,
            api_new=lambda method, endpoint, body, timeout=15: self._api_new(method, endpoint, body, timeout),
            robot_code_getter=lambda: self.robot_code,
            is_group=self._is_group_conversation,
            user_id_for_chat=self._conversation_user_id,
            webhook_entry=self._session_webhook_entry,
            send_message=lambda chat_id, caption, mention_user_ids=None: self.send_message(
                chat_id,
                caption,
                mention_user_ids=mention_user_ids,
            ),
            rate_limit=self._rate_limiter.wait_and_acquire,
            log=self._log,
        )
        self._reaction_service = DingTalkReactionService(
            get_token=self._get_token,
            robot_code_getter=lambda: self.robot_code,
            log=self._log,
        )

        # Connection state
        self._connected = False
        self._stream_thread: Optional[threading.Thread] = None
        self._stream_running = False

        # Cache for conversation info
        self._conversation_cache: Dict[str, str] = {}

        if session_state_path is None and log_path is not None:
            session_state_path = log_path.parent / "im_dingtalk_sessions.json"
        self._conversation_store = DingTalkConversationStore(session_state_path)
        # Backward-compatible alias for existing tests and internal callers.
        self._session_webhook_cache = self._conversation_store._webhooks

        # Cache for seen message IDs (survives reconnect to deduplicate SDK-resent messages)
        # Key: "{conversation_id}:{msg_id}", Value: timestamp
        self._seen_msg_ids: Dict[str, float] = {}

        # Persistent AI Card client (lazy init) — shared across stream calls
        # so throttle state survives across begin/update/end invocations.
        self._card_client: Optional[Any] = None

        # Cache: last inbound mentionable sender per conversation (for backward-compatible
        # outbound @mention fallback). Only senderStaffId is safe to reuse here.
        # conversation_id -> (staff_id, nick)   — bounded to _LAST_SENDER_MAX entries
        self._last_sender: Dict[str, tuple[str, str]] = {}

        # Inbound health tracking: timestamp of last successfully enqueued message
        self._last_enqueue_ts: float = 0.0

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

    def _remember_conversation(
        self,
        conversation_id: str,
        chat_type: str,
        user_id: str = "",
        session_webhook: str = "",
        session_expires: Any = 0,
    ) -> None:
        """Remember routing and reply metadata from an inbound DingTalk event."""
        chat_id = str(conversation_id or "").strip()
        if not chat_id:
            return
        self._conversation_store.remember(
            chat_id,
            chat_type=chat_type,
            user_id=user_id,
            session_webhook=session_webhook,
            session_expires=session_expires,
        )
        if session_webhook:
            try:
                expires_raw = float(session_expires or 0.0)
            except Exception:
                expires_raw = 0.0
            expires_at = expires_raw / 1000.0 if expires_raw > 1e10 else expires_raw
            self._log(f"[webhook] Cached: id={chat_id}, expires_raw={session_expires}, expires_at={expires_at:.0f}")

    def _forget_session_webhook(self, chat_id: str) -> None:
        self._conversation_store.forget_webhook(chat_id)

    def _session_webhook_entry(self, chat_id: str) -> Optional[tuple[str, float]]:
        return self._conversation_store.webhook_entry(chat_id)

    def _conversation_chat_type(self, chat_id: str) -> str:
        return self._conversation_store.chat_type(chat_id)

    def _conversation_user_id(self, chat_id: str) -> str:
        return self._conversation_store.user_id(chat_id)

    def _is_group_conversation(self, chat_id: str) -> bool:
        return self._conversation_store.is_group(chat_id)

    def _last_sender_staff_id(self, chat_id: str) -> Optional[str]:
        sender = self._last_sender.get(chat_id)
        if not sender:
            return None
        staff_id, _nick = sender
        return staff_id or None

    def _get_token(self) -> str:
        """Get valid access_token, refreshing if needed."""
        return self._api.get_token()

    def _refresh_token(self) -> bool:
        """Refresh access_token."""
        return self._api.refresh_token()

    def _api_old(
        self,
        method: str,
        endpoint: str,
        body: Optional[Dict[str, Any]] = None,
        timeout: int = 15,
    ) -> Dict[str, Any]:
        """Call DingTalk old API."""
        return self._api.api_old(method, endpoint, body, timeout)

    def _api_new(
        self,
        method: str,
        endpoint: str,
        body: Optional[Dict[str, Any]] = None,
        timeout: int = 15,
    ) -> Dict[str, Any]:
        """Call DingTalk new API."""
        return self._api.api_new(method, endpoint, body, timeout)

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
            conversation_id = str(event.get("conversationId", "") or "").strip()
            sender_staff_id = str(event.get("senderStaffId", "") or "").strip()
            sender_id = str(event.get("senderId", "") or "").strip()
            sender_display_id = sender_staff_id or sender_id
            mention_user_ids = [sender_staff_id] if sender_staff_id else []
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

            # Cache sessionWebhook and routing metadata for this conversation (for replying)
            session_webhook = event.get("sessionWebhook", "")
            session_expires = event.get("sessionWebhookExpiredTime", 0)
            self._remember_conversation(
                conversation_id,
                chat_type,
                sender_display_id,
                session_webhook,
                session_expires,
            )

            # Cache sender for outbound @mention fallback (group chats only).
            # senderId is not necessarily a valid atUserIds target, so only keep staffId.
            if chat_type == "group" and sender_staff_id and conversation_id:
                # Evict oldest entries when cache is full
                if len(self._last_sender) >= self._LAST_SENDER_MAX:
                    try:
                        oldest_key = next(iter(self._last_sender))
                        del self._last_sender[oldest_key]
                    except StopIteration:
                        pass
                self._last_sender[conversation_id] = (sender_staff_id, sender_nick)
            elif conversation_id:
                self._last_sender.pop(conversation_id, None)

            # Normalize message
            normalized = {
                "chat_id": conversation_id,
                "chat_title": chat_title,
                "chat_type": chat_type,
                "routed": True,  # ChatbotHandler only receives messages directed at the bot
                "thread_id": 0,  # DingTalk doesn't have threading like this
                "text": text,
                "attachments": attachments,
                "from_user": sender_nick or sender_display_id,
                "from_user_id": sender_display_id,
                "mention_user_ids": mention_user_ids,
                "message_id": _reaction_message_id(msg_id, conversation_id),
                "timestamp": self._parse_event_time(event.get("createAt")),
                # Keep sessionWebhook for potential reply use
                "_session_webhook": session_webhook,
            }

            with self._queue_lock:
                self._message_queue.append(normalized)
            self._last_enqueue_ts = time.time()
            self._log(f"[enqueue] OK msg_id={msg_id} from={sender_nick or sender_id} text={text[:80]!r}")
            return True

        except Exception as e:
            self._log(f"[enqueue] Error: {e}")
            return False

    def _parse_event_time(self, raw: Any) -> float:
        """Parse DingTalk createAt into epoch seconds (supports ms)."""
        return parse_event_time(raw)

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
        return extract_rich_text(rich_text)

    def _get_conversation_title_cached(self, conversation_id: str) -> str:
        """Get conversation title with caching."""
        if conversation_id in self._conversation_cache:
            return self._conversation_cache[conversation_id]

        title = self.get_chat_title(conversation_id)
        self._conversation_cache[conversation_id] = title
        return title

    def _build_markdown_payload(self, text: str, at_user_ids: Optional[List[str]] = None) -> Dict[str, Any]:
        """Build a DingTalk markdown payload with optional real-mention metadata."""
        return build_markdown_payload(text, at_user_ids)

    def _send_via_webhook(self, webhook_url: str, text: str,
                          at_user_ids: Optional[List[str]] = None) -> bool:
        """Send message via sessionWebhook (most reliable for groups)."""
        return self._sender.send_via_webhook(webhook_url, text, at_user_ids=at_user_ids)

    def send_message(
        self,
        chat_id: str,
        text: str,
        thread_id: Optional[int] = None,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
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

        return self._sender.send_text(chat_id, text, mention_user_ids=mention_user_ids)

    def _send_message_legacy(
        self,
        chat_id: str,
        text: str,
        at_user_ids: Optional[List[str]] = None,
    ) -> bool:
        """Send message using legacy API (for older bot types)."""
        return self._sender.send_legacy(chat_id, text, at_user_ids=at_user_ids)

    def _compose_safe(self, text: str) -> str:
        """Ensure message fits within DingTalk limits."""
        summarized = self.summarize(text, self.max_chars, self.max_lines)

        if len(summarized) > DINGTALK_MAX_MESSAGE_LENGTH:
            summarized = summarized[: DINGTALK_MAX_MESSAGE_LENGTH - 1] + "..."

        return summarized

    def _prepare_stream_text(self, text: str) -> str:
        """Normalize AI Card content to the same safe envelope as plain messages."""
        return self._compose_safe(text)

    def _normalize_text(self, text: str) -> str:
        """Normalize outbound text without truncating content."""
        return normalize_text(text)

    def _split_message_chunks(self, text: str) -> List[str]:
        """Split a long outbound message into DingTalk-sized chunks."""
        return split_message_chunks(text, max_chars=self.max_chars, max_lines=self.max_lines)

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
        return self._media.download_attachment(attachment)

    def _upload_media(self, raw: bytes, filename: str, media_type: str = "file") -> Optional[str]:
        """Upload file to DingTalk and return media_id."""
        return self._media.upload_media(raw, filename, media_type)

    def _send_file_via_webhook(
        self,
        webhook_url: str,
        raw: bytes,
        filename: str,
        is_image: bool = False,
    ) -> bool:
        """Send file via sessionWebhook."""
        return self._media.send_file_via_webhook(webhook_url, raw, filename, is_image)

    def _send_file_via_api(
        self,
        chat_id: str,
        raw: bytes,
        filename: str,
        is_image: bool = False,
    ) -> bool:
        """Send file via new robot API."""
        return self._media.send_file_via_api(chat_id, raw, filename, is_image)

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
        """
        Send a file to a conversation.

        Uses sessionWebhook first (most reliable for groups), falls back to
        new robot API (/v1.0/robot/groupMessages/send).
        """
        _ = thread_id  # DingTalk doesn't support threading

        if not self._connected:
            return False
        return self._media.send_file(
            chat_id,
            file_path=file_path,
            filename=filename,
            caption=caption,
            mention_user_ids=mention_user_ids,
        )

    def format_outbound(self, by: str, to: List[str], text: str, is_system: bool = False) -> str:
        """Format message for DingTalk display."""
        return super().format_outbound(by, to, text, is_system)

    def add_reaction(self, message_id: str, emoji_type: str = "") -> Optional[str]:
        """Add a DingTalk emoji reaction to an inbound message."""
        if not message_id or not self._connected:
            return None
        target = _parse_reaction_message_id(message_id)
        if not target:
            self._log(f"[reaction] Invalid DingTalk message_id for reaction: {message_id}")
            return None
        msg_id, conversation_id = target
        reaction_type = emoji_type or "🤔Thinking"
        ok = self._run_async(
            self._reaction_service.send_reaction(
                message_id=msg_id,
                conversation_id=conversation_id,
                reaction_type=reaction_type,
                recall=False,
            )
        )
        if not ok:
            return None
        reaction_id = _reaction_message_id(msg_id, conversation_id, reaction_type)
        self._log(f"[reaction] Added {reaction_type} to {msg_id} -> {reaction_id}")
        return reaction_id

    def remove_reaction(self, message_id: str, reaction_id: str) -> bool:
        """Recall a DingTalk emoji reaction."""
        if not message_id or not reaction_id or not self._connected:
            return False
        parsed = _parse_reaction_id(reaction_id)
        if not parsed:
            self._log(f"[reaction] Invalid DingTalk reaction_id: {reaction_id}")
            return False
        msg_id, conversation_id, reaction_type = parsed
        ok = self._run_async(
            self._reaction_service.send_reaction(
                message_id=msg_id,
                conversation_id=conversation_id,
                reaction_type=reaction_type,
                recall=True,
            )
        )
        if ok:
            self._log(f"[reaction] Removed {reaction_id}")
            return True
        return False

    def on_processing_start(self, context: IMProcessingContext) -> Optional[str]:
        """Show best-effort processing feedback with a temporary AI Card."""
        reaction_id = self.add_reaction(context.message_id, "🤔Thinking")
        if reaction_id:
            return f"reaction:{reaction_id}"
        if not self.robot_code:
            self._log(f"[processing] dingtalk AI Card skipped: missing robot_code chat={context.chat_id}")
            return None
        try:
            client = self._get_card_client()
            card_instance_id = self._run_async(client.create_card(context.chat_id, "处理中..."))
        except Exception as exc:
            self._log(f"[processing] dingtalk AI Card create failed chat={context.chat_id}: {exc}")
            return None
        if not card_instance_id:
            self._log(f"[processing] dingtalk AI Card create returned no card id chat={context.chat_id}")
            return None
        self._log(f"[processing] dingtalk AI Card created card={card_instance_id} chat={context.chat_id}")
        return f"dingtalk_card:{card_instance_id}"

    def on_processing_complete(
        self,
        context: IMProcessingContext,
        outcome: IMProcessingOutcome,
        handle: Optional[str],
    ) -> None:
        """Finalize a temporary processing AI Card, if one was created."""
        if handle and handle.startswith("reaction:"):
            reaction_id = handle.removeprefix("reaction:")
            self.remove_reaction(context.message_id, reaction_id)
            final_reaction = "🥳Done" if outcome == IMProcessingOutcome.SUCCESS else "❌Failed"
            self.add_reaction(context.message_id, final_reaction)
            return
        if not handle or not handle.startswith("dingtalk_card:"):
            return
        card_instance_id = handle.removeprefix("dingtalk_card:")
        if not card_instance_id:
            return
        text = "处理完成"
        if outcome == IMProcessingOutcome.FAILURE:
            text = "处理失败"
        elif outcome == IMProcessingOutcome.CANCELLED:
            text = "处理已取消"
        try:
            client = self._get_card_client()
            self._run_async(client.finalize_card(card_instance_id, text))
            self._log(f"[processing] dingtalk AI Card finalized card={card_instance_id} outcome={outcome.value}")
        except Exception as exc:
            self._log(f"[processing] dingtalk AI Card finalize failed card={card_instance_id}: {exc}")

    # ===== Webhook Event Handling =====

    def handle_webhook_event(self, event: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        """
        Handle incoming webhook event from DingTalk.

        This method should be called by an external HTTP server
        when receiving events from DingTalk webhook/callback.
        """
        self._enqueue_message(event)
        return None

    # ── Streaming (AI Card) ────────────────────────────────────────────

    def _get_card_client(self) -> Any:
        """Return the persistent DingTalkAICardClient (lazy init).

        Shared across all stream calls so throttle state survives.
        """
        if self._card_client is None:
            from .dingtalk_card import DingTalkAICardClient

            self._card_client = DingTalkAICardClient(
                self._get_token,
                robot_code=self.robot_code,
            )
        else:
            try:
                self._card_client._robot_code = self.robot_code
            except Exception:
                pass
        return self._card_client

    @staticmethod
    def _run_async(coro: Any) -> Any:
        """Run an async coroutine from sync context."""
        import asyncio

        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            loop = None
        if loop is not None and loop.is_running():
            import concurrent.futures

            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                return pool.submit(asyncio.run, coro).result(timeout=30)
        return asyncio.run(coro)

    def begin_stream(self, chat_id: str, stream_id: str, *, text: str = "", thread_id: Optional[int] = None) -> Optional[OutboundStreamHandle]:
        """Create a DingTalk AI Card and return a stream handle."""
        try:
            client = self._get_card_client()
            safe_text = self._prepare_stream_text(text)
            card_instance_id = self._run_async(client.create_card(chat_id, safe_text))
            if not card_instance_id:
                self._log(f"[stream] begin_stream create_card failed for chat={chat_id} stream={stream_id}")
                return None
            from .dingtalk_card import DingTalkCardHandle

            card_handle = DingTalkCardHandle(client, card_instance_id, stream_id=stream_id)
            return card_handle.as_handle()
        except Exception:
            self._log(f"[stream] begin_stream failed for chat={chat_id} stream={stream_id}")
            return None

    def update_stream(self, handle: OutboundStreamHandle, *, text: str = "", seq: int = 0) -> bool:
        """Push a streaming update to the AI Card."""
        try:
            client = self._get_card_client()
            card_instance_id = handle.get("platform_handle", "")
            if not card_instance_id:
                return False
            safe_text = self._prepare_stream_text(text)
            self._run_async(client.update_card(str(card_instance_id), safe_text, seq=seq))
            return True
        except Exception:
            self._log(f"[stream] update_stream failed for stream={handle.get('stream_id', '')}")
            return False

    def end_stream(self, handle: OutboundStreamHandle, *, text: str = "") -> bool:
        """Finalize the AI Card."""
        try:
            client = self._get_card_client()
            card_instance_id = handle.get("platform_handle", "")
            if not card_instance_id:
                return False
            safe_text = self._prepare_stream_text(text)
            self._run_async(client.finalize_card(str(card_instance_id), safe_text))
            return True
        except Exception:
            self._log(f"[stream] end_stream failed for stream={handle.get('stream_id', '')}")
            return False

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


def _reaction_message_id(message_id: str, conversation_id: str, reaction_type: str = "") -> str:
    parts = [str(message_id or "").strip(), str(conversation_id or "").strip()]
    if reaction_type:
        parts.append(str(reaction_type).strip())
    return "|".join(parts)


def _parse_reaction_message_id(message_id: str) -> Optional[tuple[str, str]]:
    parts = str(message_id or "").split("|", 1)
    if len(parts) != 2:
        return None
    msg_id, conversation_id = (part.strip() for part in parts)
    if not msg_id or not conversation_id:
        return None
    return msg_id, conversation_id


def _parse_reaction_id(reaction_id: str) -> Optional[tuple[str, str, str]]:
    parts = str(reaction_id or "").split("|", 2)
    if len(parts) != 3:
        return None
    msg_id, conversation_id, reaction_type = (part.strip() for part in parts)
    if not msg_id or not conversation_id or not reaction_type:
        return None
    return msg_id, conversation_id, reaction_type
