"""
Subscriber management for IM Bridge.

Manages which chats are subscribed to receive messages from a group.
"""

from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

from ...util.conv import coerce_bool


def _safe_int(value: Any, default: int = 0) -> int:
    try:
        return int(value)
    except Exception:
        return int(default)


class Subscriber:
    """Represents a subscribed chat."""

    def __init__(
        self,
        chat_id: str,
        subscribed: bool = True,
        verbose: bool = True,
        subscribed_at: Optional[str] = None,
        chat_title: str = "",
        thread_id: int = 0,
        platform: str = "",
    ):
        self.chat_id = str(chat_id)
        self.subscribed = subscribed
        self.verbose = verbose  # Default True: show all messages including agent-to-agent
        self.subscribed_at = subscribed_at or time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.chat_title = chat_title
        self.thread_id = int(thread_id or 0)
        self.platform = str(platform or "").strip().lower()  # Platform that created this subscription

    def to_dict(self) -> Dict[str, Any]:
        return {
            "subscribed": self.subscribed,
            "verbose": self.verbose,
            "subscribed_at": self.subscribed_at,
            "chat_title": self.chat_title,
            "thread_id": self.thread_id,
            "platform": self.platform,
        }

    @classmethod
    def from_dict(cls, chat_id: str, data: Dict[str, Any]) -> "Subscriber":
        return cls(
            chat_id=chat_id,
            subscribed=coerce_bool(data.get("subscribed"), default=True),
            verbose=coerce_bool(data.get("verbose"), default=True),
            subscribed_at=data.get("subscribed_at"),
            chat_title=str(data.get("chat_title", "")),
            thread_id=_safe_int(data.get("thread_id"), 0),
            platform=str(data.get("platform") or ""),
        )


class SubscriberManager:
    """Manages subscribers for a group's IM bridge."""

    def __init__(self, state_dir: Path):
        self.state_dir = state_dir
        self.subscribers_path = state_dir / "im_subscribers.json"
        self._subscribers: Dict[str, Subscriber] = {}
        self._load()

    def _key(self, chat_id: str, thread_id: int = 0) -> str:
        cid = str(chat_id)
        tid = int(thread_id or 0)
        return f"{cid}:{tid}" if tid > 0 else cid

    def _load(self) -> None:
        """Load subscribers from disk."""
        if not self.subscribers_path.exists():
            self._subscribers = {}
            return

        try:
            data = json.loads(self.subscribers_path.read_text(encoding="utf-8"))
            self._subscribers = {}
            for raw_key, sub_data in data.items():
                if not isinstance(raw_key, str):
                    continue
                key = raw_key.strip()
                if not key:
                    continue

                # Support both legacy keys ("<chat_id>") and topic keys ("<chat_id>:<thread_id>").
                chat_id = key
                thread_id = 0
                if ":" in key:
                    head, tail = key.rsplit(":", 1)
                    try:
                        thread_id = int(tail)
                        chat_id = head
                    except Exception:
                        chat_id = key
                        thread_id = 0

                if not isinstance(sub_data, dict):
                    continue

                # If thread_id was encoded in the key, prefer it (but still allow stored thread_id field).
                stored_thread_id = _safe_int(sub_data.get("thread_id"), 0)
                effective_thread_id = thread_id or stored_thread_id

                sub = Subscriber.from_dict(chat_id, sub_data)
                sub.thread_id = effective_thread_id
                self._subscribers[self._key(sub.chat_id, sub.thread_id)] = sub
        except Exception:
            self._subscribers = {}

    def _save(self) -> None:
        """Save subscribers to disk."""
        self.state_dir.mkdir(parents=True, exist_ok=True)
        data = {key: sub.to_dict() for key, sub in self._subscribers.items()}
        tmp = self.subscribers_path.with_suffix(".tmp")
        tmp.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
        tmp.replace(self.subscribers_path)

    def subscribe(self, chat_id: str, chat_title: str = "", thread_id: int = 0, platform: str = "") -> Subscriber:
        """Subscribe a chat. Returns the subscriber."""
        key = self._key(chat_id, thread_id)
        if key in self._subscribers:
            sub = self._subscribers[key]
            sub.subscribed = True
            if chat_title:
                sub.chat_title = chat_title
            if platform:
                sub.platform = str(platform).strip().lower()
        else:
            sub = Subscriber(chat_id=str(chat_id), chat_title=chat_title, thread_id=thread_id, platform=platform)
            self._subscribers[key] = sub
        self._save()
        return sub

    def unsubscribe(self, chat_id: str, thread_id: int = 0) -> bool:
        """Unsubscribe a chat. Returns True if was subscribed."""
        key = self._key(chat_id, thread_id)
        if key in self._subscribers:
            self._subscribers[key].subscribed = False
            self._save()
            return True
        return False

    def set_verbose(self, chat_id: str, verbose: bool, thread_id: int = 0) -> bool:
        """Set verbose mode for a chat. Returns True if chat exists."""
        key = self._key(chat_id, thread_id)
        if key in self._subscribers:
            self._subscribers[key].verbose = verbose
            self._save()
            return True
        return False

    def toggle_verbose(self, chat_id: str, thread_id: int = 0) -> Optional[bool]:
        """Toggle verbose mode. Returns new value or None if not subscribed."""
        key = self._key(chat_id, thread_id)
        if key in self._subscribers:
            sub = self._subscribers[key]
            sub.verbose = not sub.verbose
            self._save()
            return sub.verbose
        return None

    def is_subscribed(self, chat_id: str, thread_id: int = 0) -> bool:
        """Check if a chat is subscribed."""
        sub = self._subscribers.get(self._key(chat_id, thread_id))
        return sub is not None and sub.subscribed

    def is_verbose(self, chat_id: str, thread_id: int = 0) -> bool:
        """Check if a chat has verbose mode enabled."""
        sub = self._subscribers.get(self._key(chat_id, thread_id))
        return sub is not None and sub.verbose

    def get_subscriber(self, chat_id: str, thread_id: int = 0) -> Optional[Subscriber]:
        """Get subscriber info."""
        return self._subscribers.get(self._key(chat_id, thread_id))

    def get_subscribed_targets(self, platform: Optional[str] = None) -> List[Subscriber]:
        """Get list of subscribed chat targets, optionally filtered by platform.

        Args:
            platform: If provided, only return subscribers for this platform.
                      Subscribers with empty platform match all platforms (backward compat).
        """
        result = []
        platform_filter = str(platform or "").strip().lower()
        for sub in self._subscribers.values():
            if not sub.subscribed:
                continue
            # If no platform filter, return all
            if not platform_filter:
                result.append(sub)
                continue
            # Subscriber with empty platform matches all (backward compat for legacy data)
            if not sub.platform:
                result.append(sub)
                continue
            # Match only if platform matches
            if sub.platform == platform_filter:
                result.append(sub)
        return result

    def count(self) -> int:
        """Count subscribed chats."""
        return len(self.get_subscribed_targets())
