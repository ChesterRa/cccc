"""Map Feishu inbound events to CCCC normalized IM messages."""

from __future__ import annotations

import json
from typing import Any, Callable, Dict, List, Optional

from .mentions import FeishuBotIdentity, FeishuMentionRouter


class FeishuMessageMapper:
    """Convert Feishu receive events into the bridge's normalized message shape."""

    def __init__(
        self,
        *,
        bot_identity: FeishuBotIdentity,
        chat_title_lookup: Callable[[str], str],
    ):
        self.bot_identity = bot_identity
        self.chat_title_lookup = chat_title_lookup

    def map_event(self, event: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        header = event.get("header", {})
        if header.get("event_type", "") != "im.message.receive_v1":
            return None

        payload = event.get("event", {})
        message = payload.get("message", {})
        sender = payload.get("sender", {})

        sender_type = sender.get("sender_type", "")
        if sender_type == "app":
            return None

        msg_type = message.get("message_type", "")
        content = self._parse_content(message.get("content", "{}"))
        text = self._extract_text(msg_type, content)
        text = self._replace_mention_placeholders(text, message.get("mentions") or [])
        if not text.strip():
            return None

        chat_id = message.get("chat_id", "")
        chat_type = message.get("chat_type", "")
        sender_id = sender.get("sender_id", {})
        mentions = message.get("mentions") or []
        is_at_bot = FeishuMentionRouter(self.bot_identity).mentions_bot(mentions)

        return {
            "chat_id": chat_id,
            "chat_title": self.chat_title_lookup(chat_id),
            "chat_type": chat_type,
            "routed": bool(chat_type == "p2p" or is_at_bot),
            "thread_id": message.get("root_id", 0) or 0,
            "text": text,
            "attachments": self._attachments(msg_type, content),
            "from_user": sender_id.get("open_id", ""),
            "message_id": message.get("message_id", ""),
            "timestamp": self._parse_message_time(message.get("create_time")),
        }

    def _parse_content(self, content_str: Any) -> Dict[str, Any]:
        try:
            content = json.loads(str(content_str or "{}"))
        except Exception:
            return {}
        return content if isinstance(content, dict) else {}

    def _extract_text(self, msg_type: str, content: Dict[str, Any]) -> str:
        if msg_type == "text":
            return str(content.get("text", ""))
        if msg_type == "post":
            return self._extract_post_text(content)
        if msg_type == "image":
            return "[image]"
        if msg_type == "file":
            return f"[file: {content.get('file_name', 'unknown')}]"
        return f"[{msg_type}]"

    def _replace_mention_placeholders(self, text: str, mentions: Any) -> str:
        if not isinstance(mentions, list):
            return text
        for mention in mentions:
            if not isinstance(mention, dict):
                continue
            key = mention.get("key", "")
            name = mention.get("name", "")
            if key and name:
                text = text.replace(key, f"@{name}")
        return text

    def _attachments(self, msg_type: str, content: Dict[str, Any]) -> List[Dict[str, Any]]:
        if msg_type == "image":
            return [
                {
                    "provider": "feishu",
                    "kind": "image",
                    "image_key": content.get("image_key", ""),
                    "file_name": "image.png",
                }
            ]
        if msg_type == "file":
            return [
                {
                    "provider": "feishu",
                    "kind": "file",
                    "file_key": content.get("file_key", ""),
                    "file_name": content.get("file_name", "file"),
                }
            ]
        return []

    def _extract_post_text(self, content: Dict[str, Any]) -> str:
        texts = []
        try:
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
        try:
            ts = float(raw or 0.0)
        except Exception:
            return 0.0
        if ts <= 0:
            return 0.0
        if ts > 1e11:
            ts = ts / 1000.0
        return ts
