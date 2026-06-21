"""
Base class for IM platform adapters.
"""

from __future__ import annotations

from abc import ABC, abstractmethod
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, TypedDict


DEFAULT_IM_CAPABILITIES: Dict[str, str] = {
    "text_in": "no",
    "text_out": "no",
    "files_in": "no",
    "files_out": "no",
    "threads": "no",
    "reactions": "no",
    "typing": "no",
    "streaming": "no",
    "voice_in": "no",
    "markdown": "no",
}

_CAPABILITY_STATES = frozenset({"yes", "partial", "no"})


def normalize_im_capabilities(capabilities: Optional[Dict[str, str]] = None) -> Dict[str, str]:
    """Return a stable capability map for IM status/diagnostics."""
    merged = dict(DEFAULT_IM_CAPABILITIES)
    if isinstance(capabilities, dict):
        for key, value in capabilities.items():
            if key not in merged:
                continue
            normalized = str(value or "").strip().lower()
            merged[key] = normalized if normalized in _CAPABILITY_STATES else "no"
    return merged


class OutboundStreamHandle(TypedDict):
    """Handle returned by begin_stream for subsequent updates."""

    stream_id: str
    platform_handle: Any  # Platform-specific handle (e.g., DingTalk card instance ID)


class IMProcessingOutcome(str, Enum):
    """Result of processing an inbound IM message."""

    SUCCESS = "success"
    FAILURE = "failure"
    CANCELLED = "cancelled"


@dataclass(frozen=True)
class IMProcessingContext:
    """Platform context for best-effort processing feedback."""

    chat_id: str
    thread_id: int = 0
    message_id: str = ""
    platform: str = "unknown"


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
    capabilities: Dict[str, str] = {}
    capability_notes: Dict[str, str] = {}

    def get_capabilities(self) -> Dict[str, Any]:
        """Expose adapter capabilities for user-facing diagnostics."""
        return {
            "platform": str(getattr(self, "platform", "") or "unknown"),
            "features": normalize_im_capabilities(getattr(self, "capabilities", {})),
            "notes": dict(getattr(self, "capability_notes", {}) or {}),
        }

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
    def send_message(
        self,
        chat_id: str,
        text: str,
        thread_id: Optional[int] = None,
        *,
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        """
        Send a message to a chat.
        Returns True if successful.
        """
        _ = mention_user_ids
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

    def add_reaction(self, message_id: str, emoji_type: str = "") -> Optional[str]:
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

    def on_processing_start(self, context: IMProcessingContext) -> Optional[str]:
        """Start platform feedback for an accepted inbound message."""
        if context.message_id:
            reaction_id = self.add_reaction(context.message_id, "👀")
            if reaction_id:
                return f"reaction:{reaction_id}"
        if self.send_chat_action(context.chat_id, "typing"):
            return "typing"
        return None

    def on_processing_complete(
        self,
        context: IMProcessingContext,
        outcome: IMProcessingOutcome,
        handle: Optional[str],
    ) -> None:
        """Complete platform feedback for a previously accepted inbound message."""
        _ = outcome
        if not handle or not handle.startswith("reaction:"):
            return
        reaction_id = handle.removeprefix("reaction:")
        if context.message_id and reaction_id:
            self.remove_reaction(context.message_id, reaction_id)

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
        mention_user_ids: Optional[List[str]] = None,
    ) -> bool:
        """Send a file to a chat (platform-specific)."""
        _ = thread_id
        _ = caption
        _ = filename
        _ = file_path
        _ = mention_user_ids
        return False

    def begin_stream(self, chat_id: str, stream_id: str, *, text: str = "", thread_id: Optional[int] = None) -> Optional[OutboundStreamHandle]:
        """Begin a streaming message. Default: no-op (platform does not support streaming)."""
        return None

    def update_stream(self, handle: OutboundStreamHandle, *, text: str = "", seq: int = 0) -> bool:
        """Update an in-progress streaming message. Default: no-op."""
        return False

    def end_stream(self, handle: OutboundStreamHandle, *, text: str = "") -> bool:
        """Finalize a streaming message. Default: no-op."""
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
