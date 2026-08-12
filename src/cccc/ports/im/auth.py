"""
IM Bridge authentication — dynamic key-based chat authorization.

Flow:
1. Unauthorized chat sends /subscribe → bridge generates a short-lived key
2. User binds the key via Web API or CLI → chat becomes authorized
3. Authorized chats can use all bridge commands; unauthorized chats are silently dropped
"""

from __future__ import annotations

import json
import secrets
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

from ...kernel.im_state import im_state_lock


# Key time-to-live: 10 minutes.
KEY_TTL_SECONDS = 600
ThreadId = int | str


def normalize_thread_id(value: Any) -> ThreadId:
    """Preserve platform-owned thread identifiers in the shared state shape."""
    if value is None or isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    if isinstance(value, float):
        text = str(value).strip()
        return 0 if not text or text == "0" else text
    if not isinstance(value, str):
        return 0
    text = value.strip()
    if not text or text == "0":
        return 0
    try:
        parsed = int(text)
    except ValueError:
        return text
    if -(2**63) <= parsed <= 2**63 - 1:
        return parsed
    return text


def thread_key(chat_id: str, thread_id: Any = 0) -> str:
    """Return the Python/Rust-compatible persisted key for an IM target."""
    cid = str(chat_id).strip()
    if not cid:
        return ""
    tid = normalize_thread_id(thread_id)
    return f"{cid}:{tid}" if tid != 0 else cid


class KeyManager:
    """Manage pending authorization keys and authorized chat list.

    Storage (under ``state_dir``):
    - ``im_pending_keys.json``   – short-lived keys awaiting bind
    - ``im_authorized_chats.json`` – permanently authorized chats

    All writes are atomic (.tmp → rename).
    """

    def __init__(self, state_dir: Path) -> None:
        self.state_dir = state_dir
        self._pending_path = state_dir / "im_pending_keys.json"
        self._authorized_path = state_dir / "im_authorized_chats.json"

        self._pending: Dict[str, Dict[str, Any]] = {}
        self._authorized: Dict[str, Dict[str, Any]] = {}

        self._load()

    # ------------------------------------------------------------------
    # Persistence
    # ------------------------------------------------------------------

    def _load(self) -> None:
        with im_state_lock(self.state_dir):
            self._load_unlocked()

    def _load_unlocked(self) -> None:
        self._pending = self._read_json(self._pending_path)
        self._authorized = self._read_json(self._authorized_path)

    @staticmethod
    def _read_json(path: Path) -> Dict[str, Any]:
        if not path.exists():
            return {}
        try:
            return json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            return {}

    def _save_pending(self) -> None:
        self._write_json(self._pending_path, self._pending)

    def _save_authorized(self) -> None:
        self._write_json(self._authorized_path, self._authorized)

    def _write_json(self, path: Path, data: Dict[str, Any]) -> None:
        self.state_dir.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".tmp")
        tmp.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
        tmp.replace(path)

    # ------------------------------------------------------------------
    # Key helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _chat_key(chat_id: str, thread_id: ThreadId) -> str:
        return thread_key(chat_id, thread_id)

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def generate_key(self, chat_id: str, thread_id: ThreadId, platform: str) -> str:
        """Create a pending authorization key (``secrets.token_urlsafe(8)``)."""
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            key = secrets.token_urlsafe(8)
            self._pending[key] = {
                "chat_id": str(chat_id),
                "thread_id": normalize_thread_id(thread_id),
                "platform": str(platform or ""),
                "created_at": time.time(),
            }
            self._purge_expired()
            self._save_pending()
            return key

    def get_pending_key(self, key: str) -> Optional[Dict[str, Any]]:
        """Return pending-key metadata if *key* exists and has not expired."""
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            entry = self._pending.get(key)
            if entry is None:
                return None
            if time.time() - float(entry.get("created_at", 0)) > KEY_TTL_SECONDS:
                # Expired — clean up lazily.
                self._pending.pop(key, None)
                self._save_pending()
                return None
            return dict(entry)

    def is_authorized(
        self, chat_id: str, thread_id: ThreadId, platform: str = ""
    ) -> bool:
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            ck = self._chat_key(chat_id, thread_id)
            entry = self._authorized.get(ck)
            if entry is None:
                return False
            requested = str(platform or "").strip().lower()
            if not requested or not isinstance(entry, dict):
                return True
            stored = str(entry.get("platform") or "").strip().lower()
            return not stored or stored == requested

    def authorize(
        self, chat_id: str, thread_id: ThreadId, platform: str, key_used: str
    ) -> None:
        """Mark a chat as authorized and remove the consumed key."""
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            self._store_authorization(
                chat_id,
                thread_id,
                platform,
                authorization_source="binding_key",
                key_used=key_used,
            )
            self._pending.pop(key_used, None)
            self._save_pending()

    def authorize_direct(
        self, chat_id: str, thread_id: ThreadId, platform: str, source: str
    ) -> None:
        """Authorize a chat through an explicit non-key trust flow."""
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            self._store_authorization(
                chat_id,
                thread_id,
                platform,
                authorization_source=str(source or "direct").strip() or "direct",
                key_used="",
            )

    def revoke_direct(self, chat_id: str, thread_id: ThreadId, source: str) -> bool:
        """Revoke a direct authorization only when its source matches."""
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            ck = self._chat_key(chat_id, thread_id)
            entry = self._authorized.get(ck)
            if not isinstance(entry, dict) or str(
                entry.get("authorization_source") or ""
            ) != str(source or ""):
                return False
            del self._authorized[ck]
            self._save_authorized()
            return True

    def _store_authorization(
        self,
        chat_id: str,
        thread_id: ThreadId,
        platform: str,
        *,
        authorization_source: str,
        key_used: str,
    ) -> None:
        ck = self._chat_key(chat_id, thread_id)
        self._authorized[ck] = {
            "chat_id": str(chat_id),
            "thread_id": normalize_thread_id(thread_id),
            "platform": str(platform or ""),
            "authorized_at": time.time(),
            "key_used": str(key_used),
            "authorization_source": str(authorization_source),
        }
        self._save_authorized()

    def revoke(self, chat_id: str, thread_id: ThreadId) -> bool:
        """Revoke authorization. Returns ``True`` if the chat was authorized."""
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            ck = self._chat_key(chat_id, thread_id)
            if ck in self._authorized:
                del self._authorized[ck]
                self._save_authorized()
                return True
            return False

    def list_authorized(self) -> List[Dict[str, Any]]:
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            return list(self._authorized.values())

    def list_pending(self) -> List[Dict[str, Any]]:
        """List pending bind requests (expired keys are purged first)."""
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            removed = self._purge_expired()
            if removed:
                self._save_pending()
            now = time.time()
            items: List[Dict[str, Any]] = []
            for key, entry in self._pending.items():
                created_at = float(entry.get("created_at", 0) or 0)
                expires_at = created_at + KEY_TTL_SECONDS
                items.append(
                    {
                        "key": str(key),
                        "chat_id": str(entry.get("chat_id") or ""),
                        "thread_id": normalize_thread_id(entry.get("thread_id")),
                        "platform": str(entry.get("platform") or ""),
                        "created_at": created_at,
                        "expires_at": expires_at,
                        "expires_in_seconds": max(0, int(expires_at - now)),
                    }
                )
            items.sort(
                key=lambda item: float(item.get("created_at") or 0), reverse=True
            )
            return items

    def reject_pending(self, key: str) -> bool:
        """Reject a pending bind key. Returns True when removed."""
        token = str(key or "").strip()
        if not token:
            return False
        with im_state_lock(self.state_dir):
            self._load_unlocked()
            removed = self._purge_expired()
            existed = token in self._pending
            if existed:
                self._pending.pop(token, None)
                self._save_pending()
                return True
            if removed:
                self._save_pending()
            return False

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _purge_expired(self) -> bool:
        """Remove expired pending keys (best-effort, no save)."""
        now = time.time()
        expired = [
            k
            for k, v in self._pending.items()
            if now - float(v.get("created_at", 0)) > KEY_TTL_SECONDS
        ]
        for k in expired:
            del self._pending[k]
        return bool(expired)
