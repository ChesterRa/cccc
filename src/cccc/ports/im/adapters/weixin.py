"""
Weixin (personal WeChat) adapter for CCCC IM Bridge.

Backed by ``wechatbot-sdk`` and a background asyncio loop. The SDK persists
login credentials, while this adapter additionally persists context tokens so
proactive text/file/image sends still work after bridge restarts.
"""

from __future__ import annotations

import asyncio
import json
import logging
import tempfile
import threading
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

from .base import IMAdapter

logger = logging.getLogger(__name__)

DEFAULT_MAX_CHARS = 4000
DEFAULT_MAX_LINES = 64


class WeixinAdapter(IMAdapter):
    """Personal WeChat adapter backed by ``wechatbot-sdk``."""

    platform = "weixin"
    capabilities = {
        "text_in": "yes",
        "text_out": "yes",
        "files_in": "partial",
        "files_out": "partial",
        "threads": "no",
        "reactions": "no",
        "typing": "no",
        "streaming": "no",
        "voice_in": "partial",
        "markdown": "partial",
    }
    capability_notes = {
        "voice_in": "voice media is preserved as an attachment when the SDK exposes it",
        "files_out": "image/file sending depends on the local Weixin bridge backend",
    }

    def __init__(
        self,
        *,
        account_id: str = "",
        api_base_url: str = "",
        log_path: Optional[Path] = None,
        cred_path: Optional[Path] = None,
        context_cache_path: Optional[Path] = None,
        max_chars: int = DEFAULT_MAX_CHARS,
        max_lines: int = DEFAULT_MAX_LINES,
    ):
        self.account_id = str(account_id or "").strip()
        self.api_base_url = str(api_base_url or "").strip()
        self.log_path = log_path
        self.max_chars = int(max_chars)
        self.max_lines = int(max_lines)

        state_dir = (
            log_path.parent
            if isinstance(log_path, Path)
            else Path.cwd()
        )
        self.cred_path = cred_path or (state_dir / "im_weixin_credentials.json")
        self.context_cache_path = context_cache_path or (state_dir / "im_weixin_context_tokens.json")

        self._queue_lock = threading.Lock()
        self._message_queue: List[Dict[str, Any]] = []
        self._context_lock = threading.Lock()
        self._context_tokens: Dict[str, str] = self._load_context_tokens()
        self._connected = False
        self._ready = threading.Event()
        self._connect_error: Optional[str] = None

        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._loop_thread: Optional[threading.Thread] = None
        self._receiver_task: Any = None
        self._bot: Any = None

    def _log(self, msg: str) -> None:
        line = f"[{time.strftime('%Y-%m-%d %H:%M:%S')}] [weixin] {msg}"
        logger.info(line)
        if self.log_path:
            try:
                self.log_path.parent.mkdir(parents=True, exist_ok=True)
                with open(self.log_path, "a", encoding="utf-8") as f:
                    f.write(line + "\n")
            except Exception:
                pass

    def _compose_safe(self, text: str) -> str:
        lines = str(text or "").split("\n")
        if len(lines) > self.max_lines:
            lines = lines[: self.max_lines]
            lines.append("... (truncated)")
        out = "\n".join(lines)
        if len(out) > self.max_chars:
            out = out[: self.max_chars - 20] + "\n... (truncated)"
        return out

    def _load_context_tokens(self) -> Dict[str, str]:
        try:
            loaded = json.loads(self.context_cache_path.read_text(encoding="utf-8"))
        except FileNotFoundError:
            return {}
        except Exception as e:
            self._log(f"[context] failed to load cache: {e}")
            return {}
        if not isinstance(loaded, dict):
            return {}
        out: Dict[str, str] = {}
        for key, value in loaded.items():
            chat_id = str(key or "").strip()
            token = str(value or "").strip()
            if chat_id and token:
                out[chat_id] = token
        return out

    def _save_context_tokens(self) -> None:
        try:
            self.context_cache_path.parent.mkdir(parents=True, exist_ok=True)
            payload = dict(sorted(self._context_tokens.items()))
            self.context_cache_path.write_text(
                json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
                encoding="utf-8",
            )
        except Exception as e:
            self._log(f"[context] failed to persist cache: {e}")

    def _remember_context_token(self, chat_id: str, token: str) -> None:
        normalized_chat = str(chat_id or "").strip()
        normalized_token = str(token or "").strip()
        if not normalized_chat or not normalized_token:
            return
        changed = False
        with self._context_lock:
            if self._context_tokens.get(normalized_chat) != normalized_token:
                self._context_tokens[normalized_chat] = normalized_token
                changed = True
            self._merge_bot_context_tokens({normalized_chat: normalized_token})
            if changed:
                self._save_context_tokens()

    def _merge_bot_context_tokens(self, tokens: Dict[str, str]) -> None:
        bot = self._bot
        if bot is None or not tokens:
            return
        target = getattr(bot, "_context_tokens", None)
        if not isinstance(target, dict):
            try:
                setattr(bot, "_context_tokens", dict(tokens))
            except Exception as e:
                self._log(f"[context] failed to initialize sdk cache: {e}")
            return
        target.update(tokens)

    def _run_async(self, coro: Any, timeout: float = 30.0) -> Any:
        """Submit a coroutine to the background event loop and wait for result."""
        if not self._loop or not self._loop.is_running():
            raise RuntimeError("weixin async event loop is not running")
        future = asyncio.run_coroutine_threadsafe(coro, self._loop)
        return future.result(timeout=timeout)

    async def _download_media_to_temp(self, msg: Any) -> List[Dict[str, Any]]:
        bot = self._bot
        if bot is None:
            return []
        try:
            media = await bot.download(msg)
        except Exception as e:
            self._log(f"[media] download failed: {e}")
            return []
        if media is None:
            return []

        suffix_map = {
            "image": ".jpg",
            "video": ".mp4",
            "voice": ".silk",
            "file": "",
        }
        suffix = suffix_map.get(str(media.type or "").strip(), "")
        filename = str(media.file_name or "").strip()
        if filename:
            suffix = Path(filename).suffix or suffix
        fd, path = tempfile.mkstemp(suffix=suffix, prefix="wx_media_")
        with open(fd, "wb") as f:
            f.write(media.data)
        return [{
            "type": media.type,
            "kind": "image" if media.type == "image" else "file",
            "file_path": path,
            "file_name": filename or (Path(path).name),
            "mime_type": self._guess_mime_type(filename, media.type),
            "provider": "weixin",
        }]

    def _guess_mime_type(self, filename: str, media_type: str) -> str:
        suffix = Path(filename or "").suffix.lower()
        if media_type == "image":
            return {
                ".png": "image/png",
                ".gif": "image/gif",
                ".webp": "image/webp",
                ".bmp": "image/bmp",
            }.get(suffix, "image/jpeg")
        if media_type == "video":
            return {
                ".mov": "video/quicktime",
                ".mkv": "video/x-matroska",
            }.get(suffix, "video/mp4")
        if media_type == "voice":
            return "audio/silk"
        return "application/octet-stream"

    async def _handle_incoming(self, msg: Any) -> None:
        chat_id = str(getattr(msg, "user_id", "") or "").strip()
        if not chat_id:
            return

        context_token = str(getattr(msg, "_context_token", "") or "").strip()
        if context_token:
            self._remember_context_token(chat_id, context_token)

        attachments = await self._download_media_to_temp(msg)
        message_id = ""
        raw = getattr(msg, "raw", None)
        if isinstance(raw, dict):
            message_id = str(raw.get("client_id") or raw.get("msg_id") or raw.get("id") or "").strip()
        if not message_id:
            message_id = f"wx_{int(time.time() * 1000)}"

        normalized: Dict[str, Any] = {
            "chat_id": chat_id,
            "chat_title": chat_id,
            "chat_type": "p2p",
            "routed": True,
            "thread_id": 0,
            "text": str(getattr(msg, "text", "") or ""),
            "attachments": attachments,
            "from_user": chat_id,
            "message_id": message_id,
            "timestamp": time.time(),
        }
        with self._queue_lock:
            self._message_queue.append(normalized)

    async def _async_connect(self) -> None:
        from wechatbot import WeChatBot
        from wechatbot.auth import load_credentials

        creds = await load_credentials(self.cred_path)
        if creds is None:
            raise RuntimeError(f"weixin not logged in (missing credentials: {self.cred_path})")

        bot = WeChatBot(
            base_url=self.api_base_url or None,
            cred_path=str(self.cred_path),
            on_error=lambda err: self._log(f"[bot] {type(err).__name__}: {err}"),
        )
        bot._credentials = creds
        bot._base_url = str(creds.base_url or self.api_base_url or "").strip()
        with self._context_lock:
            self._bot = bot
            self._merge_bot_context_tokens(dict(self._context_tokens))

        @bot.on_message
        async def _on_message(msg: Any) -> None:
            await self._handle_incoming(msg)

        self._connected = True
        self._receiver_task = asyncio.create_task(bot.start())

    def _run_loop(self) -> None:
        """Thread target: run the asyncio event loop."""
        self._loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self._loop)
        try:
            self._loop.run_until_complete(self._async_connect())
            self._connect_error = None
            self._ready.set()
            self._loop.run_forever()
        except Exception as e:
            self._connect_error = str(e)
            self._ready.set()
        finally:
            try:
                self._loop.run_until_complete(self._loop.shutdown_asyncgens())
            except Exception:
                pass
            self._loop.close()

    def connect(self) -> bool:
        self._disable_proxies()
        self._ready.clear()
        self._connect_error = None

        self._loop_thread = threading.Thread(target=self._run_loop, daemon=True)
        self._loop_thread.start()

        if not self._ready.wait(timeout=15.0):
            self._log("[connect] timeout waiting for transport readiness")
            self.disconnect()
            return False
        if self._connect_error:
            self._log(f"[connect] transport failed: {self._connect_error}")
            self.disconnect()
            return False

        self._connected = True
        self._log("[connect] weixin transport connected")
        return True

    def disconnect(self) -> None:
        self._connected = False
        loop = self._loop
        bot = self._bot
        receiver = self._receiver_task

        if loop and loop.is_running():
            async def _shutdown() -> None:
                if bot is not None:
                    try:
                        bot.stop()
                    except Exception:
                        pass
                if receiver and not receiver.done():
                    receiver.cancel()
                    try:
                        await receiver
                    except (asyncio.CancelledError, Exception):
                        pass
                loop.stop()

            future = asyncio.run_coroutine_threadsafe(_shutdown(), loop)
            try:
                future.result(timeout=8.0)
            except Exception:
                pass

        if self._loop_thread and self._loop_thread.is_alive():
            self._loop_thread.join(timeout=3.0)

        self._loop = None
        self._loop_thread = None
        self._bot = None
        self._receiver_task = None

        with self._queue_lock:
            self._message_queue.clear()

    def poll(self) -> List[Dict[str, Any]]:
        with self._queue_lock:
            items = list(self._message_queue)
            self._message_queue.clear()
        return items

    def send_message(
        self,
        chat_id: str,
        text: str,
        thread_id: Optional[int] = None,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        _ = thread_id
        _ = mention_user_ids
        if not text:
            return True
        if not self._connected or self._bot is None:
            return False

        chat_id = str(chat_id or "").strip()
        safe_text = self._compose_safe(text)

        try:
            self._run_async(self._bot.send(chat_id, safe_text))
            return True
        except Exception as e:
            self._log(f"[send] failed for chat={chat_id}: {e}")
            return False

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
        _ = thread_id
        _ = mention_user_ids
        if not self._connected or self._bot is None:
            return False

        chat_id = str(chat_id or "").strip()

        try:
            file_data = file_path.read_bytes()
            suffix = file_path.suffix.lower()
            if suffix in (".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp"):
                content: Dict[str, Any] = {"image": file_data}
            elif suffix in (".mp4", ".avi", ".mov", ".mkv"):
                content = {"video": file_data}
            else:
                content = {"file": file_data, "file_name": filename or file_path.name}

            self._run_async(self._bot.send_media(chat_id, content))

            if caption:
                self._run_async(self._bot.send(chat_id, self._compose_safe(caption)))
            return True
        except Exception as e:
            self._log(f"[send_file] failed for chat={chat_id}: {e}")
            return False

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        file_path = str(attachment.get("file_path") or "").strip()
        if not file_path:
            raise ValueError("weixin attachment has no file_path (media not pre-downloaded)")
        return Path(file_path).read_bytes()

    def get_chat_title(self, chat_id: str) -> str:
        return chat_id
