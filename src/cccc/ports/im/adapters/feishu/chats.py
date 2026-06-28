"""Feishu chat metadata helpers."""

from __future__ import annotations

from typing import Any, Callable, Dict


class FeishuChatTitleResolver:
    """Fetch and cache Feishu chat titles by chat_id."""

    def __init__(self, api_fn: Callable[[str, str], Dict[str, Any]]):
        self.api_fn = api_fn
        self._cache: Dict[str, str] = {}

    def get(self, chat_id: str) -> str:
        if chat_id in self._cache:
            return self._cache[chat_id]

        title = self._fetch(chat_id)
        self._cache[chat_id] = title
        return title

    def _fetch(self, chat_id: str) -> str:
        resp = self.api_fn("GET", f"/im/v1/chats/{chat_id}")
        if resp.get("code") == 0:
            data = resp.get("data", {})
            if isinstance(data, dict):
                return str(data.get("name") or data.get("chat_id") or chat_id)
        return chat_id
