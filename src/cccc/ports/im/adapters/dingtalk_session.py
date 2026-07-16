"""Persistent DingTalk conversation reply state."""

from __future__ import annotations

import json
import threading
import time
from pathlib import Path
from typing import Any, Dict, Optional


class DingTalkConversationStore:
    """Store DingTalk routing metadata and short-lived reply webhooks."""

    def __init__(self, path: Optional[Path]):
        self.path = path
        self._lock = threading.RLock()
        self._meta: Dict[str, Dict[str, str]] = {}
        self._webhooks: Dict[str, tuple[str, float]] = {}
        self._load()

    def _load(self) -> None:
        with self._lock:
            if self.path is None or not self.path.exists():
                return
            try:
                data = json.loads(self.path.read_text(encoding="utf-8"))
                conversations = data.get("conversations") if isinstance(data, dict) else None
                if not isinstance(conversations, dict):
                    return
                now = time.time()
                for raw_chat_id, raw_entry in conversations.items():
                    chat_id = str(raw_chat_id or "").strip()
                    if not chat_id or not isinstance(raw_entry, dict):
                        continue
                    chat_type = str(raw_entry.get("chat_type") or "").strip().lower()
                    user_id = str(raw_entry.get("user_id") or "").strip()
                    if chat_type or user_id:
                        self._meta[chat_id] = {"chat_type": chat_type, "user_id": user_id}

                    webhook_url = str(raw_entry.get("session_webhook") or "").strip()
                    try:
                        expires_at = float(raw_entry.get("session_webhook_expires_at") or 0.0)
                    except Exception:
                        expires_at = 0.0
                    if webhook_url and expires_at > now:
                        self._webhooks[chat_id] = (webhook_url, expires_at)
            except Exception:
                self._meta = {}
                self._webhooks = {}

    def _save(self) -> None:
        with self._lock:
            if self.path is None:
                return
            now = time.time()
            chat_ids = set(self._meta.keys()) | set(self._webhooks.keys())
            conversations: Dict[str, Dict[str, Any]] = {}
            for chat_id in chat_ids:
                meta = self._meta.get(chat_id) or {}
                entry: Dict[str, Any] = {}
                chat_type = str(meta.get("chat_type") or "").strip().lower()
                user_id = str(meta.get("user_id") or "").strip()
                if chat_type:
                    entry["chat_type"] = chat_type
                if user_id:
                    entry["user_id"] = user_id

                webhook = self._webhooks.get(chat_id)
                if webhook:
                    webhook_url, expires_at = webhook
                    if expires_at > now:
                        entry["session_webhook"] = webhook_url
                        entry["session_webhook_expires_at"] = expires_at
                if entry:
                    conversations[chat_id] = entry

            self.path.parent.mkdir(parents=True, exist_ok=True)
            tmp = self.path.with_name(f"{self.path.name}.{threading.get_ident()}.{time.monotonic_ns()}.tmp")
            try:
                tmp.write_text(
                    json.dumps({"conversations": conversations}, ensure_ascii=False, indent=2),
                    encoding="utf-8",
                )
                tmp.replace(self.path)
            finally:
                try:
                    tmp.unlink()
                except Exception:
                    pass

    def remember(
        self,
        conversation_id: str,
        *,
        chat_type: str,
        user_id: str = "",
        session_webhook: str = "",
        session_expires: Any = 0,
    ) -> None:
        chat_id = str(conversation_id or "").strip()
        if not chat_id:
            return

        with self._lock:
            normalized_chat_type = str(chat_type or "").strip().lower()
            normalized_user_id = str(user_id or "").strip()
            if normalized_chat_type in ("p2p", "group") or normalized_user_id:
                current = dict(self._meta.get(chat_id) or {})
                if normalized_chat_type in ("p2p", "group"):
                    current["chat_type"] = normalized_chat_type
                if normalized_user_id:
                    current["user_id"] = normalized_user_id
                self._meta[chat_id] = current

            if session_webhook:
                try:
                    expires_raw = float(session_expires or 0.0)
                except Exception:
                    expires_raw = 0.0
                expires_at = expires_raw / 1000.0 if expires_raw > 1e10 else expires_raw
                self._webhooks[chat_id] = (str(session_webhook), expires_at)

            self._save()

    def chat_type(self, conversation_id: str) -> str:
        with self._lock:
            meta = self._meta.get(str(conversation_id or "").strip()) or {}
            return str(meta.get("chat_type") or "").strip().lower()

    def user_id(self, conversation_id: str) -> str:
        with self._lock:
            meta = self._meta.get(str(conversation_id or "").strip()) or {}
            return str(meta.get("user_id") or "").strip()

    def is_group(self, conversation_id: str) -> bool:
        chat_type = self.chat_type(conversation_id)
        if chat_type == "group":
            return True
        if chat_type == "p2p":
            return False
        return str(conversation_id or "").startswith("cid")

    def live_webhook(self, conversation_id: str) -> Optional[str]:
        with self._lock:
            chat_id = str(conversation_id or "").strip()
            webhook = self._webhooks.get(chat_id)
            if not webhook:
                return None
            webhook_url, expires_at = webhook
            if time.time() < expires_at:
                return webhook_url
            self.forget_webhook(chat_id)
            return None

    def forget_webhook(self, conversation_id: str) -> None:
        with self._lock:
            chat_id = str(conversation_id or "").strip()
            if chat_id in self._webhooks:
                del self._webhooks[chat_id]
                self._save()

    def webhook_entry(self, conversation_id: str) -> Optional[tuple[str, float]]:
        with self._lock:
            chat_id = str(conversation_id or "").strip()
            webhook = self._webhooks.get(chat_id)
            if not webhook:
                return None
            webhook_url, expires_at = webhook
            if time.time() < expires_at:
                return webhook_url, expires_at
            self.forget_webhook(chat_id)
            return None
