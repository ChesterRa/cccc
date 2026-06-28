"""Feishu outbound message request builders."""

from __future__ import annotations

import json
from typing import Any, Dict, Optional, Tuple


MESSAGE_ENDPOINT = "/im/v1/messages?receive_id_type=chat_id"


def build_text_message_request(
    chat_id: str,
    text: str,
    *,
    thread_id: Optional[int] = None,
) -> Tuple[str, Dict[str, Any]]:
    body: Dict[str, Any] = {
        "receive_id": chat_id,
        "msg_type": "text",
        "content": json.dumps({"text": text}, ensure_ascii=False),
    }
    if thread_id:
        body["root_id"] = str(thread_id)
    return MESSAGE_ENDPOINT, body


def build_media_message_request(
    chat_id: str,
    *,
    msg_type: str,
    media_key_name: str,
    media_key: str,
    thread_id: Optional[int] = None,
) -> Tuple[str, Dict[str, Any]]:
    body: Dict[str, Any] = {
        "receive_id": chat_id,
        "msg_type": msg_type,
        "content": json.dumps({media_key_name: media_key}, ensure_ascii=False),
    }
    if thread_id:
        body["root_id"] = str(thread_id)
    return MESSAGE_ENDPOINT, body
