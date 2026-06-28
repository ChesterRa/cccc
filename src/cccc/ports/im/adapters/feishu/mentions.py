"""Feishu mention routing helpers."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass
class FeishuBotIdentity:
    open_id: str = ""
    user_id: str = ""
    name: str = ""

    @classmethod
    def from_values(cls, *, open_id: Any = "", user_id: Any = "", name: Any = "") -> "FeishuBotIdentity":
        return cls(
            open_id=str(open_id or "").strip(),
            user_id=str(user_id or "").strip(),
            name=str(name or "").strip(),
        )

    @property
    def has_id(self) -> bool:
        return bool(self.open_id or self.user_id)

    @property
    def has_matchable_value(self) -> bool:
        return bool(self.has_id or self.name)


class FeishuMentionRouter:
    """Decide whether a Feishu mentions list targets this bot."""

    def __init__(self, identity: FeishuBotIdentity):
        self.identity = identity

    def mentions_bot(self, mentions: Any) -> bool:
        if not isinstance(mentions, list) or not mentions:
            return False
        if not self.identity.has_matchable_value:
            return False

        for item in mentions:
            if not isinstance(item, dict):
                continue
            mention_id = item.get("id") if isinstance(item.get("id"), dict) else {}
            open_id = str(mention_id.get("open_id") or "").strip()
            user_id = str(mention_id.get("user_id") or "").strip()
            name = str(item.get("name") or "").strip()

            if self.identity.open_id and open_id and open_id == self.identity.open_id:
                return True
            if self.identity.user_id and user_id and user_id == self.identity.user_id:
                return True
            if self.identity.has_id:
                continue
            if self.identity.name and name and name == self.identity.name:
                return True
        return False
