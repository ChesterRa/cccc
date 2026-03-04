"""Cut point result schemas for context window management."""

from pydantic import BaseModel, Field

from .message import Message


class CutPointResult(BaseModel):
    """Cut point detection result for conversation compaction."""

    messages_to_summarize: list[Message] = Field(default_factory=list, description="Complete turns before cut point")
    turn_prefix_messages: list[Message] = Field(default_factory=list, description="Turn prefix if split turn")
    left_messages: list[Message] = Field(default_factory=list, description="Messages to keep from cut point onwards")

    is_split_turn: bool = Field(default=False, description="Whether cut point is mid-turn")
    cut_index: int = Field(default=0, description="Index of cut point in original message list")

    needs_compaction: bool = Field(default=False, description="Whether compaction is actually needed")
    token_count: int = Field(default=0, description="Total token count of original messages")
    threshold: int = Field(default=0, description="Token threshold that triggers compaction")
    accumulated_tokens: int = Field(default=0, description="Tokens accumulated when finding cut point")
