"""
Base class for IM platform adapters.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional


class IMAdapter(ABC):
    """
    Abstract base class for IM platform adapters.

    Each adapter handles:
    - Connecting to the platform
    - Receiving messages (inbound)
    - Sending messages (outbound)
    - Platform-specific formatting
    """

    platform: str = "unknown"

    def _log(self, msg: str) -> None:
        """Log a message. Subclasses should override with actual logging."""
        pass

    def _disable_proxies(self) -> None:
        """
        Disable all proxies for WebSocket-based SDKs.

        Many IM SDKs (Feishu, DingTalk) use websockets which don't support
        SOCKS proxy without python-socks. Setting no_proxy=* and clearing
        proxy environment variables ensures reliable WebSocket connections.
        """
        import os
        raw = str(os.getenv("CCCC_IM_DISABLE_PROXIES", "")).strip().lower()
        if raw in ("0", "false", "no", "off"):
            self._log("[connect] Keeping proxy environment variables (CCCC_IM_DISABLE_PROXIES=0)")
            return

        os.environ["no_proxy"] = "*"
        os.environ["NO_PROXY"] = "*"
        self._log("[connect] Set no_proxy=* to bypass all proxies")

        proxy_vars = ["ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy",
                      "HTTPS_PROXY", "https_proxy", "SOCKS_PROXY", "socks_proxy"]
        for var in proxy_vars:
            if var in os.environ:
                del os.environ[var]
                self._log(f"[connect] Cleared {var} environment variable")

    @abstractmethod
    def connect(self) -> bool:
        """
        Initialize connection to the platform.
        Returns True if successful.
        """
        pass

    @abstractmethod
    def disconnect(self) -> None:
        """Disconnect from the platform."""
        pass

    @abstractmethod
    def poll(self) -> List[Dict[str, Any]]:
        """
        Poll for new messages.

        Returns list of message dicts with at least:
        - chat_id: str
        - text: str
        - from_user: str (username or display name)
        - message_id: str (platform message ID; type varies by platform)
        """
        pass

    @abstractmethod
    def send_message(self, chat_id: str, text: str, thread_id: Optional[int] = None) -> bool:
        """
        Send a message to a chat.
        Returns True if successful.
        """
        pass

    @abstractmethod
    def get_chat_title(self, chat_id: str) -> str:
        """Get the title/name of a chat."""
        pass

    def format_outbound(self, by: str, to: List[str], text: str, is_system: bool = False) -> str:
        """
        Format an outbound message for display.

        Default implementation - adapters can override for platform-specific formatting.
        """
        if is_system:
            return f"[SYSTEM] {text}"

        if to and "user" not in to:
            # Agent to agent message
            to_str = ", ".join(to)
            return f"[{by} → {to_str}] {text}"
        else:
            # Agent to user message
            return f"[{by}] {text}"

    def send_chat_action(self, chat_id: str, action: str = "typing") -> bool:
        """Send a chat action indicator. Default: no-op."""
        return False

    def add_reaction(self, message_id: str, emoji_type: str) -> Optional[str]:
        """
        Add an emoji reaction to a message.

        Returns reaction_id on success (for later removal), None on failure.
        Default: no-op (not all platforms support reactions).
        """
        return None

    def remove_reaction(self, message_id: str, reaction_id: str) -> bool:
        """
        Remove a previously added emoji reaction.

        Returns True on success.
        Default: no-op.
        """
        return False

    def download_attachment(self, attachment: Dict[str, Any]) -> bytes:
        """Download an inbound attachment to bytes (platform-specific)."""
        raise NotImplementedError

    def send_file(
        self,
        chat_id: str,
        *,
        file_path: Path,
        filename: str,
        caption: str = "",
        thread_id: Optional[int] = None,
    ) -> bool:
        """Send a file to a chat (platform-specific)."""
        _ = thread_id
        _ = caption
        _ = filename
        _ = file_path
        return False

    def summarize(self, text: str, max_chars: int = 900, max_lines: int = 8) -> str:
        """
        Summarize text for IM display.

        - Normalize newlines
        - Collapse multiple blank lines
        - Limit lines and characters
        """
        if not text:
            return ""

        # Normalize
        t = text.replace("\r\n", "\n").replace("\r", "\n").replace("\t", "  ")
        lines = [ln.rstrip() for ln in t.split("\n")]

        # Strip leading/trailing empty lines
        while lines and not lines[0].strip():
            lines.pop(0)
        while lines and not lines[-1].strip():
            lines.pop()

        # Collapse multiple blank lines
        kept = []
        empty_count = 0
        for ln in lines:
            if not ln.strip():
                empty_count += 1
                if empty_count <= 1:
                    kept.append("")
            else:
                empty_count = 0
                kept.append(ln)

        # Limit lines
        kept = kept[:max_lines]
        out = "\n".join(kept).strip()

        # Limit characters
        if len(out) > max_chars:
            out = out[: max(0, max_chars - 1)] + "…"

        return out
