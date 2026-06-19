from __future__ import annotations

from typing import Any, Dict, List, Literal, Optional

from pydantic import BaseModel, ConfigDict, Field


SUGGESTED_USER_MESSAGE_MAX_CHARS = 4000


class Reference(BaseModel):
    """Reference to a file/commit/URL/text snippet."""

    kind: Literal["file", "url", "commit", "text"] = "url"
    url: str = ""
    path: str = ""
    title: str = ""
    sha: str = ""
    bytes: int = 0

    model_config = ConfigDict(extra="allow")


class Attachment(BaseModel):
    """Attachment metadata (payload stored under blobs/)."""

    kind: Literal["text", "image", "file"] = "file"
    path: str = ""
    title: str = ""
    mime_type: str = ""
    bytes: int = 0
    sha256: str = ""

    model_config = ConfigDict(extra="allow")


class ChatMessageData(BaseModel):
    """IM-style chat message."""

    # Core content
    text: str
    format: Literal["plain", "markdown"] = "plain"

    # Priority / workflow semantics
    priority: Literal["normal", "attention"] = "normal"
    reply_required: bool = False

    # IM semantics
    to: List[str] = Field(default_factory=list)  # @mentions (empty = broadcast)
    reply_to: Optional[str] = None  # The replied-to message event_id
    quote_text: Optional[str] = None  # Quoted snippet for display
    source_platform: Optional[str] = None  # External IM source platform, e.g. dingtalk
    source_user_name: Optional[str] = None  # External IM sender display name
    source_user_id: Optional[str] = None  # External IM sender platform user id
    mention_user_ids: Optional[List[str]] = None  # External IM real-mention targets
    sender_title: Optional[str] = None  # Immutable sender title snapshot for message rendering
    sender_runtime: Optional[str] = None  # Immutable sender runtime snapshot for message rendering
    sender_avatar_path: Optional[str] = None  # Immutable blob-backed sender avatar path

    # Cross-group provenance (for relays/forwarding)
    src_group_id: Optional[str] = None
    src_event_id: Optional[str] = None

    # Cross-group destination metadata (for "send to other group" source messages)
    dst_group_id: Optional[str] = None
    dst_to: Optional[List[str]] = None

    # Attachments and references
    refs: List[Dict[str, Any]] = Field(default_factory=list)
    attachments: List[Dict[str, Any]] = Field(default_factory=list)

    # Reserved
    thread: str = ""  # Topic/thread ID (future)

    # Streaming
    stream_id: Optional[str] = None  # Links final message to its stream
    pending_event_id: Optional[str] = None  # Stable turn-scoped id to reconcile transient UI bubbles

    # Metadata
    client_id: Optional[str] = None  # Client-generated idempotency key
    suggested_user_message: Optional[str] = Field(
        default=None,
        max_length=SUGGESTED_USER_MESSAGE_MAX_CHARS,
    )  # Optional human-owned next-message suggestion; never auto-sent.

    model_config = ConfigDict(extra="forbid")


class ChatStreamData(BaseModel):
    """Streaming chunk for real-time message rendering (not persisted)."""

    stream_id: str  # Unique stream identifier
    op: Literal["start", "update", "end"]
    mode: Literal["snapshot", "delta"] = "snapshot"
    text: str = ""
    format: Literal["plain", "markdown"] = "plain"
    seq: int = 0  # Monotonic sequence number

    # IM semantics
    to: List[str] = Field(default_factory=list)
    reply_to: Optional[str] = None

    # Metadata
    client_id: Optional[str] = None

    model_config = ConfigDict(extra="forbid")


class ChatReactionData(BaseModel):
    """Message reaction (emoji)."""

    event_id: str  # Target message event_id
    actor_id: str  # Actor who reacted
    emoji: str  # Emoji reaction (e.g., ✅/❌/👍/🤔)

    model_config = ConfigDict(extra="forbid")
